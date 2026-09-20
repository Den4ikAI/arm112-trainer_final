//! Журнал действий, учётные записи, версии классификатора.
//!
//! Раздел «Аудит» инструкции требует, чтобы было видно, кто, что и
//! когда сделал. Проверяем, что след остаётся у всех основных
//! действий, а не только у автоматических.
//!
//! Запуск: `cargo test --test admin_api`

use arm112::{api, config::Config, jobs::maintenance, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&config.database_url).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    Some((router, pool))
}

async fn call(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Value)
{
    call_with(app, m, path, token, body, &[]).await
}

/// С произвольными заголовками: нужно для проверки адреса за прокси.
async fn call_with(
    app: &Router, m: Method, path: &str, token: Option<&str>,
    body: Option<Value>, headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn token(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().unwrap().to_string()
}

/// Последняя запись аудита с таким действием.
///
/// Тесты идут параллельно и все входят в систему, поэтому там, где
/// важна именно своя запись, её ищут по метке — см. `last_marked`.
async fn last(pool: &PgPool, action: &str) -> Option<(Option<Uuid>, Option<String>, Option<Value>)> {
    sqlx::query_as(
        "SELECT user_id, host(ip), payload FROM audit_log
          WHERE action = $1 ORDER BY at DESC, id DESC LIMIT 1",
    )
    .bind(action)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Запись с заданным значением поля в подробностях.
async fn last_marked(
    pool: &PgPool, action: &str, field: &str, value: &str,
) -> Option<(Option<Uuid>, Option<String>, Option<Value>)> {
    sqlx::query_as(
        "SELECT user_id, host(ip), payload FROM audit_log
          WHERE action = $1 AND payload ->> $2 = $3
          ORDER BY at DESC, id DESC LIMIT 1",
    )
    .bind(action)
    .bind(field)
    .bind(value)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Вход — событие безопасности, и он должен быть виден.
#[tokio::test]
async fn вход_попадает_в_журнал_с_адресом() {
    let Some((app, pool)) = app().await else { return };
    // Метка, по которой найдём именно свою запись среди чужих
    let arm = format!("арм-{}", Uuid::new_v4().simple());

    call_with(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": "student1", "password": "arm112", "workstation": arm })),
        &[("x-forwarded-for", "10.20.30.40")]).await;

    let (user_id, ip, payload) = last_marked(&pool, "auth.login", "workstation", &arm)
        .await
        .expect("входа нет в журнале");
    assert!(user_id.is_some());
    assert_eq!(ip.as_deref(), Some("10.20.30.40"), "адрес за прокси не записан");
    assert_eq!(payload.unwrap()["workstation"], arm.as_str());
}

/// Неудачную попытку записываем с причиной: иначе перебор паролей
/// в журнале не отличить от забывчивости.
#[tokio::test]
async fn неудачный_вход_записывается_с_причиной() {
    let Some((app, pool)) = app().await else { return };

    let (status, _) = call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": "student1", "password": "не тот" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    assert!(last_marked(&pool, "auth.login.failed", "reason", "неверный пароль").await.is_some(),
        "неудачный вход не записан");
}

/// Пароль в журнал попасть не должен ни при каких условиях.
#[tokio::test]
async fn пароль_в_журнал_не_попадает() {
    let Some((app, pool)) = app().await else { return };
    let secret = "оченьСекретныйПароль42";

    call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": "student1", "password": secret }))).await;

    let found: Option<i64> = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE payload::text LIKE '%' || $1 || '%'")
        .bind(secret)
        .fetch_optional(&pool).await.unwrap();
    assert_eq!(found, Some(0));
}

/// Кто что отправил в службы — первое, что спрашивают при разборе
/// происшествия.
#[tokio::test]
async fn сохранение_карточки_оставляет_след() {
    let Some((app, pool)) = app().await else { return };
    let tok = token(&app, "student1").await;

    let Some(scenario) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(&pool).await.unwrap() else { return };

    let (_, s) = call(&app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    let session = s["session"]["id"].as_str().unwrap();

    call(&app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({
            "typeIds": ["101"],
            "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                         "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                         "inhouse": ["Крыша"], "gasified": ["Нет"] }
        }))).await;

    let (_, saved) = call(&app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&tok), None).await;
    let number = saved["card"]["number"].as_str().unwrap();

    let (_, _, payload) = last_marked(&pool, "card.save", "number", number)
        .await
        .expect("сохранения нет в журнале");
    let payload = payload.unwrap();
    assert_eq!(payload["number"], number);
    assert_eq!(payload["outcomeType"], "пожар: частный дом");
    // По журналу должно быть видно, кому карточка ушла
    assert!(payload["services"].as_array().unwrap().iter().any(|s| s == "mchs"));
    assert!(payload["score"].is_number());

    // И начало занятия тоже
    assert!(last(&pool, "session.start").await.is_some());
}

