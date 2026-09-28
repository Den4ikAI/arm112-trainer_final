//! Живой заявитель: реплику формулирует модель, сведения — сценарий.
//!
//! Модель подменена сервером, который запоминает запросы: так видно, что
//! именно ей отдали, — ответов на незаданные вопросы там быть не должно.
//! Настройка `applicant.live_model` общая для базы, поэтому проверки
//! идут по очереди под одним замком.
//!
//! Запуск: `cargo test --test live_applicant`

use arm112::{api, config::{Config, MlProvider}, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

/// Проверки меняют общую настройку — по одной за раз
static ЗАМОК: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Что отвечает подменная модель.
#[derive(Clone)]
struct Поведение {
    реплика: String,
    вердикт: Value,
}

struct Модель {
    запросы: Arc<Mutex<Vec<Value>>>,
    поведение: Arc<Mutex<Поведение>>,
}

async fn подменная_модель() -> (String, Модель) {
    let запросы = Arc::new(Mutex::new(Vec::new()));
    let поведение = Arc::new(Mutex::new(Поведение {
        реплика: "Да Королёв же, Московская область!".into(),
        вердикт: json!({ "keeps": "да", "adds": "нет" }),
    }));
    let (з, п) = (запросы.clone(), поведение.clone());
    let сервер = Router::new().route("/v1/chat/completions", post(move |axum::Json(тело): axum::Json<Value>| {
        let (з, п) = (з.clone(), п.clone());
        async move {
            з.lock().unwrap().push(тело.clone());
            let проверка = тело["response_format"]["json_schema"]["schema"]["properties"]
                .get("keeps").is_some();
            let п = п.lock().unwrap().clone();
            let content = if проверка { п.вердикт.to_string() } else { json!({ "reply": п.реплика }).to_string() };
            axum::Json(json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] }))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, сервер).await.unwrap() });
    (format!("http://{addr}"), Модель { запросы, поведение })
}

async fn app() -> Option<(Router, sqlx::PgPool, Модель)> {
    let config = Config::from_env().ok()?;
    let pool = sqlx::PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let (url, модель) = подменная_модель().await;
    let mut ml = MlSuite::build(MlProvider::Mock).ok()?;
    ml.llm = Box::new(arm112::ml::http::HttpLlm::new(url, "local".into()).ok()?);
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    Some((router, pool, модель))
}

async fn включить(pool: &sqlx::PgPool, да: bool) {
    sqlx::query("UPDATE settings SET value = $1 WHERE key = 'applicant.live_model'")
        .bind(json!(if да { 1 } else { 0 }))
        .execute(pool).await.unwrap();
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

struct Занятие {
    token: String,
    id: String,
    scenario: uuid::Uuid,
}

/// Билет 3, вызов 1: горит крыша частного дома в Королёве.
async fn начать(app: &Router, pool: &sqlx::PgPool) -> Option<Занятие> {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": "student3", "password": "arm112" }))).await;
    let token = b["token"].as_str()?.to_string();
    let scenario: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;
    let (status, body) = call(app, Method::POST, "/api/sessions", Some(&token),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Some(Занятие { token, id: body["session"]["id"].as_str()?.to_string(), scenario })
}

async fn ответ_сценария(pool: &sqlx::PgPool, scenario: uuid::Uuid, key: &str) -> String {
    sqlx::query_scalar("SELECT answer FROM scenario_hidden_facts WHERE scenario_id = $1 AND key = $2")
        .bind(scenario).bind(key).fetch_one(pool).await.unwrap()
}

async fn стенограмма(app: &Router, з: &Занятие) -> Vec<String> {
    let (_, lines) = call(app, Method::GET, &format!("/api/sessions/{}/transcript", з.id),
        Some(&з.token), None).await;
    lines.as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap().to_string()).collect()
}

/// Живая реплика звучит и остаётся в стенограмме, факт выяснен, а в
/// запросах к модели нет ни одного ответа на незаданный вопрос.
#[tokio::test]
async fn живая_реплика_звучит_а_незаданное_модели_не_уходит() {
    let _з = ЗАМОК.lock().await;
    let Some((app, pool, модель)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };
    включить(&pool, true).await;

    let (status, r) = call(&app, Method::POST, &format!("/api/sessions/{}/ask-free", з.id),
        Some(&з.token), Some(json!({ "text": "А город у вас какой?" }))).await;
    включить(&pool, false).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["answer"], "Да Королёв же, Московская область!");
    assert!(!r["factId"].is_null(), "факт должен быть выяснен: {r}");
    assert!(стенограмма(&app, &з).await.contains(&"Да Королёв же, Московская область!".to_string()));

    let запросы = модель.запросы.lock().unwrap().clone();
    assert_eq!(запросы.len(), 2, "реплика и её проверка");
    let всё = serde_json::to_string(&запросы).unwrap();
    assert!(всё.contains(&ответ_сценария(&pool, з.scenario, "city").await));
    for key in ["victims", "gas", "house", "flame"] {
        let a = ответ_сценария(&pool, з.scenario, key).await;
        assert!(!всё.contains(&a), "ответ на незаданный вопрос «{key}» ушёл модели: {a}");
    }
}

/// Проверка не пропустила реплику — звучит заготовленная фраза, и это
/// остаётся в журнале действий.
#[tokio::test]
async fn отбракованная_реплика_заменяется_заготовленной() {
    let _з = ЗАМОК.lock().await;
    let Some((app, pool, модель)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };
    модель.поведение.lock().unwrap().реплика = "Королёв, и сосед обжёгся!".into();
    модель.поведение.lock().unwrap().вердикт = json!({ "keeps": "да", "adds": "да" });
    включить(&pool, true).await;

    let (status, r) = call(&app, Method::POST, &format!("/api/sessions/{}/ask-free", з.id),
        Some(&з.token), Some(json!({ "text": "А город у вас какой?" }))).await;
    включить(&pool, false).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["answer"], ответ_сценария(&pool, з.scenario, "city").await);

    let в_журнале: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'applicant.fallback'
            AND entity_id = $1")
        .bind(&з.id).fetch_one(&pool).await.unwrap();
    assert_eq!(в_журнале, 1);
}

/// Кнопка из списка — тоже живая реплика, если включено.
#[tokio::test]
async fn вопрос_кнопкой_тоже_живой() {
    let _з = ЗАМОК.lock().await;
    let Some((app, pool, модель)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };
    модель.поведение.lock().unwrap().реплика = "Нет-нет, все вышли, никто не пострадал!".into();
    let fact: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM scenario_hidden_facts WHERE scenario_id = $1 AND key = 'victims'")
        .bind(з.scenario).fetch_one(&pool).await.unwrap();
    включить(&pool, true).await;

    let (status, r) = call(&app, Method::POST, &format!("/api/sessions/{}/ask", з.id),
        Some(&з.token), Some(json!({ "factId": fact }))).await;
    включить(&pool, false).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["answer"], "Нет-нет, все вышли, никто не пострадал!");
}

/// Выключено — модель не трогаем вовсе, звучит сценарий.
#[tokio::test]
async fn выключено_модель_не_зовётся() {
    let _з = ЗАМОК.lock().await;
    let Some((app, pool, модель)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };
    включить(&pool, false).await;

    let (status, r) = call(&app, Method::POST, &format!("/api/sessions/{}/ask-free", з.id),
        Some(&з.token), Some(json!({ "text": "А город у вас какой?" }))).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["answer"], ответ_сценария(&pool, з.scenario, "city").await);
    assert!(модель.запросы.lock().unwrap().is_empty());
}
