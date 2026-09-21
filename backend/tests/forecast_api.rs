//! Прогноз результата и проверка самого прогноза.
//!
//! Критерий финальной экспертизы просит «оценку достоверности путем
//! сравнения реальных данных и прогнозных значений». Такое сравнение
//! имеет смысл только если прогноз зафиксирован до события и потом
//! не переписан. Это здесь и проверяется.
//!
//! Запуск: `cargo test --test forecast_api`

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

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&config.database_url).await.ok()?;
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

/// Взять вызов и заполнить карточку, чтобы дошло до разбора.
async fn отработать(app: &Router, token: &str) -> Option<Uuid> {
    let (_, список) = call(app, Method::GET, "/api/scenarios", Some(token), None).await;
    let вызов = список.as_array()?.first()?["id"].as_str()?.to_string();

    let (_, старт) = call(app, Method::POST, "/api/sessions", Some(token),
        Some(json!({ "scenarioId": вызов, "mode": "card112" }))).await;
    let id = старт["session"]["id"].as_str()?.to_string();

    call(app, Method::PATCH, &format!("/api/sessions/{id}/card"), Some(token),
        Some(json!({ "typeIds": ["101"],
                     "answers": { "where": ["Дом"] },
                     "address": { "street": "Станционная", "house": "28" } }))).await;
    call(app, Method::POST, &format!("/api/sessions/{id}/save"), Some(token), None).await;

    Uuid::parse_str(&id).ok()
}

/// Прогноз записывается до вызова, факт — после разбора.
#[tokio::test]
async fn прогноз_фиксируется_до_а_факт_после() {
    let Some((app, pool)) = app().await else { return };
    let студент = токен(&app, "student1").await;

    let Some(session_id) = отработать(&app, &студент).await else { return };

    let строка: Option<(i32, Option<i32>, bool)> = sqlx::query_as(
        "SELECT predicted, actual, made_at < checked_at FROM predictions
          WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_optional(&pool)
    .await
    .unwrap();

    let (прогноз, факт, раньше) = строка.expect("прогноз должен был записаться");
    assert!((0..=100).contains(&прогноз));
    assert!(факт.is_some(), "после разбора факт обязан проставиться");
    assert!(раньше, "прогноз должен быть сделан раньше, чем сверен");
}

/// Повторное сохранение карточки не переписывает прогноз.
///
/// Иначе прогноз можно было бы подогнать под факт, и вся проверка
/// достоверности превратилась бы в самоподтверждение.
#[tokio::test]
async fn прогноз_не_переписывается_задним_числом() {
    let Some((app, pool)) = app().await else { return };
    let студент = токен(&app, "student2").await;

    let Some(session_id) = отработать(&app, &студент).await else { return };

    let было: Option<(i32, Option<i32>)> = sqlx::query_as(
        "SELECT predicted, actual FROM predictions WHERE session_id = $1")
        .bind(session_id).fetch_optional(&pool).await.unwrap();
    let Some(было) = было else { return };

    // Ещё раз меняем карточку и сохраняем
    let id = session_id.to_string();
    call(&app, Method::PATCH, &format!("/api/sessions/{id}/card"), Some(&студент),
        Some(json!({ "answers": { "fire-sign": ["Открытое пламя / Дым"] } }))).await;
    call(&app, Method::POST, &format!("/api/sessions/{id}/save"), Some(&студент), None).await;

    let стало: (i32, Option<i32>) = sqlx::query_as(
        "SELECT predicted, actual FROM predictions WHERE session_id = $1")
        .bind(session_id).fetch_one(&pool).await.unwrap();

    assert_eq!(было, стало, "ни прогноз, ни факт после проверки не меняются");
}

/// Достоверность считается по проверенным прогнозам и сравнивается
/// с базовой линией.
#[tokio::test]
async fn достоверность_сравнивается_с_базовой_линией() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;

    let group_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT g.id FROM training_groups g
           JOIN group_members m ON m.group_id = g.id
          JOIN predictions p ON p.user_id = m.user_id AND p.checked_at IS NOT NULL
          LIMIT 1")
        .fetch_optional(&pool).await.unwrap();
    let Some(group_id) = group_id else { return };

    let (status, ответ) = call(&app, Method::GET,
        &format!("/api/forecast/group/{group_id}"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");

    let точность = &ответ["accuracy"];
    assert!(точность["checked"].as_u64().unwrap() > 0);
    assert!(точность["meanError"].is_number(), "ошибка должна быть посчитана");
    assert!(точность["baselineError"].is_number(),
            "без базовой линии величина ошибки ни о чём не говорит");
    assert!(точность["hitRate"].as_f64().unwrap() <= 1.0);

    // Каждый проверенный прогноз показывается с фактом: «предсказали
    // столько — получил столько»
    for c in ответ["checked"].as_array().unwrap().iter().take(5) {
        assert!(c["predicted"].is_number() && c["actual"].is_number());
        assert!(c["madeAt"].is_string() && c["checkedAt"].is_string());
    }
}

/// Курсант видит свои прогнозы, но не чужие и не по группе.
#[tokio::test]
async fn чужие_прогнозы_курсанту_не_видны() {
    let Some((app, pool)) = app().await else { return };
    let студент = токен(&app, "student1").await;

    let (status, мои) = call(&app, Method::GET, "/api/forecast/me", Some(&студент), None).await;
    assert_eq!(status, StatusCode::OK);

    let свой: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student1'")
        .fetch_one(&pool).await.unwrap();
    for p in мои.as_array().unwrap() {
        let s = Uuid::parse_str(p["sessionId"].as_str().unwrap()).unwrap();
        let хозяин: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE id = $1")
            .bind(s).fetch_one(&pool).await.unwrap();
        assert_eq!(хозяин, свой, "в своей ленте только свои прогнозы");
    }

    let группа: Option<Uuid> = sqlx::query_scalar("SELECT id FROM training_groups LIMIT 1")
        .fetch_optional(&pool).await.unwrap();
    if let Some(g) = группа {
        let (status, _) = call(&app, Method::GET, &format!("/api/forecast/group/{g}"),
            Some(&студент), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

/// Настраиваемые параметры меняет администратор, и только в пределах.
#[tokio::test]
async fn параметры_настраиваются_но_не_как_попало() {
    let Some((app, _)) = app().await else { return };
    let admin = токен(&app, "admin").await;
    let teacher = токен(&app, "teacher1").await;

    let (status, список) = call(&app, Method::GET, "/api/settings", Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!список.as_array().unwrap().is_empty(), "параметры должны быть заведены");

    // Преподаватель смотрит, но не меняет: нормативы общие для всех
    let (status, _) = call(&app, Method::PATCH, "/api/settings/forecast.tolerance",
        Some(&teacher), Some(json!({ "value": 15 }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Допуск в сто баллов сделал бы прогноз «точным» всегда
    let (status, ответ) = call(&app, Method::PATCH, "/api/settings/forecast.tolerance",
        Some(&admin), Some(json!({ "value": 100 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");

    let (status, изменено) = call(&app, Method::PATCH, "/api/settings/forecast.tolerance",
        Some(&admin), Some(json!({ "value": 12 }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(изменено["value"], json!(12.0));

    // Возвращаем как было, чтобы не портить соседние проверки
    call(&app, Method::PATCH, "/api/settings/forecast.tolerance",
        Some(&admin), Some(json!({ "value": 10 }))).await;

    let (status, _) = call(&app, Method::PATCH, "/api/settings/такого.нет",
        Some(&admin), Some(json!({ "value": 1 }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
