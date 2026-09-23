//! Опросные карты, выведенные из классификатора.
//!
//! Со скриншотов боевой системы сняты только три карты. Для остальных
//! шестнадцати типов карты не было: на ДТП, «Человек в опасности»,
//! 102 и 103 курсант нажимал плашку, и не появлялось ничего — вызов
//! было нечем классифицировать.
//!
//! Запуск: `cargo test --test survey_cards`

use arm112::{
    api,
    config::Config,
    domain::classify::classify,
    ingest::survey::{derived_question_id, materialise},
    ml::MlSuite,
    state::AppState,
};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use tower::ServiceExt;
use uuid::Uuid;

/// Куда ходят проверки: отдельная база, если она задана.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, PgPool, Uuid)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let version: Uuid =
        sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
            .fetch_optional(&pool)
            .await
            .ok()??;
    // Сервер строит карты при запуске; в проверке запуска нет.
    // Ошибка здесь — не «нет базы», а поломка: молча пропускать нельзя
    materialise(&pool, version).await.expect("карты не собрались");
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    Some((router, pool, version))
}

async fn get(app: &Router, path: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn токен(app: &Router) -> String {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "login": "student1", "password": "arm112" }).to_string()))
        .unwrap();
    let bytes = app.clone().oneshot(req).await.unwrap()
        .into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&bytes).unwrap()["token"].as_str().unwrap().to_string()
}

fn ответы(pairs: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
    pairs.iter().map(|(k, v)| (k.to_string(), vec![v.to_string()])).collect()
}

/// У каждого типа, привязанного к группе классификатора, есть карта.
#[tokio::test]
async fn карта_есть_у_каждого_типа() {
    let Some((app, pool, _)) = app().await else { return };
    let токен = токен(&app).await;

    let типы: Vec<String> =
        sqlx::query_scalar("SELECT id FROM incident_types WHERE group_no IS NOT NULL")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(типы.len() >= 19);

    let mut без_карты = Vec::new();
    for t in &типы {
        let (status, _) = get(&app, &format!("/api/catalog/survey-cards/{t}"), &токен).await;
        if status != StatusCode::OK {
            без_карты.push(t.clone());
        }
    }
    assert!(без_карты.is_empty(), "нет опросной карты у типов: {без_карты:?}");
}

/// Карта ДТП — дерево признаков, как в классификаторе.
#[tokio::test]
async fn карта_дтп_устроена_деревом() {
    let Some((app, _, _)) = app().await else { return };
    let токен = токен(&app).await;

    let (status, карта) = get(&app, "/api/catalog/survey-cards/dtp", &токен).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(карта["source"], "classifier");

    let вопросы = карта["questions"].as_array().unwrap();
    assert_eq!(вопросы.len(), 3, "три уровня признаков");

    let p1 = &вопросы[0];
    assert_eq!(p1["label"], "Признак 1 (тип происшествия)");
    assert!(p1["dependsOn"].is_null());
    let варианты: Vec<&str> = p1["options"].as_array().unwrap()
        .iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(варианты, vec!["ДТП", "ДТП пострадавшие"],
               "формулировки — дословно из классификатора");

    // Второй признак зависит от первого
    let p2 = &вопросы[1];
    assert_eq!(p2["dependsOn"], derived_question_id("dtp", 1));
    let родители = &p2["optionParents"];
    assert_eq!(родители["Автоподстава"], json!(["ДТП"]),
               "автоподстава бывает только в ветке без пострадавших");
    assert_eq!(родители["Наезд на пешехода"], json!(["ДТП пострадавшие"]));

    // Третий — от второго
    let p3 = &вопросы[2];
    assert_eq!(p3["dependsOn"], derived_question_id("dtp", 2));
    assert_eq!(p3["optionParents"]["Опора моста"], json!(["Наезд на препятствие"]));
}