/// Журнал разбирают по объекту: «кто трогал эту карточку».
#[tokio::test]
async fn журнал_фильтруется_по_действию() {
    let Some((app, _)) = app().await else { return };
    let admin = token(&app, "admin").await;

    let (status, page) = call(&app, Method::GET,
        "/api/admin/audit?action=auth.login&size=10", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page["items"].as_array().unwrap().iter().all(|e| e["action"] == "auth.login"));
    // Список видов действий нужен интерфейсу для выпадающего списка
    assert!(page["actions"].as_array().unwrap().iter().any(|a| a == "auth.login"));
}

/// Журнал действий — только администратору.
///
/// ТЗ относит «просматривать системные журналы» к функциям
/// мониторинга администратора. Преподавателю для разбора занятия
/// дан другой инструмент — отчёты; кто когда входил и с какого
/// адреса, ему знать незачем.
#[tokio::test]
async fn журнал_действий_только_администратору() {
    let Some((app, _)) = app().await else { return };

    for who in ["student1", "teacher1"] {
        let tok = token(&app, who).await;
        let (status, _) = call(&app, Method::GET, "/api/admin/audit", Some(&tok), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who} не должен видеть журнал действий");
    }

    let admin = token(&app, "admin").await;
    let (status, _) = call(&app, Method::GET, "/api/admin/audit", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn администратор_заводит_учётную_запись() {
    let Some((app, pool)) = app().await else { return };
    let admin = token(&app, "admin").await;
    let login = format!("курсант-{}", Uuid::new_v4().simple());

    let (status, created) = call(&app, Method::POST, "/api/admin/users", Some(&admin),
        Some(json!({
            "login": login, "password": "длинныйПароль", "fio": "Новиков Н.Н.",
            "role": "student", "serviceId": "mchs", "workstation": "9"
        }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["serviceTitle"], "Классификатор МЧС");

    // Новый человек сразу может войти
    let (status, _) = call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "длинныйПароль" }))).await;
    assert_eq!(status, StatusCode::OK);

    // И заведение осталось в журнале
    assert!(last_marked(&pool, "user.create", "login", &login).await.is_some(),
        "заведение не попало в журнал");
}

/// Слабый пароль, чужая роль и служба у преподавателя — всё
/// отклоняется на сервере, а не только формой.
#[tokio::test]
async fn негодные_учётные_записи_отклоняются() {
    let Some((app, _)) = app().await else { return };
    let admin = token(&app, "admin").await;

    let bad = [
        json!({ "login": "a", "password": "корот", "fio": "Иванов", "role": "student" }),
        json!({ "login": "b", "password": "длинныйПароль", "fio": "Иванов", "role": "бог" }),
        json!({ "login": "c", "password": "длинныйПароль", "fio": "Иванов",
                "role": "teacher", "serviceId": "mchs" }),
        json!({ "login": "", "password": "длинныйПароль", "fio": "Иванов", "role": "student" }),
        // Курсант без службы: лента отбирается по ней, и без неё
        // у него пустой главный экран вместо занятия
        json!({ "login": "d", "password": "длинныйПароль", "fio": "Иванов", "role": "student" }),
    ];

    for body in bad {
        let (status, _) = call(&app, Method::POST, "/api/admin/users",
            Some(&admin), Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "прошло негодное: {body}");
    }
}

