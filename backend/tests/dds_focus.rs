//! Финал: тренажёр сосредоточен на работе диспетчера ДДС.
//!
//! Обратная связь экспертов 18.10:
//! * логины и пароли учебного стенда — на экране входа (`/api/stand`);
//! * приём вызовов 112 свернуть (параметр `ui.op112`);
//! * показать, как ИИ формирует карточки для специалиста ДДС:
//!   преподавателю — «собрать для службы» (`/api/dds-incidents/generate`),
//!   курсанту — самостоятельная тренировка (`/api/dds-incidents/practice`).
//!
//! Модель здесь — заглушка: она текста не пишет, и карточка обязана
//! собраться без неё. Признаки и службы считает классификатор, поэтому
//! от модели не зависят.
//!
//! Запуск: `cargo test --test dds_focus`

use arm112::{api, config::Config, ml::MlSuite, state::AppState};
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

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app_with(demo: Option<&str>) -> Option<(Router, PgPool)> {
    let mut config = Config::from_env().ok()?;
    config.demo_password = demo.map(str::to_string);
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    Some((router, pool))
}

async fn app() -> Option<(Router, PgPool)> {
    app_with(None).await
}

async fn call(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Value)
{
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn токен(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().expect("вход не удался").to_string()
}

/// Два диспетчера одной службы в своей новой группе: у второго не
/// должно появляться то, что первый заказал себе сам.
struct Пара {
    первый: (Uuid, String),
    второй: (Uuid, String),
    группа: Uuid,
}

async fn пара_диспетчеров(pool: &PgPool, service: &str) -> Пара {
    let метка = Uuid::new_v4().simple().to_string();
    let mut люди = Vec::new();
    for n in 1..=2 {
        let login = format!("трен{n}-{}", &метка[..10]);
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO users (login, password_hash, fio, role, service_id)
             SELECT $1, password_hash, 'Тренировкин Т.Т.', 'student', $2
               FROM users WHERE login = 'student1'
             RETURNING id",
        )
        .bind(&login).bind(service).fetch_one(pool).await.unwrap();
        люди.push((id, login));
    }
    let группа: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id)
         SELECT $1, id FROM users WHERE login = 'teacher1' RETURNING id",
    )
    .bind(format!("тренировка {метка}")).fetch_one(pool).await.unwrap();
    for (id, _) in &люди {
        sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
            .bind(группа).bind(id).execute(pool).await.unwrap();
    }
    let второй = люди.pop().unwrap();
    let первый = люди.pop().unwrap();
    Пара { первый, второй, группа }
}

async fn убрать(pool: &PgPool, п: &Пара) {
    let cards: Vec<Uuid> = sqlx::query_scalar(
        "SELECT lc.card_id FROM lesson_cards lc JOIN lessons l ON l.id = lc.lesson_id
          WHERE l.group_id = $1",
    )
    .bind(п.группа).fetch_all(pool).await.unwrap();
    sqlx::query("DELETE FROM training_groups WHERE id = $1").bind(п.группа).execute(pool).await.ok();
    for c in cards {
        sqlx::query("DELETE FROM cards WHERE id = $1").bind(c).execute(pool).await.ok();
    }
    for (id, _) in [&п.первый, &п.второй] {
        sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(id).execute(pool).await.ok();
        sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(pool).await.ok();
    }
}

async fn op112(pool: &PgPool) -> f64 {
    sqlx::query_scalar::<_, Value>("SELECT value FROM settings WHERE key = 'ui.op112'")
        .fetch_one(pool).await.unwrap().as_f64().unwrap()
}

// --- Экран входа ---------------------------------------------------------

#[tokio::test]
async fn без_пароля_стенда_учётные_записи_не_показываются() {
    let Some((app, _)) = app().await else { return };
    // Без входа: экран входа спрашивает до того, как кто-то вошёл
    let (st, v) = call(&app, Method::GET, "/api/stand", None, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["demo"].is_null(), "рабочая установка не раскрывает учётные записи: {v}");
    assert!(v["op112"].is_boolean());
}

