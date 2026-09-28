//! Журнал карточек и статусы реагирования ДДС.
//!
//! Запуск: `cargo test --test cards_api`

use arm112::{api, config::Config, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// Куда ходят проверки.
///
/// Отдельная база, если она задана: иначе прогон копит мусор в той же,
/// на которой стоит стенд, и это видно в администрировании и в
/// переписке сценариев.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, sqlx::PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = sqlx::PgPool::connect(&адрес_базы(&config)).await.ok()?;
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
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().unwrap().to_string()
}

/// Оператор 112 заполняет и сохраняет карточку пожара в частном доме.
/// Пожар — профиль МЧС, не ЖКХ: на этом и строятся проверки ленты.
async fn saved_fire_card(app: &Router, pool: &sqlx::PgPool) -> Option<(String, String)> {
    let tok = token(app, "student1").await;
    let scenario: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;

    let (_, s) = call(app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    let session = s["session"]["id"].as_str().unwrap();

    call(app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({
            "description": "горит крыша",
            "address": { "settlement": "Королёв", "street": "Станционная", "house": "28" },
            "typeIds": ["101"],
            "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                         "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                         "inhouse": ["Крыша"], "gasified": ["Нет"] }
        }))).await;

    let (_, saved) = call(app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&tok), None).await;

    Some((saved["card"]["id"].as_str().unwrap().to_string(),
          saved["card"]["number"].as_str().unwrap().to_string()))
}

/// «Чтобы в их ленту попадали только профильные события» — требование
/// ТЗ. Фильтр стоит в SQL, а не в интерфейсе.
#[tokio::test]
async fn в_ленту_ддс_попадают_только_профильные_карточки() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, number)) = saved_fire_card(&app, &pool).await else { return };

    let mchs = token(&app, "student3").await;
    let (status, feed) = call(&app, Method::GET, "/api/cards", Some(&mchs), None).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert!(feed["items"].as_array().unwrap().iter().any(|c| c["number"] == number.as_str()),
        "пожар не дошёл до МЧС");

    let lift = token(&app, "student2").await;
    let (_, feed) = call(&app, Method::GET, "/api/cards", Some(&lift), None).await;
    assert!(!feed["items"].as_array().unwrap().iter().any(|c| c["number"] == number.as_str()),
        "пожар в частном доме попал в ленту Мослифта");

    // Оператору 112 лента ДДС не положена вовсе: у него свои карточки
    let operator = token(&app, "student1").await;
    let (status, body) = call(&app, Method::GET, "/api/cards", Some(&operator), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "wrong_duty");
    let (status, own) = call(&app, Method::GET, "/api/cards?scope=own", Some(&operator), None).await;
    assert_eq!(status, StatusCode::OK, "{own}");
    assert!(own["items"].as_array().unwrap().iter().any(|c| c["number"] == number.as_str()),
        "своя сохранённая карточка должна быть в журнале оператора 112");

    // И прямой запрос по идентификатору тоже не проходит
    let lift = token(&app, "student2").await;
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{card_id}"),
        Some(&lift), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Свою карточку оператор видит всегда, даже если она не профильная
/// для его ДДС: он её и заполнял.
#[tokio::test]
async fn свою_карточку_оператор_видит_и_вне_профиля() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, number)) = saved_fire_card(&app, &pool).await else { return };

    let tok = token(&app, "student1").await;
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{card_id}"),
        Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK);

    let (_, own) = call(&app, Method::GET, "/api/cards?scope=own", Some(&tok), None).await;
    assert!(own["items"].as_array().unwrap().iter().any(|c| c["number"] == number.as_str()));
}

/// Ленту целиком курсанту не отдаём, даже если он попросит scope=all.
#[tokio::test]
async fn курсант_не_может_запросить_всю_ленту() {
    let Some((app, _)) = app().await else { return };

    let student = token(&app, "student3").await;
    let (status, _) = call(&app, Method::GET, "/api/cards?scope=all", Some(&student), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let teacher = token(&app, "teacher1").await;
    let (status, _) = call(&app, Method::GET, "/api/cards?scope=all", Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);
}

/// Порядок статусов задан памяткой ДДС. Проверка живёт на сервере:
/// кнопку в интерфейсе можно спрятать, а запрос всё равно придёт.
#[tokio::test]
async fn статусы_проставляются_только_последовательно() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, _)) = saved_fire_card(&app, &pool).await else { return };
    let tok = token(&app, "student3").await;
    let url = format!("/api/cards/{card_id}/responses");

    // Мимо очереди нельзя
    let (status, body) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Прибытие" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "status_not_allowed");
    // Сервер сразу говорит, что можно — интерфейсу не надо гадать
    assert_eq!(body["allowed"], json!(["Принята", "Не принята"]));

    let (status, _) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Принята", "orderNo": "23" }))).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Прибытие" }))).await;
    assert_eq!(status, StatusCode::CREATED);

    // Назад ходить нельзя
    let (status, _) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Начало реагирования" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// «Не принята» и «Отказ от выполнения работ» без объяснения
