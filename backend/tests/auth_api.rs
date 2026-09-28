//! Вход, токен и разграничение доступа.
//!
//! Запуск: `cargo test --test auth_api`
//! Без базы тесты молча пропускаются — как и остальные в этом каталоге.

use arm112::{api, config::Config, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Request, StatusCode},
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

async fn app() -> Option<Router> {
    let config = Config::from_env().ok()?;
    let pool = sqlx::PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    Some(api::routes().with_state(AppState::new(pool, config, ml)))
}

async fn post(app: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    read(app.clone().oneshot(req).await.unwrap()).await
}

async fn get_with(app: &Router, path: &str, token: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::get(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    read(app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap()).await
}

async fn read(resp: axum::response::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn login(app: &Router, user: &str) -> String {
    let (status, body) = post(app, "/api/auth/login",
        json!({ "login": user, "password": "arm112" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn вход_курсанта_выдаёт_токен_и_службу() {
    let Some(app) = app().await else { return };

    let (status, body) = post(&app, "/api/auth/login",
        json!({ "login": "student1", "password": "arm112", "workstation": "12" })).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["role"], "student");
    // student1 — оператор 112: принимает вызовы, службы у него нет
    assert_eq!(body["user"]["duty"], "op112");
    assert!(body["user"]["serviceId"].is_null(), "{body}");
    // Номер АРМ вводится при входе и перекрывает записанный в профиле
    assert_eq!(body["user"]["workstation"], "12");
    assert!(body["token"].as_str().is_some_and(|t| t.split('.').count() == 3));
}

/// Неизвестный логин и неверный пароль должны отвечать одинаково,
/// иначе по ответу можно перебирать логины.
#[tokio::test]
async fn неверные_учётные_данные_неразличимы() {
    let Some(app) = app().await else { return };

    let (s1, b1) = post(&app, "/api/auth/login",
        json!({ "login": "student1", "password": "не тот" })).await;
    let (s2, b2) = post(&app, "/api/auth/login",
        json!({ "login": "такого-нет", "password": "arm112" })).await;

    assert_eq!(s1, StatusCode::UNAUTHORIZED);
    assert_eq!(s1, s2);
    assert_eq!(b1, b2);
}

#[tokio::test]
async fn me_без_токена_и_с_испорченным_токеном_отказывает() {
    let Some(app) = app().await else { return };

    let (no_token, _) = get_with(&app, "/api/auth/me", None).await;
    assert_eq!(no_token, StatusCode::UNAUTHORIZED);

    let token = login(&app, "student1").await;
    // Меняем подпись — сервер обязан отвергнуть
    let broken = format!("{}x", token);
    let (tampered, _) = get_with(&app, "/api/auth/me", Some(&broken)).await;
    assert_eq!(tampered, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_возвращает_номер_арм_из_сеанса_а_не_из_профиля() {
    let Some(app) = app().await else { return };

    let (_, body) = post(&app, "/api/auth/login",
        json!({ "login": "student1", "password": "arm112", "workstation": "77" })).await;
    let token = body["token"].as_str().unwrap();

    let (status, me) = get_with(&app, "/api/auth/me", Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["login"], "student1");
    assert_eq!(me["workstation"], "77",
        "человек мог сесть за чужое место — в карточку пойдёт номер из входа");
}

/// У преподавателя и администратора службы нет: они видят все карточки.
#[tokio::test]
async fn у_преподавателя_нет_привязки_к_службе() {
    let Some(app) = app().await else { return };

    let token = login(&app, "teacher1").await;
    let (status, me) = get_with(&app, "/api/auth/me", Some(&token)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["role"], "teacher");
    assert!(me["serviceId"].is_null());
}

/// Диспетчеры сидят в разных ДДС — иначе многопользовательское занятие
/// вырождается в одну ленту на всех.
#[tokio::test]
async fn курсанты_привязаны_к_разным_службам() {
    let Some(app) = app().await else { return };

    let mut services = Vec::new();
    for login_name in ["student2", "student3", "student4", "student5"] {
        let token = login(&app, login_name).await;
        let (_, me) = get_with(&app, "/api/auth/me", Some(&token)).await;
        assert_eq!(me["duty"], "dds", "{login_name} — диспетчер ДДС: {me}");
        services.push(me["serviceId"].as_str().unwrap().to_string());
    }

    let mut unique = services.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), services.len(), "службы курсантов дублируются: {services:?}");
}
