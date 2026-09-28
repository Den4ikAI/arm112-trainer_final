//! Свободный вопрос заявителю: своими словами, текстом или голосом.
//!
//! Модель здесь не нужна — провайдер нарочно заглушка: сопоставление
//! по словам детерминировано, и проверяется то, что вокруг него —
//! раскрытие факта, стенограмма и разбор «выяснил или угадал».
//!
//! Запуск: `cargo test --test free_question`

use arm112::{api, config::{Config, MlProvider}, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, sqlx::PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = sqlx::PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(MlProvider::Mock).ok()?;
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
    let (s, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    b["token"].as_str().unwrap().to_string()
}

struct Занятие {
    token: String,
    id: String,
    /// ключ факта → идентификатор
    facts: Vec<(String, String)>,
}

impl Занятие {
    fn fact(&self, key: &str) -> &str {
        &self.facts.iter().find(|(k, _)| k == key).unwrap().1
    }
}

/// Билет 3, вызов 1: горит крыша частного дома в Королёве. Заявитель —
/// Иванова Инна Степановна.
async fn начать(app: &Router, pool: &sqlx::PgPool) -> Option<Занятие> {
    let tok = token(app, "student1").await;
    let scenario: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;
    let (status, body) = call(app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let facts = body["scenario"]["questions"].as_array().unwrap().iter()
        .map(|q| (q["key"].as_str().unwrap().to_string(),
                  q["factId"].as_str().unwrap().to_string()))
        .collect();
    Some(Занятие { token: tok, id: body["session"]["id"].as_str().unwrap().to_string(), facts })
}

async fn спросить(app: &Router, з: &Занятие, text: &str) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/sessions/{}/ask-free", з.id), Some(&з.token),
        Some(json!({ "text": text }))).await
}

async fn выяснено(pool: &sqlx::PgPool, з: &Занятие) -> Vec<String> {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT fact_id FROM revealed_facts WHERE session_id = $1::uuid")
        .bind(&з.id).fetch_all(pool).await.unwrap()
        .into_iter().map(|u| u.to_string()).collect()
}

/// Спросили своими словами — заявитель ответил записанным в сценарии,
/// факт выяснен, а в стенограмме дословно то, что сказал курсант.
#[tokio::test]
async fn понятый_вопрос_раскрывает_факт_и_попадает_в_стенограмму() {
    let Some((app, pool)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };

    let (status, r) = спросить(&app, &з, "А город у вас какой?").await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["factId"], з.fact("city"));
    assert!(r["answer"].as_str().unwrap().contains("Королёв"), "{r}");
    assert!(r["unlocks"].as_array().unwrap().iter().any(|u| u == "address.settlement"));

    assert_eq!(выяснено(&pool, &з).await, vec![з.fact("city").to_string()]);

    let (_, lines) = call(&app, Method::GET, &format!("/api/sessions/{}/transcript", з.id),
        Some(&з.token), None).await;
    let texts: Vec<&str> = lines.as_array().unwrap().iter()
        .map(|l| l["text"].as_str().unwrap()).collect();
    assert!(texts.contains(&"А город у вас какой?"),
        "в стенограмме должен быть вопрос курсанта, а не формулировка сценария: {texts:?}");
}

/// Вопрос ни про один факт вызова: заявитель переспрашивает, ничего не
/// выдумывает, и выясненным ничего не становится.
#[tokio::test]
async fn непонятый_вопрос_заявитель_переспрашивает() {
    let Some((app, pool)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };

    let (status, r) = спросить(&app, &з, "Как вас зовут?").await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert!(r["factId"].is_null(), "{r}");
    // Заявитель — Иванова Инна Степановна
    assert!(r["answer"].as_str().unwrap().starts_with("Не поняла"), "{r}");
    assert!(выяснено(&pool, &з).await.is_empty());
}

/// Выясненное своими словами засчитывается так же, как кнопкой: верно
/// заполненная карточка — ни одного «угадал».
#[tokio::test]
async fn выясненное_своими_словами_не_угадано() {
    let Some((app, pool)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };

    for q in ["какой город?", "люди пострадали?", "газ в доме есть?",
              "дом частный или многоэтажка", "пламя есть"] {
        let (status, r) = спросить(&app, &з, q).await;
        assert_eq!(status, StatusCode::OK, "{r}");
        assert!(!r["factId"].is_null(), "вопрос «{q}» не понят: {r}");
    }
    assert_eq!(выяснено(&pool, &з).await.len(), 5);

    call(&app, Method::PATCH, &format!("/api/sessions/{}/card", з.id), Some(&з.token),
        Some(json!({
            "address": { "region": "Московская область", "settlement": "Королёв",
                         "street": "Станционная", "house": "28" },
            "typeIds": ["101"],
            "answers": {
                "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                "inhouse": ["Крыша"], "gasified": ["Нет"]
            }
        }))).await;
    let (status, body) = call(&app, Method::POST, &format!("/api/sessions/{}/save", з.id),
        Some(&з.token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let guessed: Vec<&Value> = body["grade"]["findings"].as_array().unwrap().iter()
        .filter(|f| f["kind"] == "guessed").collect();
    assert!(guessed.is_empty(), "выясненное вопросом засчитано как угаданное: {guessed:?}");
}

#[tokio::test]
async fn пустой_вопрос_не_принимается_а_чужое_занятие_не_видно() {
    let Some((app, pool)) = app().await else { return };
    let Some(з) = начать(&app, &pool).await else { return };

    let (status, _) = спросить(&app, &з, "   ").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let чужой = token(&app, "student2").await;
    let (status, _) = call(&app, Method::POST, &format!("/api/sessions/{}/ask-free", з.id),
        Some(&чужой), Some(json!({ "text": "какой город?" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(выяснено(&pool, &з).await.is_empty());
}