/// оставляют карточку без движения.
#[tokio::test]
async fn отказ_без_комментария_не_проходит() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, _)) = saved_fire_card(&app, &pool).await else { return };
    let tok = token(&app, "student3").await;
    let url = format!("/api/cards/{card_id}/responses");

    let (status, body) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Не принята" }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "comment_required");

    // Пробелы за объяснение не считаются
    let (status, _) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Не принята", "comment": "   " }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = call(&app, Method::POST, &url, Some(&tok),
        Some(json!({ "serviceId": "mchs", "status": "Не принята",
                     "comment": "нет свободных расчётов" }))).await;
    assert_eq!(status, StatusCode::CREATED);

    // После «Не принята» остаётся только принять
    let (_, card) = call(&app, Method::GET, &format!("/api/cards/{card_id}"),
        Some(&tok), None).await;
    let mchs = card["services"].as_array().unwrap().iter()
        .find(|s| s["serviceId"] == "mchs").unwrap();
    assert_eq!(mchs["allowedNext"], json!(["Принята"]));
}

/// Диспетчер отвечает за свою службу и ни за чью больше.
#[tokio::test]
async fn диспетчер_не_ставит_статус_за_чужую_службу() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, _)) = saved_fire_card(&app, &pool).await else { return };
    let tok = token(&app, "student3").await;

    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{card_id}/responses"),
        Some(&tok), Some(json!({ "serviceId": "codd", "status": "Принята" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Терминальный статус переводит карточку в «Отработана».
#[tokio::test]
async fn завершение_работ_переводит_карточку_в_отработана() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, _)) = saved_fire_card(&app, &pool).await else { return };
    let tok = token(&app, "student3").await;
    let url = format!("/api/cards/{card_id}/responses");

    for status in ["Принята", "Работы завершены"] {
        let (code, body) = call(&app, Method::POST, &url, Some(&tok),
            Some(json!({ "serviceId": "mchs", "status": status }))).await;
        assert_eq!(code, StatusCode::CREATED, "{body}");
    }

    let (_, card) = call(&app, Method::GET, &format!("/api/cards/{card_id}"),
        Some(&tok), None).await;
    assert_eq!(card["status"], "Отработана");

    let mchs = card["services"].as_array().unwrap().iter()
        .find(|s| s["serviceId"] == "mchs").unwrap();
    assert_eq!(mchs["allowedNext"], json!([]), "после терминального статуса ничего не доступно");
}

/// Служба вне списка оповещения статус проставить не может:
/// она этой карточки не получала.
#[tokio::test]
async fn служба_вне_списка_оповещения_отклоняется() {
    let Some((app, pool)) = app().await else { return };
    let Some((card_id, _)) = saved_fire_card(&app, &pool).await else { return };
    let teacher = token(&app, "teacher1").await;

    let (status, body) = call(&app, Method::POST, &format!("/api/cards/{card_id}/responses"),
        Some(&teacher), Some(json!({ "serviceId": "moslift", "status": "Принята" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

/// Замечание со стенда: в журнале «Постр.: Нет» и пустая колонка
/// «АРМ», хотя в сохранённой карточке «Пострадавшие: 1» и АРМ 4.
/// Журнал показывает то, что сохранено в карточке.
#[tokio::test]
async fn журнал_показывает_пострадавших_и_арм_из_карточки() {
    let Some((app, pool)) = app().await else { return };
    let tok = token(&app, "student1").await;
    let Some(scenario) = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(&pool).await.unwrap() else { return };

    let (_, s) = call(&app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    let session = s["session"]["id"].as_str().unwrap();
    call(&app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({ "victims": 1, "typeIds": ["101"] }))).await;
    let (_, saved) = call(&app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&tok), None).await;
    let number = saved["card"]["number"].as_str().unwrap();

    let teacher = token(&app, "teacher1").await;
    let (_, feed) = call(&app, Method::GET, &format!("/api/cards?scope=all&q={number}"),
        Some(&teacher), None).await;
    let row = feed["items"].as_array().unwrap().iter()
        .find(|c| c["number"] == number).expect("карточки нет в журнале");

    assert_eq!(row["victims"], 1, "{row}");
    assert_eq!(row["workstation"], saved["card"]["workstation"], "{row}");
}
