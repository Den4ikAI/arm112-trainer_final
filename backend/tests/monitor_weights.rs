//! Преподавателю: веса оценки, ход занятия по местам, аналитика.
//!
//! Запуск: `cargo test --test monitor_weights`
//!
//! Проверки веса меняют общие настройки базы, поэтому в конце каждая
//! возвращает веса по умолчанию: от них зависят баллы других проверок.

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

/// Группа с одним курсантом внутри.
async fn группа(pool: &PgPool, курсант: Uuid) -> Uuid {
    let teacher: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(pool).await.unwrap();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id")
        .bind(format!("занятие-{}", Uuid::new_v4().simple()))
        .bind(teacher)
        .fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(id).bind(курсант).execute(pool).await.unwrap();
    id
}

/// Свой оператор 112: без службы — значит оператор 112.
async fn оператор(pool: &PgPool) -> (Uuid, String) {
    let login = format!("оператор-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role)
         SELECT $1, password_hash, 'Проверочный О.О.', 'student'
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).fetch_one(pool).await.unwrap();
    (id, login)
}

async fn убрать_оператора(pool: &PgPool, id: Uuid) {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(pool).await.ok();
    sqlx::query("DELETE FROM cards WHERE created_by = $1").bind(id).execute(pool).await.ok();
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(id).execute(pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(pool).await.ok();
}

async fn прибрать(pool: &PgPool, group_id: Uuid) {
    for sql in [
        "UPDATE sessions SET assignment_id = NULL WHERE assignment_id IN (
             SELECT id FROM assignments WHERE group_id = $1
                OR lesson_id IN (SELECT id FROM lessons WHERE group_id = $1))",
        "DELETE FROM assignments WHERE group_id = $1
             OR lesson_id IN (SELECT id FROM lessons WHERE group_id = $1)",
        "DELETE FROM training_groups WHERE id = $1",
    ] {
        sqlx::query(sql).bind(group_id).execute(pool).await.ok();
    }
}

/// Диспетчер ДДС своей проверочной службы в своей группе.
async fn диспетчер(pool: &PgPool) -> (Uuid, String, Uuid) {
    let метка = Uuid::new_v4().simple().to_string();
    let service = format!("проверка-{}", &метка[..12]);
    let login = format!("диспетчер-{}", &метка[..12]);
    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&service).execute(pool).await.unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Проверочный Д.Д.', 'student', $2
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).bind(&service).fetch_one(pool).await.unwrap();
    let group_id = группа(pool, user_id).await;
    (user_id, service, group_id)
}

