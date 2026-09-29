//! Правка вызова перепиской, когда модель сорвалась.
//!
//! UX-проход 1, находка 10: модель оборвала JSON на середине правки,
//! сервер отдал `500 {"error":"internal"}`, и преподаватель не понял,
//! что делать. Теперь — 503 с понятным текстом, тот же текст в
//! переписке, вызов не меняется.
//!
//! Запуск: `cargo test --test scenario_chat_failure`

use arm112::{api, config::Config, ml::{Llm, MlSuite}, state::AppState};
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

/// Модель, которая отвечает не по форме — как Qwen с оборванным JSON.
struct Сорвавшаяся;

#[async_trait::async_trait]
impl Llm for Сорвавшаяся {
    async fn generate_scenario(&self, _: Value) -> anyhow::Result<Value> { unimplemented!() }
    async fn refine_scenario(&self, _: Value) -> anyhow::Result<Value> { unimplemented!() }
    async fn group_insights(&self, _: Value) -> anyhow::Result<String> { unimplemented!() }
    async fn chat_json(&self, _: Value, _: u32, _: Option<Value>) -> anyhow::Result<Value> {
        anyhow::bail!("в ответе модели нет JSON: {{\"patch\": {{\"applicantName\": \"Пожилая")
    }
}

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let mut ml = MlSuite::build(config.ml_provider).ok()?;
    ml.llm = Box::new(Сорвавшаяся);
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

#[tokio::test]
async fn ответ_модели_не_по_форме_даёт_понятный_отказ_а_не_internal() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;

    let (st, v) = call(&app, Method::POST, "/api/scenarios/manual", Some(&t), Some(json!({
        "title": format!("Сорвавшаяся правка {}", Uuid::new_v4().simple()),
        "legend": "Алло! Горит крыша частного дома, приезжайте!",
        "applicantName": "Петрова Анна Сергеевна",
        "typeIds": ["101"],
        "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"] },
        "address": { "settlement": "Москва", "street": "Станционная", "house": "28" },
        "hidden": [{ "key": "", "question": "Пострадавшие есть?", "answer": "Нет, все вышли." }],
    }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["id"].as_str().unwrap().to_string();

    let (st, v) = call(&app, Method::POST, &format!("/api/scenarios/{id}/chat"), Some(&t),
        Some(json!({ "comment": "Сделай заявителя пожилой женщиной", "apply": true }))).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert_eq!(v["error"], "unavailable");
    let текст = v["detail"].as_str().unwrap_or_default();
    assert!(текст.contains("не по форме") && текст.contains("повторите"), "{v}");

    // Замечание и объяснение — в переписке, вызов прежний
    let (_, chat) = call(&app, Method::GET, &format!("/api/scenarios/{id}/chat"), Some(&t), None).await;
    let реплики = chat.as_array().unwrap();
    assert!(реплики.iter().any(|m| m["content"] == "Сделай заявителя пожилой женщиной"), "{chat}");
    assert!(реплики.iter().any(|m| m["role"] == "system"
        && m["content"].as_str().unwrap_or("").contains("не по форме")), "{chat}");
    let (_, вызов) = call(&app, Method::GET, &format!("/api/scenarios/{id}"), Some(&t), None).await;
    assert_eq!(вызов["applicantName"], "Петрова Анна Сергеевна", "{вызов}");

    sqlx::query("DELETE FROM scenarios WHERE id = $1::uuid").bind(&id).execute(&pool).await.ok();
}
