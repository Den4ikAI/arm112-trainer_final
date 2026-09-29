//! Учебный материал, который собирает преподаватель: вызовы 112 и
//! происшествия ДДС — вручную и по описанию для модели.
//!
//! Замечания команды, п. 6: «преподаватель может создать карточки как
//! через промт для нейросети (после править и утверждать результат),
//! или вручную собирать карточку. Все карточки должны быть доступны при
//! составлении занятия». П. 10: карточки операторов 112 к диспетчерам
//! не попадают — учебное происшествие помечено `origin = 'teacher'`.
//!
//! Запуск: `cargo test --test teacher_material`

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

/// Признаки пожара в частном доме: по ним классификатор строит эталон
fn пожар_в_доме() -> Value {
    json!({ "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
            "house-kind": ["Дом частный"], "people-threat": ["Нет"],
            "inhouse": ["Крыша"], "gasified": ["Нет"] })
}

#[tokio::test]
async fn вызов_вручную_собирается_черновиком_с_эталоном_и_утверждается() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    let тема = format!("Пожар в частном доме {}", Uuid::new_v4().simple());

    let (st, v) = call(&app, Method::POST, "/api/scenarios/manual", Some(&t), Some(json!({
        "title": тема,
        "legend": "Алло! Горит крыша частного дома, приезжайте!",
        "applicantName": "Петрова Анна Сергеевна",
        "typeIds": ["101"],
        "answers": пожар_в_доме(),
        "address": { "settlement": "Москва", "street": "Станционная", "house": "28" },
        "hidden": [
            { "key": "", "question": "Пострадавшие есть?", "answer": "Нет, все вышли." },
            { "key": "gas", "question": "Дом газифицирован?", "answer": "Нет." },
        ],
    }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["status"], "draft");
    assert_eq!(v["source"], "manual");
    assert_eq!(v["title"], тема.as_str());
    assert_eq!(v["hidden"].as_array().unwrap().len(), 2);
    // Пустой ключ факта получает свой, а не теряется
    assert_eq!(v["hidden"][0]["key"], "f1");
    // Эталон посчитал классификатор, не преподаватель и не модель
    assert!(v["classifyReason"].is_null(), "{v}");
    assert!(v["expected"]["outcome"].is_string());
    assert!(v["expected"]["services"].as_array().unwrap().iter().any(|s| s == "mchs"));
    assert_eq!(v["expected"]["address"]["street"], "Станционная");

    let id = v["id"].as_str().unwrap().to_string();

    // В списке преподавателя — с темой и статусом
    let (_, list) = call(&app, Method::GET, "/api/teacher/scenarios", Some(&t), None).await;
    let строка = list.as_array().unwrap().iter()
        .find(|x| x["id"] == id.as_str()).expect("нет в списке").clone();
    assert_eq!(строка["title"], тема.as_str());
    assert_eq!(строка["status"], "draft");
    assert_eq!(строка["typeIds"], json!(["101"]));

    // Правка темы и типа вручную
    let (st, v) = call(&app, Method::PATCH, &format!("/api/scenarios/{id}"), Some(&t),
        Some(json!({ "title": format!("{тема} (правка)") }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["title"].as_str().unwrap().ends_with("(правка)"));

    let (st, v) = call(&app, Method::POST, &format!("/api/scenarios/{id}/approve"), Some(&t), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "approved");

    sqlx::query("DELETE FROM scenarios WHERE id = $1::uuid").bind(&id).execute(&pool).await.ok();
}

#[tokio::test]
async fn вызов_по_описанию_без_модели_остаётся_черновиком_с_описанием() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    let описание = format!("Пожилая женщина сообщает о запахе газа в подъезде {}", Uuid::new_v4().simple());

    let (st, v) = call(&app, Method::POST, "/api/scenarios/from-prompt", Some(&t),
        Some(json!({ "title": "Запах газа", "prompt": описание }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["status"], "draft");
    assert_eq!(v["source"], "prompt");
    assert_eq!(v["title"], "Запах газа");
    // На заглушке модели — черновик, а не отказ; причина в переписке
    let id = v["id"].as_str().unwrap().to_string();
    let (_, chat) = call(&app, Method::GET, &format!("/api/scenarios/{id}/chat"), Some(&t), None).await;
    let реплики = chat.as_array().unwrap();
    assert!(реплики.iter().any(|m| m["content"].as_str().unwrap_or("").contains(&описание)),
            "описание преподавателя должно остаться в переписке: {chat}");
    assert!(реплики.len() >= 2);

    let (st, _) = call(&app, Method::POST, "/api/scenarios/from-prompt", Some(&t),
        Some(json!({ "prompt": "   " }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    sqlx::query("DELETE FROM scenarios WHERE id = $1::uuid").bind(&id).execute(&pool).await.ok();
}

#[tokio::test]
async fn происшествие_ддс_вручную_службы_от_классификатора_и_утверждение() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    let тема = format!("Пожар: частный дом {}", Uuid::new_v4().simple());

    let (st, v) = call(&app, Method::POST, "/api/dds-incidents", Some(&t), Some(json!({
        "title": тема,
        "applicantName": "Сидоров Иван Петрович",
        "applicantStatus": "очевидец",
        "address": { "settlement": "Москва", "street": "Грина", "house": "11" },
        "description": "Горит крыша частного дома, открытое пламя, пострадавших нет.",
        "typeIds": ["101"],
        "answers": пожар_в_доме(),
        "extraServices": ["moslift"],
    }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["status"], "draft");
    assert_eq!(v["title"], тема.as_str());
    let services = v["services"].as_array().unwrap();
    assert!(services.iter().any(|s| s == "mchs"), "службы подбирает классификатор: {v}");
    assert_eq!(v["manualServices"], json!(["moslift"]));
    assert!(v["outcomeType"].is_string());
    let id = v["id"].as_str().unwrap().to_string();

    // Карточка помечена как материал преподавателя: к диспетчеру она
    // попадёт только через занятие
    let origin: String = sqlx::query_scalar("SELECT origin FROM cards WHERE id = $1::uuid")
        .bind(&id).fetch_one(&pool).await.unwrap();
    assert_eq!(origin, "teacher");
    // Пока черновик — не «сохранена» и в ленты не идёт
    let saved: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT saved_at FROM cards WHERE id = $1::uuid")
            .bind(&id).fetch_one(&pool).await.unwrap();
    assert!(saved.is_none());

    // Правка возвращает в черновик и пересчитывает службы
    let (st, v) = call(&app, Method::PUT, &format!("/api/dds-incidents/{id}"), Some(&t), Some(json!({
        "title": тема,
        "address": { "settlement": "Москва", "street": "Грина", "house": "11" },
        "description": "Горит крыша частного дома.",
        "typeIds": ["101"],
        "answers": пожар_в_доме(),
    }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["manualServices"], json!([]));

    let (st, v) = call(&app, Method::POST, &format!("/api/dds-incidents/{id}/approve"), Some(&t), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "approved");
    let saved: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT saved_at FROM cards WHERE id = $1::uuid")
            .bind(&id).fetch_one(&pool).await.unwrap();
    assert!(saved.is_some(), "утверждённое можно выдавать диспетчеру");

    let (_, list) = call(&app, Method::GET, "/api/dds-incidents", Some(&t), None).await;
    assert!(list.as_array().unwrap().iter().any(|x| x["id"] == id.as_str() && x["status"] == "approved"));

    let (st, _) = call(&app, Method::DELETE, &format!("/api/dds-incidents/{id}"), Some(&t), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, Method::GET, &format!("/api/dds-incidents/{id}"), Some(&t), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn неполное_происшествие_не_утверждается() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    let (st, v) = call(&app, Method::POST, "/api/dds-incidents", Some(&t),
        Some(json!({ "title": "Пустое", "description": "" }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["id"].as_str().unwrap().to_string();

    let (st, v) = call(&app, Method::POST, &format!("/api/dds-incidents/{id}/approve"), Some(&t), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let причина = v["detail"].as_str().unwrap_or("");
    assert!(причина.contains("описание") && причина.contains("тип") && причина.contains("адрес"),
            "отказ называет, чего не хватает: {v}");

    sqlx::query("DELETE FROM cards WHERE id = $1::uuid").bind(&id).execute(&pool).await.ok();
}

#[tokio::test]
async fn происшествие_по_описанию_на_заглушке_черновик_с_описанием() {
    let Some((app, pool)) = app().await else { return };
    let t = токен(&app, "teacher1").await;
    let описание = format!("Задымление мусоропровода в подъезде {}", Uuid::new_v4().simple());
    let (st, v) = call(&app, Method::POST, "/api/dds-incidents/from-prompt", Some(&t),
        Some(json!({ "title": "Мусоропровод", "prompt": описание }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["status"], "draft");
    assert_eq!(v["title"], "Мусоропровод");
    assert!(!v["description"].as_str().unwrap().is_empty());
    let id = v["id"].as_str().unwrap().to_string();
    sqlx::query("DELETE FROM cards WHERE id = $1::uuid").bind(&id).execute(&pool).await.ok();
}

#[tokio::test]
async fn курсант_учебный_материал_не_создаёт_и_не_видит() {
    let Some((app, _)) = app().await else { return };
    let s = токен(&app, "student1").await;
    let (st, _) = call(&app, Method::GET, "/api/dds-incidents", Some(&s), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&app, Method::POST, "/api/dds-incidents", Some(&s),
        Some(json!({ "title": "x" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&app, Method::POST, "/api/scenarios/manual", Some(&s),
        Some(json!({ "title": "x", "legend": "y" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&app, Method::GET, "/api/teacher/scenarios", Some(&s), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}