#[tokio::test]
async fn стенд_показывает_учётные_записи_и_сворачивает_оператора_112() {
    let Some((app, pool)) = app_with(Some("arm112")).await else { return };
    let было = op112(&pool).await;
    let admin = токен(&app, "admin").await;

    let (st, _) = call(&app, Method::PATCH, "/api/settings/ui.op112", Some(&admin),
        Some(json!({ "value": 0 }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = call(&app, Method::GET, "/api/stand", None, None).await;
    assert_eq!(v["op112"], false);
    assert_eq!(v["demo"]["password"], "arm112");
    let accounts = v["demo"]["accounts"].as_array().unwrap();
    let logins: Vec<&str> = accounts.iter().map(|a| a["login"].as_str().unwrap()).collect();
    assert!(logins.contains(&"teacher1"), "преподаватель на экране входа: {logins:?}");
    assert!(logins.contains(&"admin"));
    // Первым — преподаватель: с него начинается показ
    assert_eq!(accounts[0]["role"], "teacher");
    // Диспетчер каждой службы — по одному, с названием службы
    let ддс: Vec<&Value> = accounts.iter().filter(|a| a["duty"] == "dds").collect();
    assert!(!ддс.is_empty());
    assert!(ддс.iter().all(|a| a["service"].is_string()));
    let mut службы: Vec<&str> = ддс.iter().map(|a| a["service"].as_str().unwrap()).collect();
    let всего = службы.len();
    службы.dedup();
    assert_eq!(службы.len(), всего, "по одной записи на службу: {службы:?}");
    assert!(accounts.iter().all(|a| a["duty"] != "op112"),
            "оператор 112 свёрнут — его учётной записи на входе нет");

    // Администратор разворачивает 112 — и оператор возвращается
    call(&app, Method::PATCH, "/api/settings/ui.op112", Some(&admin),
        Some(json!({ "value": 1 }))).await;
    let (_, v) = call(&app, Method::GET, "/api/stand", None, None).await;
    assert_eq!(v["op112"], true);
    assert!(v["demo"]["accounts"].as_array().unwrap().iter().any(|a| a["duty"] == "op112"));

    // Преподаватель параметр не меняет: он общий для всех групп
    let teacher = токен(&app, "teacher1").await;
    let (st, _) = call(&app, Method::PATCH, "/api/settings/ui.op112", Some(&teacher),
        Some(json!({ "value": 0 }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    sqlx::query("UPDATE settings SET value = to_jsonb($1::float8) WHERE key = 'ui.op112'")
        .bind(было).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn чужой_пароль_стенда_не_раскрывает_ни_одной_записи() {
    // Пароль в окружении не подходит ни к одной учётной записи — список
    // пуст: на экране входа не может оказаться неработающая пара
    let Some((app, _)) = app_with(Some("не-тот-пароль")).await else { return };
    let (_, v) = call(&app, Method::GET, "/api/stand", None, None).await;
    assert_eq!(v["demo"]["accounts"].as_array().unwrap().len(), 0, "{v}");
}

// --- Происшествие, собранное системой ------------------------------------

#[tokio::test]
async fn преподаватель_собирает_происшествие_для_службы_и_утверждает() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;

    // Мослифт по классификатору получает карточку только для сведения —
    // его добавляют вручную, как в учебном наборе
    for service in ["mchs", "mosgaz", "smp", "codd", "moslift", "dep-zhkh"] {
        let (st, inc) = call(&app, Method::POST, "/api/dds-incidents/generate", Some(&t),
            Some(json!({ "serviceId": service }))).await;
        assert_eq!(st, StatusCode::CREATED, "{service}: {inc}");
        assert_eq!(inc["source"], "ai");
        assert_eq!(inc["status"], "draft", "собранное — черновик, решает преподаватель");
        assert!(inc["services"].as_array().unwrap().iter().any(|s| s == service),
                "{service} в службах карточки: {inc}");
        assert!(!inc["typeIds"].as_array().unwrap().is_empty());
        assert!(inc["outcomeType"].is_string(), "итоговый тип посчитан классификатором: {inc}");
        assert!(inc["address"]["street"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(!inc["description"].as_str().unwrap().is_empty());
        if service == "moslift" || service == "dep-zhkh" {
            // Обычно добавлен вручную. Но «Карточка-112» с заглавной
            // классификатор считает реагированием (сравнение с учётом
            // регистра, domain::classify) — тогда он подобран сам
            let вручную = inc["manualServices"].as_array().unwrap().iter().any(|s| s == service);
            let сам = inc["classifyReason"].is_null();
            assert!(вручную || сам, "{inc}");
        } else {
            assert!(inc["manualServices"].as_array().unwrap().is_empty(),
                    "{service} подобрана классификатором, а не добавлена: {inc}");
        }

        // Полная карточка: утверждается без правок
        let id = inc["id"].as_str().unwrap();
        let (st, ok) = call(&app, Method::POST, &format!("/api/dds-incidents/{id}/approve"),
            Some(&t), None).await;
        assert_eq!(st, StatusCode::OK, "{service}: {ok}");
        sqlx::query("DELETE FROM cards WHERE id = $1::uuid").bind(id).execute(&pool).await.unwrap();
    }
}

#[tokio::test]
async fn ответы_карты_собранного_происшествия_ведут_к_той_же_службе() {
    // Повторная классификация сохранённых ответов даёт ту же службу:
    // карточка не противоречит сама себе
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    for _ in 0..5 {
        let (_, inc) = call(&app, Method::POST, "/api/dds-incidents/generate", Some(&t),
            Some(json!({ "serviceId": "smp" }))).await;
        let id = inc["id"].as_str().unwrap();
        let (st, again) = call(&app, Method::POST, "/api/classifier/evaluate", Some(&t),
            Some(json!({ "typeIds": inc["typeIds"], "answers": inc["answers"] }))).await;
        assert_eq!(st, StatusCode::OK, "{again}");
        assert!(again["services"].as_array().unwrap().iter().any(|s| s["serviceId"] == "smp"),
                "{again}");
        sqlx::query("DELETE FROM cards WHERE id = $1::uuid").bind(id).execute(&pool).await.unwrap();
    }
}

#[tokio::test]
async fn службе_без_позиций_собирать_не_из_чего() {
    let Some((app, _)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    let (st, v) = call(&app, Method::POST, "/api/dds-incidents/generate", Some(&t),
        Some(json!({ "serviceId": "нет-такой-службы" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

#[tokio::test]
async fn собирать_для_службы_может_только_преподаватель() {
    let Some((app, _)) = app().await else { return };
    let s = токен(&app, "student3").await;
    let (st, _) = call(&app, Method::POST, "/api/dds-incidents/generate", Some(&s),
        Some(json!({ "serviceId": "mchs" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

// --- Самостоятельная тренировка диспетчера -------------------------------

#[tokio::test]
async fn диспетчер_получает_происшествие_от_ии_только_себе() {
    let Some((app, pool)) = app().await else { return };
    let п = пара_диспетчеров(&pool, "mchs").await;
    let первый = токен(&app, &п.первый.1).await;
    let второй = токен(&app, &п.второй.1).await;

    let (st, v) = call(&app, Method::POST, "/api/dds-incidents/practice", Some(&первый), None).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let card = v["cardId"].as_str().unwrap().to_string();

    // Сразу в ленте своей службы, открывается
    let (_, лента) = call(&app, Method::GET, "/api/cards?scope=service", Some(&первый), None).await;
    assert!(лента["items"].as_array().unwrap().iter().any(|c| c["id"] == card.as_str()),
            "происшествие в ленте: {лента}");
    let (st, детали) = call(&app, Method::GET, &format!("/api/cards/{card}"), Some(&первый), None).await;
    assert_eq!(st, StatusCode::OK, "{детали}");

    // В «Учебных происшествиях» — не начато, а не «не пройдено»: личное
    // занятие завершено сразу при выдаче
    let (_, работа) = call(&app, Method::GET, "/api/me/work?section=training", Some(&первый), None).await;
    let строка = работа.as_array().unwrap().iter().find(|w| w["cardId"] == card.as_str())
        .unwrap_or_else(|| panic!("нет в разделе: {работа}"));
    assert_eq!(строка["status"], "not_started");
    assert_eq!(строка["lessonTitle"], "Самостоятельная тренировка с ИИ");

    // Соседу по группе той же службы — не видно ни в ленте, ни по ссылке
    let (_, лента2) = call(&app, Method::GET, "/api/cards?scope=service", Some(&второй), None).await;
    assert!(лента2["items"].as_array().unwrap().iter().all(|c| c["id"] != card.as_str()));
    assert_eq!(лента2["total"], 0, "{лента2}");
    let (st, _) = call(&app, Method::GET, &format!("/api/cards/{card}"), Some(&второй), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, работа2) = call(&app, Method::GET, "/api/me/work?section=training", Some(&второй), None).await;
    assert!(работа2.as_array().unwrap().iter().all(|w| w["cardId"] != card.as_str()));

    // В «Учебном материале» преподавателя его нет: это работа курсанта
    let t = токен(&app, "teacher1").await;
    let (_, материал) = call(&app, Method::GET, "/api/dds-incidents", Some(&t), None).await;
    assert!(материал.as_array().unwrap().iter().all(|i| i["id"] != card.as_str()));
    // А в журнале преподавателя — есть, как любая выданная карточка
    let (_, журнал) = call(&app, Method::GET, &format!("/api/cards/{card}"), Some(&t), None).await;
    assert_eq!(журнал["id"], card.as_str());

    убрать(&pool, &п).await;
}

#[tokio::test]
async fn личная_тренировка_не_подменяет_ленту_идущего_занятия() {
    // Пока идёт занятие группы, лента курсанта — только его карточки.
    // Личная тренировка во время занятия эту ленту не перехватывает
    let Some((app, pool)) = app().await else { return };
    let п = пара_диспетчеров(&pool, "mosgaz").await;
    let tok = токен(&app, &п.первый.1).await;
    let занятие: Uuid = sqlx::query_scalar(
        "INSERT INTO lessons (group_id, teacher_id, title, mode, started_at)
         SELECT $1, id, 'идёт', 'dds', now() FROM users WHERE login = 'teacher1' RETURNING id",
    )
    .bind(п.группа).fetch_one(&pool).await.unwrap();

    let (st, v) = call(&app, Method::POST, "/api/dds-incidents/practice", Some(&tok), None).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let (_, лента) = call(&app, Method::GET, "/api/cards?scope=service", Some(&tok), None).await;
    assert!(лента["items"].as_array().unwrap().iter().all(|c| c["id"] != v["cardId"]),
            "идёт занятие — в ленте только его карточки: {лента}");
    // Сама карточка доступна по ссылке, из раздела «Учебные происшествия»
    let (st, _) = call(&app, Method::GET, &format!("/api/cards/{}", v["cardId"].as_str().unwrap()),
        Some(&tok), None).await;
    assert_eq!(st, StatusCode::OK);

    sqlx::query("DELETE FROM lessons WHERE id = $1").bind(занятие).execute(&pool).await.unwrap();
    убрать(&pool, &п).await;
}

#[tokio::test]
async fn тренировку_с_ии_берёт_только_диспетчер_ддс() {
    let Some((app, _)) = app().await else { return };
    for login in ["student1", "teacher1"] {
        let tok = токен(&app, login).await;
        let (st, v) = call(&app, Method::POST, "/api/dds-incidents/practice", Some(&tok), None).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{login}: {v}");
    }
}

#[tokio::test]
async fn диспетчер_вне_группы_получает_понятный_отказ() {
    let Some((app, pool)) = app().await else { return };
    let login = format!("одиночка-{}", &Uuid::new_v4().simple().to_string()[..10]);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Одиночкин О.О.', 'student', 'codd'
           FROM users WHERE login = 'student1' RETURNING id",
    )
    .bind(&login).fetch_one(&pool).await.unwrap();
    let tok = токен(&app, &login).await;
    let (st, v) = call(&app, Method::POST, "/api/dds-incidents/practice", Some(&tok), None).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(&pool).await.ok();
}
