//! Сторож нормативов: 30 секунд на подтверждение, 48 часов на работы.
//!
//! Время не ждём, а сдвигаем `saved_at` в прошлое — иначе один тест
//! занимал бы двое суток.
//!
//! Запуск: `cargo test --test watchdog`

use arm112::{api, config::Config, jobs::watchdog, ml::MlSuite, state::AppState};
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

/// Оператор заполняет и сохраняет карточку пожара в частном доме.
async fn saved_card(app: &Router, pool: &PgPool) -> Option<Uuid> {
    let tok = token(app, "student1").await;
    let scenario: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;

    let (_, s) = call(app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    let session = s["session"]["id"].as_str().unwrap();

    call(app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({
            "typeIds": ["101"],
            "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                         "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                         "inhouse": ["Крыша"], "gasified": ["Нет"] }
        }))).await;

    let (_, saved) = call(app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&tok), None).await;
    Some(saved["card"]["id"].as_str().unwrap().parse().unwrap())
}

/// Сдвинуть момент сохранения в прошлое.
async fn age(pool: &PgPool, card: Uuid, seconds: i64) {
    sqlx::query("UPDATE cards SET saved_at = now() - make_interval(secs => $2) WHERE id = $1")
        .bind(card)
        .bind(seconds as f64)
        .execute(pool)
        .await
        .unwrap();
}

async fn status(pool: &PgPool, card: Uuid) -> String {
    sqlx::query_scalar("SELECT status::text FROM cards WHERE id = $1")
        .bind(card)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Норматив памятки ДДС — 30 секунд на подтверждение карточки.
#[tokio::test]
async fn карточка_без_подтверждения_краснеет_через_тридцать_секунд() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    age(&pool, card, 29).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Зарегистрирована",
        "до истечения норматива молчание службы не нарушение");

    age(&pool, card, 31).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Не оповещено");
}

/// Каждый автоматический переход обязан оставить след: иначе потом
/// не объяснить, почему карточка оказалась в «Не оповещено».
#[tokio::test]
async fn переход_попадает_в_аудит() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    age(&pool, card, 31).await;
    watchdog::sweep(&pool).await.unwrap();

    let entry: Option<(String, Value)> = sqlx::query_as(
        "SELECT action, payload FROM audit_log
          WHERE entity = 'card' AND entity_id = $1 ORDER BY at DESC LIMIT 1",
    )
    .bind(card.to_string())
    .fetch_optional(&pool)
    .await
    .unwrap();

    let (action, payload) = entry.expect("перехода нет в аудите");
    assert_eq!(action, "card.status.auto");
    assert_eq!(payload["to"], "Не оповещено");
    assert_eq!(payload["from"], "Зарегистрирована");
}

/// Второй проход по той же карточке ничего не меняет и не плодит
/// записей аудита — задача бежит каждые пять секунд.
#[tokio::test]
async fn повторный_проход_не_плодит_записи() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    age(&pool, card, 31).await;
    watchdog::sweep(&pool).await.unwrap();

    let count = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_log WHERE entity = 'card' AND entity_id = $1")
            .bind(card.to_string())
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let before = count().await;

    watchdog::sweep(&pool).await.unwrap();
    watchdog::sweep(&pool).await.unwrap();

    assert_eq!(count().await, before, "статус не изменился — записи быть не должно");
}

/// Диспетчер подтвердил приём — карточка уходит из «Не оповещено».
#[tokio::test]
async fn подтверждение_службой_снимает_не_оповещено() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    age(&pool, card, 31).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Не оповещено");

    // Все службы карточки подтверждают приём
    let services: Vec<String> = sqlx::query_scalar(
        "SELECT service_id FROM card_services WHERE card_id = $1")
        .bind(card).fetch_all(&pool).await.unwrap();
    for s in services {
        sqlx::query(
            "INSERT INTO response_entries (card_id, service_id, status, operator)
             VALUES ($1, $2, 'Принята', 'тест')")
            .bind(card).bind(&s).execute(&pool).await.unwrap();
    }

    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Отработана");
}

/// 48 часов без «Работы завершены» — карточка застряла.
#[tokio::test]
async fn через_сорок_восемь_часов_карточка_не_завершена() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    let services: Vec<String> = sqlx::query_scalar(
        "SELECT service_id FROM card_services WHERE card_id = $1")
        .bind(card).fetch_all(&pool).await.unwrap();
    for s in &services {
        sqlx::query(
            "INSERT INTO response_entries (card_id, service_id, status, operator)
             VALUES ($1, $2, 'Принята', 'тест')")
            .bind(card).bind(s).execute(&pool).await.unwrap();
    }

    age(&pool, card, 47 * 3600).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Отработана");

    age(&pool, card, 49 * 3600).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Не завершено");
}

/// Все службы закончили — карточка завершена, и 48 часов ей уже
/// не страшны.
#[tokio::test]
async fn завершение_всеми_службами_закрывает_карточку() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    let services: Vec<String> = sqlx::query_scalar(
        "SELECT service_id FROM card_services WHERE card_id = $1")
        .bind(card).fetch_all(&pool).await.unwrap();
    for s in &services {
        for st in ["Принята", "Работы завершены"] {
            sqlx::query(
                "INSERT INTO response_entries (card_id, service_id, status, operator)
                 VALUES ($1, $2, $3::response_status, 'тест')")
                .bind(card).bind(s).bind(st).execute(&pool).await.unwrap();
        }
    }

    age(&pool, card, 100 * 3600).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Завершена");
}

/// Отметку преподавателя фоновая задача не трогает: иначе она
/// пропадала бы через пять секунд.
#[tokio::test]
async fn проверенную_карточку_сторож_не_перебивает() {
    let Some((app, pool)) = app().await else { return };
    let Some(card) = saved_card(&app, &pool).await else { return };

    sqlx::query("UPDATE cards SET status = 'Проверена' WHERE id = $1")
        .bind(card).execute(&pool).await.unwrap();

    age(&pool, card, 3600).await;
    watchdog::sweep(&pool).await.unwrap();
    assert_eq!(status(&pool, card).await, "Проверена");
}