/// Карты со скриншотов выведенными не затираются.
#[tokio::test]
async fn снятые_карты_остаются_как_были() {
    let Some((app, _, _)) = app().await else { return };
    let токен = токен(&app).await;

    let (_, карта) = get(&app, "/api/catalog/survey-cards/101", &токен).await;
    assert_eq!(карта["source"], "screenshot");
    assert_eq!(карта["questions"][0]["label"], "Где",
               "формулировки боевой системы не подменяются общими");
}

/// По выведенной карте вызов классифицируется, и службы подбираются.
#[tokio::test]
async fn дтп_классифицируется_по_выведенной_карте() {
    let Some((_, pool, v)) = app().await else { return };

    let r = classify(&pool, v, &["dtp".into()], &ответы(&[
        (&derived_question_id("dtp", 1), "ДТП пострадавшие"),
        (&derived_question_id("dtp", 2), "Наезд на пешехода"),
    ]))
    .await
    .unwrap();

    assert!(r.reason.is_none(), "классификатор не должен жаловаться: {:?}", r.reason);
    assert!(r.outcome.is_some(), "итоговый тип должен определиться");
    assert!(!r.services.is_empty(), "службы должны подобраться");
}

/// Один первый признак — это «ДТП» без уточнений, а не вопрос.
///
/// В классификаторе есть позиция без второго признака. Раньше её было
/// не выбрать: незаполненный второй уровень давал все позиции ветки,
/// и классификатор просил уточнить то, чего заявитель не говорил.
#[tokio::test]
async fn первый_признак_без_уточнений_даёт_общую_позицию() {
    let Some((_, pool, v)) = app().await else { return };

    let r = classify(&pool, v, &["dtp".into()], &ответы(&[
        (&derived_question_id("dtp", 1), "ДТП"),
    ]))
    .await
    .unwrap();

    assert!(r.reason.is_none(), "общая позиция есть, уточнять нечего: {:?}", r.reason);
    assert_eq!(r.positions.len(), 1, "ровно одна — та, что без второго признака");
}

/// Ответы двух выведенных карт не смешиваются.
///
/// Ответы карточки — один словарь на все выбранные типы. Общий ключ
/// `p1` у ДТП и «Человека в опасности» смешал бы их признаки.
#[tokio::test]
async fn ответы_разных_карт_не_смешиваются() {
    let Some((_, pool, v)) = app().await else { return };

    let r = classify(&pool, v, &["dtp".into(), "person-danger".into()], &ответы(&[
        (&derived_question_id("dtp", 1), "ДТП"),
        (&derived_question_id("person-danger", 1), "Падение"),
        (&derived_question_id("person-danger", 2), "Падение человека с высоты (из окна с балкона на стройке в котлован на производстве )"),
    ]))
    .await
    .unwrap();

    assert!(r.reason.is_none(), "{:?}", r.reason);
    assert!(r.positions.len() >= 2, "по позиции на каждый тип: {:?}", r.positions);
}

/// «Без признака» предпочитается, только когда уровень выше определён.
///
/// В ветке «на улице» у пожара нет позиции без второго признака, но
/// есть «запах гари» без третьего. Правило «незаполненный уровень —
/// позиция без признака» на третьем уровне превращало «на улице» без
/// уточнений в «запах гари на улице», хотя второй уровень курсант
/// не выбирал вовсе.
#[tokio::test]
async fn улица_без_уточнений_не_превращается_в_запах_гари() {
    let Some((_, pool, v)) = app().await else { return };

    let r = classify(&pool, v, &["101".into()], &ответы(&[("where", "Улица")]))
        .await
        .unwrap();

    assert_ne!(r.outcome.as_deref(), Some("запах гари на улице"),
               "второй уровень не выбран — угадывать нельзя");
    assert!(r.reason.as_deref().is_some_and(|p| p.contains("второго уровня")),
            "классификатор должен попросить уточнить: {:?}", r.reason);
}