/// Учётные записи — целиком администраторские. ТЗ отдаёт ему
/// и создание, и назначение ролей, и блокировку; преподавателя
/// в этом списке нет.
#[tokio::test]
async fn учётные_записи_только_администратору() {
    let Some((app, _)) = app().await else { return };
    let teacher = token(&app, "teacher1").await;

    let (status, _) = call(&app, Method::GET, "/api/admin/users", Some(&teacher), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "преподаватель не видит список людей");

    let (status, _) = call(&app, Method::POST, "/api/admin/users", Some(&teacher),
        Some(json!({ "login": "x", "password": "длинныйПароль",
                     "fio": "Иванов", "role": "student" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(&app, Method::GET, "/api/admin/classifier-versions",
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "версии классификатора — конфигурация системы");
}

/// Но переаттестация — его: смена справочника означает, что часть
/// сценариев перестала соответствовать эталону.
#[tokio::test]
async fn последствия_смены_версии_преподаватель_видит() {
    let Some((app, pool)) = app().await else { return };
    let teacher = token(&app, "teacher1").await;

    let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM classifier_versions WHERE name LIKE 'v046_11%' LIMIT 1")
        .fetch_optional(&pool).await.unwrap() else { return };

    let (status, _) = call(&app, Method::GET,
        &format!("/api/classifier/versions/{id}/impact"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);
}

/// Отключённый человек в систему не входит, но его история остаётся.
#[tokio::test]
async fn отключённая_запись_не_пускает_в_систему() {
    let Some((app, _)) = app().await else { return };
    let admin = token(&app, "admin").await;
    let login = format!("отключу-{}", Uuid::new_v4().simple());

    let (_, created) = call(&app, Method::POST, "/api/admin/users", Some(&admin),
        Some(json!({ "login": login, "password": "длинныйПароль",
                     "fio": "Петров П.П.", "role": "student",
                     "serviceId": "mchs" }))).await;
    let id = created["id"].as_str().unwrap();

    let (status, _) = call(&app, Method::POST, &format!("/api/admin/users/{id}/active"),
        Some(&admin), Some(json!({ "active": false }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "длинныйПароль" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Запись осталась в списке — история по ней должна читаться
    let (_, users) = call(&app, Method::GET, "/api/admin/users", Some(&admin), None).await;
    assert!(users.as_array().unwrap().iter()
        .any(|u| u["login"] == login.as_str() && u["isActive"] == false));
}

/// Отключив себя, администратор запер бы систему.
#[tokio::test]
async fn администратор_не_отключает_сам_себя() {
    let Some((app, pool)) = app().await else { return };
    let admin = token(&app, "admin").await;

    let id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'admin'")
        .fetch_one(&pool).await.unwrap();

    let (status, _) = call(&app, Method::POST, &format!("/api/admin/users/{id}/active"),
        Some(&admin), Some(json!({ "active": false }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Активная версия должна быть ровно одна: по ней считается и список
/// оповещения, и эталон при разборе.
#[tokio::test]
async fn версии_классификатора_показываются() {
    let Some((app, _)) = app().await else { return };
    let admin = token(&app, "admin").await;

    let (status, versions) = call(&app, Method::GET,
        "/api/admin/classifier-versions", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);

    let list = versions.as_array().unwrap();
    assert!(!list.is_empty(), "классификатор не импортирован");
    assert_eq!(list.iter().filter(|v| v["isActive"] == true).count(), 1,
        "активной должна быть ровно одна версия");
}

/// Партиции аудита созданы миграцией до декабря 2026. Как только
/// этот срок пройдёт, запись в журнал начнёт падать, а вместе с ней
/// и действие, которое её вызвало.
#[tokio::test]
async fn партиции_аудита_создаются_наперёд() {
    let Some((_, pool)) = app().await else { return };

    maintenance::ensure_partitions(&pool).await.unwrap();

    // Текущий месяц и три вперёд
    let covered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
            SELECT to_char(date_trunc('month', now() + make_interval(months => g)), 'YYYY_MM') AS m
              FROM generate_series(0, 3) AS g
         ) months
         WHERE to_regclass('audit_log_' || m) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(covered, 4, "не хватает партиций на ближайшие месяцы");

    // Повторный вызов ничего не ломает: задача бежит каждые сутки
    assert!(maintenance::ensure_partitions(&pool).await.unwrap().is_empty());
}
