//! Разделы курсанта и выдача только преподавателем (Денис, 29.09).
//!
//! Запуск: `cargo test --test student_work`

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
    Some((api::routes().with_state(AppState::new(pool.clone(), config, ml)), pool))
}

async fn call(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Value)
{
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
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

async fn токен(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().expect("вход не удался").to_string()
}

/// Свой курсант в своей группе: оператор 112 (служба пустая) или
/// диспетчер ДДС (своя служба).
async fn курсант(pool: &PgPool, служба: Option<&str>) -> (Uuid, String, Uuid) {
    let метка = Uuid::new_v4().simple().to_string();
    let login = format!("раздел-{}", &метка[..12]);
    if let Some(s) = служба {
        sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')
                     ON CONFLICT DO NOTHING")
            .bind(s).execute(pool).await.unwrap();
    }
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id, duty)
         SELECT $1, password_hash, 'Разделов Р.Р.', 'student', $2,
                CASE WHEN $2::text IS NULL THEN 'op112' ELSE 'dds' END::user_duty
           FROM users WHERE login = 'student1' RETURNING id")
        .bind(&login).bind(служба).fetch_one(pool).await.unwrap();
    let group: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id)
         SELECT $1, id FROM users WHERE login = 'teacher1' RETURNING id")
        .bind(format!("разделы-{}", &метка[..12])).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(group).bind(user).execute(pool).await.unwrap();
    (user, login, group)
}

async fn раздел(app: &Router, т: &str, section: &str) -> Vec<Value> {
    let (status, body) = call(app, Method::GET, &format!("/api/me/work?section={section}"),
        Some(т), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

/// Учебные вызовы оператора 112: выданное преподавателем со статусом и
/// баллом; закончённое повторно не проходится — сервер отвечает 409.
#[tokio::test]
async fn учебный_вызов_проходится_один_раз_и_виден_со_статусом() {
    let Some((app, pool)) = app().await else { return };
    let Some(вызов): Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1 AND status = 'approved'")
        .fetch_optional(&pool).await.unwrap() else { return };
    let teacher = токен(&app, "teacher1").await;
    let (_, login, group) = курсант(&pool, None).await;
    let т = токен(&app, &login).await;

    // До выдачи — пусто: вызовы раздаёт преподаватель
    assert!(раздел(&app, &т, "training").await.is_empty());

    let (status, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group, "title": "Раздел", "mode": "card112",
                     "scenarioIds": [вызов] }))).await;
    assert_eq!(status, StatusCode::CREATED, "{занятие}");
    let id = занятие["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;

    let строки = раздел(&app, &т, "training").await;
    assert_eq!(строки.len(), 1, "{строки:?}");
    assert_eq!(строки[0]["status"], "not_started");
    assert!(строки[0]["issuedAt"].is_string());
    assert!(!строки[0]["topic"].as_str().unwrap().is_empty());
    // Экзамен и тренировка по частям — отдельные разделы
    assert!(раздел(&app, &т, "exam").await.is_empty());
    assert!(раздел(&app, &т, "blitz").await.is_empty());

    let (status, начато) = call(&app, Method::POST, "/api/sessions", Some(&т),
        Some(json!({ "scenarioId": вызов, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{начато}");
    let session = начато["session"]["id"].as_str().unwrap().to_string();
    assert_eq!(раздел(&app, &т, "training").await[0]["status"], "in_progress");

    let (status, сохранено) = call(&app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&т), None).await;
    assert_eq!(status, StatusCode::OK, "{сохранено}");
    let строка = раздел(&app, &т, "training").await.remove(0);
    assert_eq!(строка["status"], "done");
    assert!(строка["score"].is_number(), "{строка}");
    assert_eq!(строка["sessionId"], session.as_str());

    // Повторно — нельзя, только разбор
    let (status, body) = call(&app, Method::POST, "/api/sessions", Some(&т),
        Some(json!({ "scenarioId": вызов, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
}

/// Диспетчеру ДДС выдаются только учебные происшествия преподавателя,
/// поимённо и по порядку; карточку курсанта 112 выдать нельзя.
#[tokio::test]
async fn происшествия_ддс_выдаются_поимённо_и_только_преподавательские() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let служба = format!("раздел-сл-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let (_, login, group) = курсант(&pool, Some(&служба)).await;
    let т = токен(&app, &login).await;

    let карточка = |origin: &'static str| {
        let pool = pool.clone();
        let служба = служба.clone();
        async move {
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO cards (number, saved_at, type_ids, description, origin)
                 VALUES ($1, now(), '{101}', 'учебное происшествие', $2) RETURNING id")
                .bind(format!("Р{}", &Uuid::new_v4().simple().to_string()[..10]))
                .bind(origin).fetch_one(&pool).await.unwrap();
            sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
                .bind(id).bind(&служба).execute(&pool).await.unwrap();
            id
        }
    };
    let первое = карточка("teacher").await;
    let второе = карточка("teacher").await;
    let курсантская = карточка("session").await;

    // Карточку курсанта 112 выдать нельзя
    let (status, _) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group, "title": "Нельзя", "mode": "dds",
                     "cardIds": [курсантская] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group, "title": "Нельзя", "mode": "dds",
                     "source": "by_students" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Экзамен ДДС — поимённо, по порядку: второе придёт через интервал
    let (status, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group, "title": "Экзамен ДДС", "mode": "dds",
                     "kind": "marathon", "cardIds": [первое, второе],
                     "cardIntervalSec": 600 }))).await;
    assert_eq!(status, StatusCode::CREATED, "{занятие}");
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, начато) = call(&app, Method::POST, &format!("/api/lessons/{id}/start"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");

    let экзамен = раздел(&app, &т, "exam").await;
    assert_eq!(экзамен.len(), 1, "пока пришло только первое: {экзамен:?}");
    assert_eq!(экзамен[0]["cardId"], первое.to_string());
    assert_eq!(экзамен[0]["status"], "not_started");
    assert!(раздел(&app, &т, "training").await.is_empty());

    // Курсантская — не видна ни в ленте, ни по ссылке
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{курсантская}"),
        Some(&т), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Взялся — «выполняется»; балл экзамена скрыт до конца
    call(&app, Method::POST, &format!("/api/cards/{первое}/responses"), Some(&т),
        Some(json!({ "serviceId": служба, "status": "Принята", "comment": "Принято" }))).await;
    let строка = раздел(&app, &т, "exam").await.remove(0);
    assert_eq!(строка["status"], "in_progress");
    assert_eq!(строка["hidden"], true);
    assert!(строка["score"].is_null());

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
}
