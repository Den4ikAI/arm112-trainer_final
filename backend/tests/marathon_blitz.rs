//! Марафон, блиц и удаление учётной записи администратором.
//!
//! Пожелания команды: «блиц — это обучение на конкретных сегментах
//! карточки, а марафон — это экзамен на 5–10 карточек»;
//! «администратор может удалять и добавлять учеников и
//! преподавателей». ТЗ при этом запрещает удалять критически важные
//! данные без резервного копирования.
//!
//! Запуск: `cargo test --test marathon_blitz`
//!
//! Каждому тесту — свой курсант, своя группа и свои копии вызовов:
//! прогон идёт параллельно по одной базе.

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

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    Some((router, pool))
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

/// Свой курсант в своей группе и свои копии утверждённого вызова.
struct Стенд {
    user_id: Uuid,
    login: String,
    group_id: Uuid,
    вызовы: Vec<Uuid>,
}

async fn стенд(pool: &PgPool, вызовов: usize) -> Стенд {
    let метка = Uuid::new_v4().simple().to_string();
    let login = format!("марафонец-{}", &метка[..12]);

    // Пароль тот же, что у остальных учебных учёток
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Марафонцев М.М.', 'student', 'mchs'
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).fetch_one(pool).await.unwrap();

    let teacher: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(pool).await.unwrap();
    let group_id: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id")
        .bind(format!("марафон-{}", &метка[..12]))
        .bind(teacher)
        .fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(group_id).bind(user_id).execute(pool).await.unwrap();

    // Копии настоящего утверждённого вызова: в тестовой базе их два,
    // а марафону нужно пять
    let образец: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE status = 'approved' AND ticket IS NOT NULL
          ORDER BY ticket, call_no LIMIT 1",
    )
    .fetch_one(pool).await.unwrap();

    let mut вызовы = Vec::new();
    for _ in 0..вызовов {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO scenarios (legend, applicant_name, applicant_phone, applicant_status,
                                    norm_sec, expected, version_id, status, created_by)
             SELECT legend, applicant_name, applicant_phone, applicant_status,
                    norm_sec, expected, version_id, 'approved', created_by
               FROM scenarios WHERE id = $1
             RETURNING id",
        )
        .bind(образец).fetch_one(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO scenario_hidden_facts (scenario_id, key, question, answer, unlocks, sort_order)
             SELECT $2, key, question, answer, unlocks, sort_order
               FROM scenario_hidden_facts WHERE scenario_id = $1",
        )
        .bind(образец).bind(id).execute(pool).await.unwrap();
        вызовы.push(id);
    }

    Стенд { user_id, login, group_id, вызовы }
}

async fn прибрать(pool: &PgPool, с: &Стенд) {
    for sql in [
        "UPDATE lessons SET finished_at = now() WHERE group_id = $1 AND finished_at IS NULL
            AND started_at IS NOT NULL",
        "DELETE FROM sessions WHERE user_id = $2",
        "DELETE FROM assignments WHERE lesson_id IN (SELECT id FROM lessons WHERE group_id = $1)",
        "DELETE FROM lessons WHERE group_id = $1",
        "DELETE FROM training_groups WHERE id = $1",
        "UPDATE audit_log SET user_id = NULL WHERE user_id = $2",
        "DELETE FROM users WHERE id = $2",
    ] {
        sqlx::query(sql).bind(с.group_id).bind(с.user_id).execute(pool).await.ok();
    }
    for v in &с.вызовы {
        sqlx::query("DELETE FROM scenarios WHERE id = $1").bind(v).execute(pool).await.ok();
    }
}

async fn занятие(app: &Router, teacher: &str, тело: Value) -> (StatusCode, Value) {
    call(app, Method::POST, "/api/lessons", Some(teacher), Some(тело)).await
}