async fn убрать_диспетчера(pool: &PgPool, user: Uuid, service: &str, group: Uuid) {
    прибрать(pool, group).await;
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(user).execute(pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(user).execute(pool).await.ok();
    sqlx::query("DELETE FROM services WHERE id = $1").bind(service).execute(pool).await.ok();
}

/// Попытка оператора 112 с готовым разбором: сессия, карточка, оценка.
async fn попытка(pool: &PgPool, user: Uuid, замечания: Value, over: bool) -> (Uuid, Uuid) {
    let session: Uuid = sqlx::query_scalar(
        "INSERT INTO sessions (user_id, mode, norm_sec, finished_at)
         VALUES ($1, 'card112', 60, now()) RETURNING id")
        .bind(user).fetch_one(pool).await.unwrap();
    let номер = format!("В{}", &Uuid::new_v4().simple().to_string()[..10]);
    let card: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, session_id, created_by, saved_at, type_ids)
         VALUES ($1, $2, $3, now(), '{101}') RETURNING id")
        .bind(&номер).bind(session).bind(user).fetch_one(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO grades (session_id, score, elapsed_sec, norm_sec, over_norm, findings, services)
         VALUES ($1, 50, 70, 60, $2, $3, $4)")
        .bind(session).bind(over).bind(замечания)
        .bind(json!({ "expected": [], "actual": [], "missed": [], "extra": [] }))
        .execute(pool).await.unwrap();
    (session, card)
}

fn веса(address: i32) -> Value {
    json!({ "time": 3, "statuses": 3, "comments": 3, "address": address,
            "typeServices": 3, "completeness": 3, "text": 0 })
}

/// Превью весов ничего не сохраняет; сохранение пересчитывает баллы
/// всех попыток и пишет в журнал, кто и как поменял веса.
#[tokio::test]
async fn веса_превью_не_сохраняет_а_сохранение_пересчитывает() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (оп, логин) = оператор(&pool).await;
    let (session, _) = попытка(&pool, оп, json!([
        { "kind": "wrong", "field": "address.house", "label": "Дом/Вл.", "expected": "5", "actual": "7" },
        { "kind": "ok", "field": "answers.where", "label": "Где", "expected": "Дом", "actual": "Дом" }
    ]), false).await;

    // Курсант веса не видит и не меняет
    let (status, _) = call(&app, Method::GET, "/api/grading/weights",
        Some(&токен(&app, &логин).await), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, было) = call(&app, Method::GET, "/api/grading/weights", Some(&teacher), None).await;
    assert_eq!(было["weights"]["address"], 3, "{было}");

    let (status, превью) = call(&app, Method::POST, "/api/grading/weights/preview",
        Some(&teacher), Some(веса(5))).await;
    assert_eq!(status, StatusCode::OK, "{превью}");
    assert!(превью["changed"].as_u64().unwrap() >= 1, "{превью}");
    let (_, после_превью) = call(&app, Method::GET, "/api/grading/weights", Some(&teacher), None).await;
    assert_eq!(после_превью["weights"]["address"], 3, "превью не должно сохранять");

    // Вес вне 0–5 не принимается
    let (status, _) = call(&app, Method::PUT, "/api/grading/weights", Some(&teacher), Some(веса(9))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = call(&app, Method::PUT, "/api/grading/weights", Some(&teacher), Some(веса(5))).await;
    assert_eq!(status, StatusCode::OK);
    let адрес_важнее: i32 = sqlx::query_scalar("SELECT score FROM grades WHERE session_id = $1")
        .bind(session).fetch_one(&pool).await.unwrap();
    call(&app, Method::PUT, "/api/grading/weights", Some(&teacher), Some(веса(0))).await;
    let адрес_не_важен: i32 = sqlx::query_scalar("SELECT score FROM grades WHERE session_id = $1")
        .bind(session).fetch_one(&pool).await.unwrap();
    assert!(адрес_важнее < адрес_не_важен, "{адрес_важнее} {адрес_не_важен}");
    assert_eq!(адрес_не_важен, 100);

    let в_журнале: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'grading.weights' AND at > now() - interval '1 minute'")
        .fetch_one(&pool).await.unwrap();
    assert!(в_журнале >= 2);

    // Вернуть веса по умолчанию: от них зависят другие проверки
    call(&app, Method::PUT, "/api/grading/weights", Some(&teacher), Some(веса(3))).await;
    убрать_оператора(&pool, оп).await;
}

/// Монитор занятия: операторы 112 и диспетчеры ДДС отдельными блоками,
/// у оператора — текущая карточка с таймером и ходом заполнения.
#[tokio::test]
async fn монитор_делит_операторов_112_и_диспетчеров_ддс() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (оп, оп_логин) = оператор(&pool).await;
    let (дисп, служба, группа_id) = диспетчер(&pool).await;
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(группа_id).bind(оп).execute(&pool).await.unwrap();

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": группа_id, "title": "Монитор", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, начато) = call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");

    let сценарий: Uuid = sqlx::query_scalar(
        "SELECT scenario_id FROM assignments WHERE lesson_id = $1::uuid AND user_id = $2 LIMIT 1")
        .bind(&id).bind(оп).fetch_one(&pool).await.unwrap();
    let курсант = токен(&app, &оп_логин).await;
    let (status, начат) = call(&app, Method::POST, "/api/sessions", Some(&курсант),
        Some(json!({ "scenarioId": сценарий, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{начат}");

    // Курсанту монитор не положен
    let (status, _) = call(&app, Method::GET, &format!("/api/lessons/{id}/monitor"), Some(&курсант), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, м) = call(&app, Method::GET, &format!("/api/lessons/{id}/monitor"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{м}");
    let оп112 = м["op112"].as_array().unwrap();
    let ддс = м["dds"].as_array().unwrap();
    assert!(оп112.iter().any(|s| s["userId"] == оп.to_string()), "{м}");
    assert!(ддс.iter().any(|s| s["userId"] == дисп.to_string()), "{м}");
    assert!(оп112.iter().all(|s| s["userId"] != дисп.to_string()));

    let место = оп112.iter().find(|s| s["userId"] == оп.to_string()).unwrap();
    assert_eq!(место["counters"][0], 1, "начат один вызов: {место}");
    let текущая = &место["current"];
    assert!(текущая.is_object(), "у начатого вызова есть текущая карточка: {место}");
    assert!(текущая["normSec"].as_i64().unwrap() > 0);
    assert!(текущая["steps"].as_array().unwrap().iter().any(|s| s["key"] == "address"));
    assert_eq!(м["working"], 1);

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    убрать_диспетчера(&pool, дисп, &служба, группа_id).await;
    убрать_оператора(&pool, оп).await;
}

/// Аналитика по обучающемуся и по группе: попытки, ошибки по группам
/// проверок, «на чём ошибся».
#[tokio::test]
async fn аналитика_по_ученику_и_по_группе() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (оп, _) = оператор(&pool).await;
    let группа_id = группа(&pool, оп).await;
    попытка(&pool, оп, json!([
        { "kind": "missing", "field": "address.street", "label": "Улица", "expected": "Грина", "actual": null }
    ]), true).await;

    let (status, у) = call(&app, Method::GET, &format!("/api/analytics/students/{оп}"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{у}");
    assert_eq!(у["attempts"].as_array().unwrap().len(), 1, "{у}");
    assert_eq!(у["duty"], "op112");
    let адрес = у["rates"].as_array().unwrap().iter().find(|r| r["group"] == "address").unwrap();
    assert_eq!(адрес["errorPct"], 100.0);
    assert!(у["topIssues"].as_array().unwrap().iter()
        .any(|i| i["label"].as_str().unwrap().contains("Улица")), "{у}");

    let (status, г) = call(&app, Method::GET,
        &format!("/api/analytics/groups/{группа_id}?duty=op112"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{г}");
    assert_eq!(г["students"].as_array().unwrap().len(), 1);
    assert_eq!(г["attempts"], 1);
    assert!(г["typical"].as_array().unwrap().iter()
        .any(|i| i["label"] == "Норматив набора карточки превышен"), "{г}");

    прибрать(&pool, группа_id).await;
    убрать_оператора(&pool, оп).await;
}
