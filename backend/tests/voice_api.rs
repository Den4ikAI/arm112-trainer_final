//! Голос: распознавание реплики и озвучивание собеседника.
//!
//! Настоящий сервер голоса тестам не нужен: его заменяет крошечный
//! сервер тут же, в тесте. Проверяется то, за что отвечает сервер
//! приложения, — права, проверка входа и честный отказ 503, когда
//! голоса нет.
//!
//! Запуск: `cargo test --test voice_api`

use arm112::{
    api,
    config::Config,
    ml::{
        voice::{VoiceAsr, VoiceClient, VoiceTts},
        MlSuite,
    },
    state::AppState,
};
use axum::{
    body::{Body, Bytes},
    http::{header, Method, Request, StatusCode},
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

/// Приложение с голосом по адресу `voice` или без голоса вовсе.
async fn app(voice: Option<String>) -> Option<Router> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let mut ml = MlSuite::build(arm112::config::MlProvider::Mock).ok()?;
    if let Some(url) = voice {
        let client = VoiceClient::new(url).ok()?;
        ml.asr = Box::new(VoiceAsr(client.clone()));
        ml.tts = Box::new(VoiceTts(client));
    }
    Some(api::routes().with_state(AppState::new(pool, config, ml)))
}

/// Подменный сервер голоса: распознаёт всё как одну фразу и
/// «синтезирует» короткий WAV.
async fn поддельный_голос() -> String {
    let router = Router::new()
        .route("/asr", post(|body: Bytes| async move {
            Json(json!({ "text": format!("Прибыли на место ({} байт)", body.len()),
                         "durationMs": 1500 }))
        }))
        .route("/tts", post(|Json(req): Json<Value>| async move {
            let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
            wav.extend_from_slice(req["voice"].as_str().unwrap_or("").as_bytes());
            ([(header::CONTENT_TYPE, "audio/wav")], wav)
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

async fn token(app: &Router, login: &str) -> String {
    let (_, b, _) = send(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" })), None).await;
    let b: Value = serde_json::from_slice(&b).unwrap();
    b["token"].as_str().unwrap().to_string()
}

/// Запрос: JSON или сырое тело. Возвращает статус, тело и тип.
async fn send(
    app: &Router, m: Method, path: &str, token: Option<&str>,
    json: Option<Value>, raw: Option<Vec<u8>>,
) -> (StatusCode, Bytes, String) {
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match (json, raw) {
        (Some(b), _) => req.header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        (None, Some(r)) => req.header(header::CONTENT_TYPE, "audio/wav").body(Body::from(r)),
        (None, None) => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let kind = resp.headers().get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    (status, resp.into_body().collect().await.unwrap().to_bytes(), kind)
}

fn json_of(b: &Bytes) -> Value {
    serde_json::from_slice(b).unwrap_or(Value::Null)
}

/// Без сервера голоса — честный отказ: клиент по нему переходит на
/// текст. Заглушку распознавания выдавать за слова курсанта нельзя.
#[tokio::test]
async fn без_сервера_голоса_отказ_503_и_статус_выключен() {
    let Some(app) = app(None).await else { return };
    let t = token(&app, "student2").await;

    let (status, body, _) = send(&app, Method::GET, "/api/voice/status", Some(&t), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&body)["asr"], false);
    assert_eq!(json_of(&body)["tts"], false);

    let (status, body, _) = send(&app, Method::POST, "/api/voice/asr", Some(&t),
        None, Some(vec![1, 2, 3])).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_of(&body)["error"], "unavailable");

    let (status, _, _) = send(&app, Method::POST, "/api/voice/tts", Some(&t),
        Some(json!({ "text": "Прибыли на место", "voice": "brigade" })), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn без_входа_голос_недоступен() {
    let Some(app) = app(None).await else { return };
    for path in ["/api/voice/status"] {
        let (status, _, _) = send(&app, Method::GET, path, None, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _, _) = send(&app, Method::POST, "/api/voice/asr", None, None, Some(vec![1])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = send(&app, Method::POST, "/api/voice/tts", None,
        Some(json!({ "text": "т", "voice": "brigade" })), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn с_сервером_голоса_реплика_распознаётся_а_ответ_озвучивается() {
    let url = поддельный_голос().await;
    let Some(app) = app(Some(url)).await else { return };
    let t = token(&app, "student2").await;

    let (status, body, _) = send(&app, Method::GET, "/api/voice/status", Some(&t), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&body)["asr"], true);
    assert_eq!(json_of(&body)["voices"], json!(["brigade", "chief"]));

    let (status, body, _) = send(&app, Method::POST, "/api/voice/asr", Some(&t),
        None, Some(vec![0u8; 100])).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(json_of(&body)["text"], "Прибыли на место (100 байт)");
    assert_eq!(json_of(&body)["durationMs"], 1500);

    // Голос начальника — его зовёт доклад вышестоящему
    let (status, body, kind) = send(&app, Method::POST, "/api/voice/tts", Some(&t),
        Some(json!({ "text": "Доложите обстановку", "voice": "chief" })), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(kind, "audio/wav");
    assert!(body.starts_with(b"RIFF") && body.ends_with(b"chief"));
}

/// Роли собеседников — закрытый список: произвольная строка до сервера
/// голоса не доходит. Пустой и слишком длинный текст — тоже.
#[tokio::test]
async fn синтез_проверяет_голос_и_текст() {
    let url = поддельный_голос().await;
    let Some(app) = app(Some(url)).await else { return };
    let t = token(&app, "student2").await;

    for req in [
        json!({ "text": "Прибыли", "voice": "ru_m5" }),
        json!({ "text": "   ", "voice": "brigade" }),
        json!({ "text": "а".repeat(601), "voice": "brigade" }),
    ] {
        let (status, body, _) = send(&app, Method::POST, "/api/voice/tts", Some(&t),
            Some(req.clone()), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{req} → {body:?}");
    }

    let (status, _, _) = send(&app, Method::POST, "/api/voice/asr", Some(&t),
        None, Some(Vec::new())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "пустое аудио");
}

/// Сервер голоса упал посреди занятия — не 500, а тот же 503: клиент
/// спокойно переходит на текст.
#[tokio::test]
async fn упавший_сервер_голоса_даёт_503_а_не_500() {
    // Порт, на котором заведомо никто не слушает
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let Some(app) = app(Some(url)).await else { return };
    let t = token(&app, "student2").await;

    let (status, _, _) = send(&app, Method::POST, "/api/voice/asr", Some(&t),
        None, Some(vec![1, 2, 3])).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _, _) = send(&app, Method::POST, "/api/voice/tts", Some(&t),
        Some(json!({ "text": "Прибыли", "voice": "brigade" })), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