/// Начать вызов и сразу сохранить карточку — без вопросов и полей.
async fn пройти(app: &Router, курсант: &str, вызов: Uuid) -> (Value, Value) {
    let (status, начато) = call(app, Method::POST, "/api/sessions", Some(курсант),
        Some(json!({ "scenarioId": вызов, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{начато}");
    let id = начато["session"]["id"].as_str().unwrap().to_string();
    let (status, сохранено) = call(app, Method::POST, &format!("/api/sessions/{id}/save"),
        Some(курсант), None).await;
    assert_eq!(status, StatusCode::OK, "{сохранено}");
    (начато, сохранено)
}

// ─── Марафон ─────────────────────────────────────────────────────────

/// Экзамен на пять вызовов: строго по порядку, одна попытка на вызов,
/// разбора до последнего вызова нет, в конце — протокол.
#[tokio::test]
async fn марафон_идёт_по_порядку_и_прячет_разбор_до_конца() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 5).await;
    let teacher = токен(&app, "teacher1").await;

    let (status, создано) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "Экзамен", "mode": "card112",
        "kind": "marathon", "scenarioIds": с.вызовы,
    })).await;
    assert_eq!(status, StatusCode::CREATED, "{создано}");
    assert_eq!(создано["kind"], "marathon");
    assert_eq!(создано["callCount"], 5);
    let id = создано["id"].as_str().unwrap().to_string();

    let (status, начато) = call(&app, Method::POST, &format!("/api/lessons/{id}/start"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");
    assert_eq!(начато["items"], 5);

    let курсант = токен(&app, &с.login).await;

    // В списке только первый вызов
    let (_, список) = call(&app, Method::GET, "/api/scenarios", Some(&курсант), None).await;
    let видно: Vec<&str> = список.as_array().unwrap().iter()
        .map(|v| v["id"].as_str().unwrap()).collect();
    assert_eq!(видно, vec![с.вызовы[0].to_string().as_str()], "видно больше одного вызова");

    // Вперёд не забежать
    let (status, _) = call(&app, Method::POST, "/api/sessions", Some(&курсант),
        Some(json!({ "scenarioId": с.вызовы[1], "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "второй вызов начат раньше первого");

    let mut первая_попытка = String::new();
    for (n, вызов) in с.вызовы.iter().enumerate() {
        let (начато, сохранено) = пройти(&app, &курсант, *вызов).await;
        let session = начато["session"]["id"].as_str().unwrap().to_string();
        if n == 0 {
            первая_попытка = session.clone();
        }

        if n < 4 {
            assert!(сохранено["grade"].is_null(), "разбор открыт посреди марафона: {сохранено}");
            let (status, _) = call(&app, Method::GET, &format!("/api/sessions/{session}/grade"),
                Some(&курсант), None).await;
            assert_eq!(status, StatusCode::CONFLICT);
            // И после обновления страницы тоже закрыт
            let (_, состояние) = call(&app, Method::GET, &format!("/api/sessions/{session}"),
                Some(&курсант), None).await;
            assert!(состояние["grade"].is_null());

            // Протокол курсанта — без баллов
            let (_, протокол) = call(&app, Method::GET, &format!("/api/lessons/{id}/protocol"),
                Some(&курсант), None).await;
            assert_eq!(протокол["rows"][0]["hidden"], true);
            assert!(протокол["rows"][0]["calls"][n]["score"].is_null());
            assert_eq!(протокол["rows"][0]["done"], n as i64 + 1);

            // Преподаватель видит балл сразу — он принимает экзамен
            let (status, разбор) = call(&app, Method::GET,
                &format!("/api/sessions/{session}/grade"), Some(&teacher), None).await;
            assert_eq!(status, StatusCode::OK, "{разбор}");
        } else {
            assert!(сохранено["grade"]["score"].is_number(),
                    "после последнего вызова разбор открывается: {сохранено}");
        }
    }

    // Все пройдены — список пуст, повторить нельзя
    let (_, список) = call(&app, Method::GET, "/api/scenarios", Some(&курсант), None).await;
    assert!(список.as_array().unwrap().is_empty(), "{список}");
    let (status, _) = call(&app, Method::POST, "/api/sessions", Some(&курсант),
        Some(json!({ "scenarioId": с.вызовы[0], "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "в марафоне вторая попытка");

    // Разбор первого вызова теперь открыт
    let (status, _) = call(&app, Method::GET, &format!("/api/sessions/{первая_попытка}/grade"),
        Some(&курсант), None).await;
    assert_eq!(status, StatusCode::OK);

    // Итоговый протокол
    let (_, протокол) = call(&app, Method::GET, &format!("/api/lessons/{id}/protocol"),
        Some(&курсант), None).await;
    let строка = &протокол["rows"][0];
    assert_eq!(строка["hidden"], false);
    assert_eq!(строка["done"], 5);
    assert_eq!(строка["total"], 5);
    assert!(строка["avgScore"].is_number());
    assert!(строка["calls"].as_array().unwrap().iter().all(|c| c["score"].is_number()));
    // Порядок — как выбрал преподаватель
    let порядок: Vec<&str> = строка["calls"].as_array().unwrap().iter()
        .map(|c| c["scenarioId"].as_str().unwrap()).collect();
    let ожидали: Vec<String> = с.вызовы.iter().map(|v| v.to_string()).collect();
    assert_eq!(порядок, ожидали.iter().map(String::as_str).collect::<Vec<_>>());
    // Пустая карточка: службы не направлены — грубые ошибки есть
    assert!(строка["grossCount"].as_i64().unwrap() > 0, "{строка}");

    // Протокол чужого занятия курсанту не положен
    let (status, _) = call(&app, Method::GET, &format!("/api/lessons/{id}/protocol"),
        Some(&токен(&app, "student1").await), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    прибрать(&pool, &с).await;
}

/// «Экзамен на 5–10 карточек»: меньше пяти и больше десяти не
/// собрать, и марафон бывает только у приёма вызова.
#[tokio::test]
async fn марафон_собирается_только_из_пяти_до_десяти_вызовов() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 4).await;
    let teacher = токен(&app, "teacher1").await;

    let (status, ответ) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "Мало", "mode": "card112",
        "kind": "marathon", "scenarioIds": с.вызовы,
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");

    let (status, _) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "Много", "mode": "card112",
        "kind": "marathon", "callCount": 11,
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "ДДС", "mode": "dds", "kind": "marathon",
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Выбрать можно только утверждённые
    let черновик: Uuid = sqlx::query_scalar(
        "UPDATE scenarios SET status = 'draft' WHERE id = $1 RETURNING id")
        .bind(с.вызовы[0]).fetch_one(&pool).await.unwrap();
    let mut выбор = с.вызовы.clone();
    выбор.push(черновик);
    let (status, _) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "Черновик", "mode": "card112",
        "kind": "marathon", "scenarioIds": выбор,
    })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    прибрать(&pool, &с).await;
}

// ─── Блиц ────────────────────────────────────────────────────────────

/// Блиц по адресу: оценивается только адрес, а незаполненные службы
/// и тип не штрафуются. Разбор — сразу после вызова.
#[tokio::test]
async fn блиц_оценивает_только_свой_сегмент() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 3).await;
    let teacher = токен(&app, "teacher1").await;

    let (status, создано) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "Адрес", "mode": "card112",
        "kind": "blitz", "segment": "address", "scenarioIds": с.вызовы,
    })).await;
    assert_eq!(status, StatusCode::CREATED, "{создано}");
    let id = создано["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;

    let курсант = токен(&app, &с.login).await;
    let вызов = с.вызовы[0];

    // Адрес заполнен по эталону, остальное пусто
    let адрес: Value = sqlx::query_scalar("SELECT expected->'address' FROM scenarios WHERE id = $1")
        .bind(вызов).fetch_one(&pool).await.unwrap();
    let (status, начато) = call(&app, Method::POST, "/api/sessions", Some(&курсант),
        Some(json!({ "scenarioId": вызов, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{начато}");
    let session = начато["session"]["id"].as_str().unwrap().to_string();
    call(&app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&курсант),
        Some(json!({ "address": адрес }))).await;
    let (_, сохранено) = call(&app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&курсант), None).await;

    let разбор = &сохранено["grade"];
    assert_eq!(разбор["segment"], "address", "{сохранено}");
    let поля: Vec<&str> = разбор["findings"].as_array().unwrap().iter()
        .map(|f| f["field"].as_str().unwrap()).collect();
    assert!(!поля.is_empty());
    assert!(поля.iter().all(|f| f.starts_with("address.")), "в разбор блица попало чужое: {поля:?}");
    // Все поля адреса верны; «угадал» за адрес без вопроса — это не
    // ноль, но и не штраф за пустые службы
    let полный: i32 = sqlx::query_scalar("SELECT score FROM grades WHERE session_id = $1::uuid")
        .bind(&session).fetch_one(&pool).await.unwrap();
    assert!(разбор["score"].as_i64().unwrap() > полный as i64,
            "балл сегмента должен быть выше балла за пустую карточку: {} против {полный}",
            разбор["score"]);
    assert_eq!(разбор["overNorm"], false, "время в блице не оценивается");

    // Протокол — балл по сегменту
    let (_, протокол) = call(&app, Method::GET, &format!("/api/lessons/{id}/protocol"),
        Some(&teacher), None).await;
    assert_eq!(протокол["segmentLabel"], "Адрес");
    assert_eq!(протокол["rows"][0]["calls"].as_array().unwrap().iter()
                   .find(|c| c["finished"] == true).unwrap()["score"],
               разбор["score"]);

    // В блице можно начинать любой вызов серии — порядок не строгий
    let (status, _) = call(&app, Method::POST, "/api/sessions", Some(&курсант),
        Some(json!({ "scenarioId": с.вызовы[2], "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED);

    прибрать(&pool, &с).await;
}

#[tokio::test]
async fn блиц_без_годного_сегмента_не_собирается() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 3).await;
    let teacher = токен(&app, "teacher1").await;

    for тело in [
        json!({ "groupId": с.group_id, "title": "Без сегмента", "mode": "card112", "kind": "blitz" }),
        json!({ "groupId": с.group_id, "title": "Не тот", "mode": "card112", "kind": "blitz",
                "segment": "statuses" }),
        json!({ "groupId": с.group_id, "title": "ДДС не тот", "mode": "dds", "kind": "blitz",
                "segment": "address" }),
        json!({ "groupId": с.group_id, "title": "Сегмент обычному", "mode": "card112",
                "segment": "address" }),
    ] {
        let (status, ответ) = занятие(&app, &teacher, тело.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{тело} → {ответ}");
    }

    // Блиц ДДС по статусам собирается
    let (status, ответ) = занятие(&app, &teacher, json!({
        "groupId": с.group_id, "title": "Статусы", "mode": "dds",
        "kind": "blitz", "segment": "statuses",
    })).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");
    assert_eq!(ответ["segment"], "statuses");

    прибрать(&pool, &с).await;
}

// ─── Удаление учётной записи ─────────────────────────────────────────

async fn завести(app: &Router, admin: &str, роль: &str) -> (String, String) {
    let login = format!("удаляемый-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let (status, created) = call(app, Method::POST, "/api/admin/users", Some(admin),
        Some(json!({ "login": login, "password": "длинныйПароль", "fio": "Удаляев У.У.",
                     "role": роль, "serviceId": if роль == "student" { json!("mchs") } else { Value::Null } }))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (created["id"].as_str().unwrap().to_string(), login)
}

/// Завели по ошибке и ни разу не вошли — стирается сразу, терять нечего.
#[tokio::test]
async fn запись_без_истории_удаляется_сразу() {
    let Some((app, pool)) = app().await else { return };
    let admin = токен(&app, "admin").await;
    let (id, _) = завести(&app, &admin, "teacher").await;

    let (status, ответ) = call(&app, Method::DELETE, &format!("/api/admin/users/{id}"),
        Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    assert_eq!(ответ["mode"], "deleted");
    assert!(ответ["backupId"].is_null());

    let есть: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1::uuid)")
        .bind(&id).fetch_one(&pool).await.unwrap();
    assert!(!есть);

    let след: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM audit_log WHERE action = 'user.delete' AND entity_id = $1)")
        .bind(&id).fetch_one(&pool).await.unwrap();
    assert!(след, "удаление должно остаться в журнале действий");
}

/// Запись с историей: сначала резервная копия, потом обезличивание.
/// Оценки остаются, человек из системы исчезает.
#[tokio::test]
async fn запись_с_историей_копируется_и_обезличивается() {
    let Some((app, pool)) = app().await else { return };
    let admin = токен(&app, "admin").await;
    let (id, login) = завести(&app, &admin, "student").await;

    // Вошёл и отработал вызов — история есть
    let (_, вход) = call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "длинныйПароль" }))).await;
    let его = вход["token"].as_str().unwrap().to_string();
    let вызов: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE status = 'approved' AND ticket IS NOT NULL LIMIT 1")
        .fetch_one(&pool).await.unwrap();
    let (начато, _) = пройти(&app, &его, вызов).await;
    let session = начато["session"]["id"].as_str().unwrap().to_string();

    let (status, ответ) = call(&app, Method::DELETE, &format!("/api/admin/users/{id}"),
        Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    assert_eq!(ответ["mode"], "anonymized");
    let копия = ответ["backupId"].as_str().expect("без резервной копии удалять нельзя");

    // В копии — то, что стёрто из записи
    let данные: Value = sqlx::query_scalar("SELECT data FROM user_backups WHERE id = $1::uuid")
        .bind(копия).fetch_one(&pool).await.unwrap();
    assert_eq!(данные["user"]["login"], login.as_str());
    assert_eq!(данные["user"]["fio"], "Удаляев У.У.");
    assert_eq!(данные["sessions"].as_array().unwrap().len(), 1);

    // Запись обезличена, оценка на месте
    let (фио, логин, активна): (String, String, bool) = sqlx::query_as(
        "SELECT fio, login, is_active FROM users WHERE id = $1::uuid")
        .bind(&id).fetch_one(&pool).await.unwrap();
    assert_ne!(фио, "Удаляев У.У.");
    assert_ne!(логин, login);
    assert!(!активна);
    let оценка: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM grades WHERE session_id = $1::uuid)")
        .bind(&session).fetch_one(&pool).await.unwrap();
    assert!(оценка, "история оценок должна остаться");

    // Войти нельзя ни по старому логину, ни по старому токену
    let (status, _) = call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "длинныйПароль" }))).await;
    assert_ne!(status, StatusCode::OK);
    let (status, _) = call(&app, Method::GET, "/api/auth/me", Some(&его), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // В списке учётных записей её нет, включить обратно нельзя
    let (_, все) = call(&app, Method::GET, "/api/admin/users", Some(&admin), None).await;
    assert!(!все.as_array().unwrap().iter().any(|u| u["id"] == id.as_str()));
    let (status, _) = call(&app, Method::POST, &format!("/api/admin/users/{id}/active"),
        Some(&admin), Some(json!({ "active": true }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Второй раз не удалить
    let (status, _) = call(&app, Method::DELETE, &format!("/api/admin/users/{id}"),
        Some(&admin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn удалять_может_только_администратор_и_не_себя() {
    let Some((app, pool)) = app().await else { return };
    let admin = токен(&app, "admin").await;
    let (id, _) = завести(&app, &admin, "student").await;

    let teacher = токен(&app, "teacher1").await;
    let (status, _) = call(&app, Method::DELETE, &format!("/api/admin/users/{id}"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let сам: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'admin'")
        .fetch_one(&pool).await.unwrap();
    let (status, _) = call(&app, Method::DELETE, &format!("/api/admin/users/{сам}"),
        Some(&admin), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    call(&app, Method::DELETE, &format!("/api/admin/users/{id}"), Some(&admin), None).await;
}
