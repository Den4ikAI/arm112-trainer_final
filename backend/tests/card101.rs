//! Карта 101 по веткам «Улица» и «Транспорт».
//!
//! Вопросы и варианты сняты с кадров эксперта (refs/КАРТОЧКА 112,
//! image14–image39). Ожидания — дословные типы происшествия из ЕКП
//! и службы, которые кадры показывают в списке оповещения.
//!
//! Требуют базы с импортированным классификатором, без неё тихо
//! пропускаются. Запуск: `cargo test --test card101`

use arm112::{api, config::Config, domain::classify::classify, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use tower::ServiceExt;
use uuid::Uuid;

async fn setup() -> Option<(Config, PgPool, Uuid)> {
    let config = Config::from_env().ok()?;
    let url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone());
    let pool = PgPool::connect(&url).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let version: Uuid =
        sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
            .fetch_optional(&pool)
            .await
            .ok()??;
    Some((config, pool, version))
}

fn ответы(pairs: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
    pairs.iter().map(|(k, v)| (k.to_string(), vec![v.to_string()])).collect()
}

/// Кадр 19: улица, открытое пламя, нет доступа, мусор.
const МУСОР: &[(&str, &str)] = &[
    ("where", "Улица"),
    ("street-sign", "Открытое пламя / Дым"),
    ("access", "Нет доступа"),
    ("street-object", "Мусор"),
];

/// Кадр 36: транспорт, открытое пламя, нет доступа, общественный транспорт.
const АВТОБУС: &[(&str, &str)] = &[
    ("where", "Транспорт"),
    ("transport-sign", "Открытое пламя / Дым"),
    ("access", "Нет доступа"),
    ("transport-object", "Общественный транспорт"),
];

async fn оценить(pool: &PgPool, v: Uuid, pairs: &[(&str, &str)]) -> arm112::domain::classify::ClassifyResult {
    classify(pool, v, &["101".into()], &ответы(pairs)).await.unwrap()
}

fn есть(r: &arm112::domain::classify::ClassifyResult, id: &str) -> bool {
    r.services.iter().any(|s| s.service_id == id)
}

#[tokio::test]
async fn пожар_мусора_на_улице_подбирает_мчс_и_цодд() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, МУСОР).await;

    assert!(r.reason.is_none(), "классификатор не должен жаловаться: {:?}", r.reason);
    assert_eq!(r.outcome.as_deref(), Some("пожар: мусор"));
    // Кадр 19: «Служба 101» и ЦОДД в списке оповещения
    assert!(есть(&r, "mchs"), "МЧС обязан быть в списке оповещения");
    assert!(есть(&r, "codd"), "ЦОДД на кадре есть");
    assert!(!есть(&r, "mvd"), "правонарушение ещё не выбрано");
}

/// Кадр 24: выбран признак «Правонарушение» — появляется «Служба 102».
#[tokio::test]
async fn правонарушение_на_улице_вызывает_мвд() {
    let Some((_, pool, v)) = setup().await else { return };

    let mut pairs = МУСОР.to_vec();
    pairs.push(("offense", "Есть правонарушение"));
    let r = оценить(&pool, v, &pairs).await;

    assert!(есть(&r, "mvd"));
    assert_eq!(r.outcome.as_deref(), Some("пожар: мусор"),
               "правонарушение службы добавляет, а тип не меняет");
}

/// Кадр 28: «Проведена ли газификация — Да» — появляется «Служба 104».
#[tokio::test]
async fn газификация_на_улице_вызывает_мосгаз() {
    let Some((_, pool, v)) = setup().await else { return };

    let без = оценить(&pool, v, МУСОР).await;
    let mut pairs = МУСОР.to_vec();
    pairs.push(("gasified", "Да"));
    let с = оценить(&pool, v, &pairs).await;

    assert!(!есть(&без, "mosgaz"));
    assert!(есть(&с, "mosgaz"));
}

/// Кадр 29: запах гари на улице — одна «Служба 101».
#[tokio::test]
async fn запах_гари_на_улице() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, &[("where", "Улица"), ("street-sign", "Запах гари")]).await;

    assert_eq!(r.outcome.as_deref(), Some("запах гари на улице"));
    assert!(есть(&r, "mchs"));
}

