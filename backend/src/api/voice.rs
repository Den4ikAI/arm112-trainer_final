//! Голос в учебном разговоре: распознать реплику курсанта и озвучить
//! ответ собеседника.
//!
//! Сами модели — на сервере голоса (`ml/voice.rs`). Здесь только
//! вход с проверкой прав и честный отказ: голос не подключён — 503,
//! и клиент ведёт разговор текстом, как без голоса.
//!
//! Собеседники по ответу заказчика (чат, 27.09.2026): голосом ДДС
//! говорит со старшим бригады и с вышестоящим начальником.

use crate::{
    api::auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

/// Голоса собеседников. Какой стиль синтеза за каким — решает сервер
/// голоса, здесь только закрытый список ролей
pub const VOICES: [&str; 4] = ["brigade", "chief", "applicant_m", "applicant_f"];

/// Минута WAV 16 кГц моно — около 1,9 МБ; с запасом на 44,1 кГц,
/// если клиент не пересчитал частоту
const MAX_AUDIO_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_CHARS: usize = 600;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/voice/status", get(status))
        .route("/api/voice/asr", post(asr).layer(DefaultBodyLimit::max(MAX_AUDIO_BYTES)))
        .route("/api/voice/tts", post(tts))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    /// Реплику курсанта можно распознать
    pub asr: bool,
    /// Ответ собеседника можно озвучить
    pub tts: bool,
    pub voices: Vec<&'static str>,
}

#[utoipa::path(
    get,
    path = "/api/voice/status",
    tag = "Голос",
    responses(
        (status = 200, description = "Что из голоса подключено", body = VoiceStatus),
        (status = 401, description = "Требуется вход"),
    ),
)]
pub async fn status(State(state): State<AppState>, _user: AuthUser) -> Json<VoiceStatus> {
    Json(VoiceStatus {
        asr: state.ml.asr.available(),
        tts: state.ml.tts.available(),
        voices: VOICES.to_vec(),
    })
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Recognized {
    pub text: String,
    /// Длительность реплики, мс
    pub duration_ms: u32,
}

/// Распознать реплику. Тело — WAV (лучше 16 кГц моно: клиент
/// пересчитывает сам, чтобы серверу не нужен был ffmpeg).
#[utoipa::path(
    post,
    path = "/api/voice/asr",
    tag = "Голос",
    request_body(content = Vec<u8>, content_type = "audio/wav", description = "Реплика, WAV"),
    responses(
        (status = 200, description = "Распознанный текст", body = Recognized),
        (status = 400, description = "Пустое аудио"),
        (status = 401, description = "Требуется вход"),
        (status = 503, description = "Распознавание не подключено или не отвечает"),
    ),
)]
pub async fn asr(
    State(state): State<AppState>,
    _user: AuthUser,
    body: Bytes,
) -> AppResult<Json<Recognized>> {
    if !state.ml.asr.available() {
        return Err(AppError::Unavailable("распознавание речи не подключено".into()));
    }
    if body.is_empty() {
        return Err(AppError::Validation("пустое аудио".into()));
    }
    let transcript = state.ml.asr.transcribe(&body).await.map_err(|e| {
        tracing::warn!(error = %e, "распознавание не удалось");
        AppError::Unavailable("сервер голоса не ответил".into())
    })?;
    let duration_ms = transcript.segments.iter().map(|s| s.end_ms).max().unwrap_or(0);
    Ok(Json(Recognized { text: transcript.text, duration_ms }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpeakRequest {
    pub text: String,
    /// `brigade` — старший бригады, `chief` — вышестоящий начальник,
    /// `applicant_m` / `applicant_f` — заявитель в разговоре с оператором 112
    pub voice: String,
}

/// Озвучить реплику собеседника. Ответ — WAV.
#[utoipa::path(
    post,
    path = "/api/voice/tts",
    tag = "Голос",
    request_body = SpeakRequest,
    responses(
        (status = 200, description = "Звук, WAV", content_type = "audio/wav", body = Vec<u8>),
        (status = 400, description = "Пустой или слишком длинный текст, неизвестный голос"),
        (status = 401, description = "Требуется вход"),
        (status = 503, description = "Синтез не подключён или не отвечает"),
    ),
)]
pub async fn tts(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(req): Json<SpeakRequest>,
) -> AppResult<Response> {
    let text = req.text.trim();
    if text.is_empty() {
        return Err(AppError::Validation("пустой текст".into()));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(AppError::Validation(format!("текст длиннее {MAX_TEXT_CHARS} символов")));
    }
    if !VOICES.contains(&req.voice.as_str()) {
        return Err(AppError::Validation(format!("голос — один из {}", VOICES.join(", "))));
    }
    if !state.ml.tts.available() {
        return Err(AppError::Unavailable("синтез речи не подключён".into()));
    }
    let wav = state.ml.tts.wav(text, &req.voice).await.map_err(|e| {
        tracing::warn!(error = %e, "синтез не удался");
        AppError::Unavailable("сервер голоса не ответил".into())
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "audio/wav"),
            // Одна и та же фраза одним голосом звучит одинаково: клиенту
            // незачем качать её повторно
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        wav,
    )
        .into_response())
}
