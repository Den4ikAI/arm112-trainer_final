//! Справочники: типы происшествия, службы, опросные карты.
//!
//! Фронтенд держит их в кэше и перезапрашивает, когда меняется активная
//! версия классификатора. Отсюда заголовок `ETag`.

use crate::{error::AppResult, state::AppState};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/catalog/incident-types", get(incident_types))
        .route("/api/catalog/services", get(services))
        .route("/api/catalog/survey-cards/{type_id}", get(survey_card))
        .route("/api/catalog/applicant-statuses", get(applicant_statuses))
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct IncidentType {
    id: String,
    title: String,
    /// Показывать плашкой на пустой карточке
    frequent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_no: Option<i32>,
}

async fn incident_types(State(state): State<AppState>) -> AppResult<Json<Vec<IncidentType>>> {
    // Частые идут первыми — так же, как на кадре пустой карточки
    let rows = sqlx::query_as::<_, IncidentType>(
        "SELECT id, title, frequent, group_no FROM incident_types
          ORDER BY frequent DESC, title",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Service {
    id: String,
    title: String,
    is_primary: bool,
}

async fn services(State(state): State<AppState>) -> AppResult<Json<Vec<Service>>> {
    let rows = sqlx::query_as::<_, Service>(
        "SELECT id, title, is_primary FROM services ORDER BY title",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Question {
    id: String,
    label: String,
    kind: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    options: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SurveyCard {
    type_id: String,
    title: String,
    questions: Vec<Question>,
    /// Откуда взята карта: со скриншота боевой системы или выведена
    /// из классификатора. Фронтенд может это показывать преподавателю.
    source: &'static str,
}

/// Карты, снятые со скриншотов. Для остальных типов карта выводится
/// из классификатора — это отдельная задача.
const FROM_SCREENSHOT: [&str; 3] = ["101", "104", "explosion"];

async fn survey_card(
    State(state): State<AppState>,
    Path(type_id): Path<String>,
) -> AppResult<Response> {
    let title: Option<String> =
        sqlx::query_scalar("SELECT title FROM incident_types WHERE id = $1")
            .bind(&type_id)
            .fetch_optional(&state.db)
            .await?;

    let Some(title) = title else {
        return Err(crate::error::AppError::NotFound);
    };

    let questions: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, label, kind::text FROM survey_questions
          WHERE type_id = $1 ORDER BY sort_order",
    )
    .bind(&type_id)
    .fetch_all(&state.db)
    .await?;

    // Карты нет — это не ошибка: для большинства типов она ещё
    // не построена из классификатора. Отвечаем 204, чтобы фронтенд
    // отличал «нет карты» от «нет такого типа».
    if questions.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let mut out = Vec::with_capacity(questions.len());
    for (id, label, kind) in questions {
        let options: Vec<String> = sqlx::query_scalar(
            "SELECT value FROM survey_options
              WHERE type_id = $1 AND question_id = $2 ORDER BY sort_order",
        )
        .bind(&type_id)
        .bind(&id)
        .fetch_all(&state.db)
        .await?;

        out.push(Question { id, label, kind, options });
    }

    let source = if FROM_SCREENSHOT.contains(&type_id.as_str()) {
        "screenshot"
    } else {
        "classifier"
    };

    Ok(Json(SurveyCard { type_id, title, questions: out, source }).into_response())
}

/// Статусы заявителя. Источник — инструкция по заведению карточки,
/// раздел 5.3. Справочник неизменный, в базе держать незачем.
const APPLICANT_STATUSES: [&str; 6] = [
    "очевидец", "пострадавший", "родственник", "знакомый", "ребенок", "участник",
];

async fn applicant_statuses() -> (HeaderMap, Json<[&'static str; 6]>) {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "public, max-age=86400".parse().unwrap());
    (headers, Json(APPLICANT_STATUSES))
}