#[tokio::test]
async fn трава_и_пух_это_пожар_травы() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, &[
        ("where", "Улица"),
        ("street-sign", "Открытое пламя / Дым"),
        ("street-object", "Трава, пух"),
    ]).await;

    assert_eq!(r.outcome.as_deref(), Some("пожар: трава"));
}

/// Кадр 36: в списке «Служба 101», «Служба 102» и ЦОДД.
#[tokio::test]
async fn пожар_общественного_транспорта() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, АВТОБУС).await;

    assert_eq!(r.outcome.as_deref(), Some("пожар: автобус"));
    assert!(есть(&r, "mchs"));
    assert!(есть(&r, "mvd"));
    assert!(есть(&r, "codd"));
}

/// У «метро» второго признака в классификаторе нет вовсе. Раньше движок
/// всё равно просил уточнить второй уровень — между одним и ничем.
#[tokio::test]
async fn пожар_в_метро_не_просит_второй_уровень() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, &[
        ("where", "Транспорт"),
        ("transport-sign", "Открытое пламя / Дым"),
        ("transport-object", "Метро"),
    ]).await;

    assert!(r.reason.is_none(), "классификатор не должен жаловаться: {:?}", r.reason);
    assert_eq!(r.outcome.as_deref(), Some("пожар: метро"));
    assert!(есть(&r, "mchs"));
}

/// Одно «Улица» — вызов не классифицирован, но причина понятна.
#[tokio::test]
async fn одна_улица_просит_признак_второго_уровня() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, &[("where", "Улица")]).await;

    assert!(r.outcome.is_none());
    assert!(r.services.is_empty());
    let причина = r.reason.unwrap_or_default();
    assert!(причина.contains("второго уровня"), "причина: {причина}");
}

/// Ветка «Дом» после перестройки карты работает как раньше.
#[tokio::test]
async fn ветка_дома_не_задета() {
    let Some((_, pool, v)) = setup().await else { return };

    let r = оценить(&pool, v, &[
        ("where", "Дом"),
        ("fire-sign", "Открытое пламя / Дым"),
        ("inhouse", "Квартира"),
    ]).await;

    assert_eq!(r.outcome.as_deref(), Some("пожар: квартира"));
    assert!(есть(&r, "mchs"));
}

/// Карта растёт по ответам: условия показа приходят вместе с вопросами.
#[tokio::test]
async fn карта_отдаёт_условия_показа_вопросов() {
    let Some((config, pool, _)) = setup().await else { return };
    let ml = MlSuite::build(config.ml_provider).unwrap();
    let app: Router = api::routes().with_state(AppState::new(pool, config, ml));

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "login": "student1", "password": "arm112" }).to_string()))
        .unwrap();
    let bytes = app.clone().oneshot(req).await.unwrap()
        .into_body().collect().await.unwrap().to_bytes();
    let токен = serde_json::from_slice::<Value>(&bytes).unwrap()["token"]
        .as_str().unwrap().to_string();

    let req = Request::builder()
        .uri("/api/catalog/survey-cards/101")
        .header(header::AUTHORIZATION, format!("Bearer {токен}"))
        .body(Body::empty())
        .unwrap();
    let bytes = app.oneshot(req).await.unwrap()
        .into_body().collect().await.unwrap().to_bytes();
    let карта: Value = serde_json::from_slice(&bytes).unwrap();
    let вопрос = |id: &str| -> Value {
        карта["questions"].as_array().unwrap().iter()
            .find(|q| q["id"] == id).cloned()
            .unwrap_or_else(|| panic!("нет вопроса {id}"))
    };

    assert_eq!(карта["source"], "screenshot");
    assert_eq!(вопрос("where")["showWhen"], json!([]), "«Где» виден всегда");
    assert_eq!(вопрос("street-sign")["label"], "Признак пожара (улица)");
    assert_eq!(вопрос("street-sign")["showWhen"],
               json!([{ "question": "where", "values": ["Улица"] }]));
    assert_eq!(вопрос("street-object")["label"], "Улица (пламя, дым)");
    assert_eq!(вопрос("street-object")["showWhen"],
               json!([{ "question": "street-sign", "values": ["Открытое пламя / Дым"] }]));
    assert_eq!(вопрос("transport-object")["label"], "Транспорт (пламя, дым)");
    assert_eq!(вопрос("offense-note")["showWhen"],
               json!([{ "question": "offense", "values": ["Есть правонарушение"] }]));
    assert_eq!(вопрос("offense-note")["kind"], "text");
}
