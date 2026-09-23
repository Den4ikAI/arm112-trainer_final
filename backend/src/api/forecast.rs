//! Прогноз результата и его проверка; настраиваемые параметры.
//!
//! Критерий финальной экспертизы просит не прогноз, а его проверку —
//! «оценка достоверности путем сравнения реальных данных и прогнозных
//! значений». Поэтому главный эндпоинт здесь отдаёт не предсказание,
//! а то, насколько предыдущие предсказания сбылись.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    domain::{advice::Efficacy, forecast::Accuracy},
    error::{AppError, AppResult},
    reports::{advice, forecast},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/forecast/group/{id}", get(group))
        .route("/api/forecast/me", get(mine))
        .route("/api/advice/me", get(my_advice))
        .route("/api/advice/group/{id}", get(group_advice))
        .route("/api/settings", get(settings))
        .route("/api/settings/{key}", patch(set_setting))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupForecast {
    pub accuracy: Accuracy,
    /// Проверенные прогнозы: предсказали столько — получил столько
    pub checked: Vec<forecast::Checked>,
}

#[utoipa::path(
    get,
    path = "/api/forecast/group/{id}",
    tag = "Прогноз",
    params(("id" = String, Path, description = "Идентификатор группы")),
    responses(
        (status = 200, description = "Достоверность прогнозов по группе", body = GroupForecast),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn group(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<GroupForecast>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let (accuracy, checked) = forecast::group_accuracy(&state.db, id)
        .await
        .map_err(AppError::Other)?;
    Ok(Json(GroupForecast { accuracy, checked }))
}

/// Свой прогноз и то, как он сбывался.
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MyPrediction {
    pub session_id: Uuid,
    pub predicted: i32,
    pub actual: Option<i32>,
    pub basis: i32,
    pub trained: bool,
    /// Из чего сложился прогноз: вклад каждого признака в баллах
    pub features: Value,
    pub made_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/forecast/me",
    tag = "Прогноз",
    responses(
        (status = 200, description = "Свои прогнозы и факты", body = Vec<MyPrediction>),
        (status = 401, description = "Требуется вход"),
    ),
)]
pub async fn mine(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<MyPrediction>>> {
    let rows: Vec<MyPrediction> = sqlx::query_as(
        "SELECT session_id, predicted, actual, basis, trained, features, made_at
           FROM predictions WHERE user_id = $1
          ORDER BY made_at DESC LIMIT 50",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get,
    path = "/api/advice/me",
    tag = "Прогноз",
    responses(
        (status = 200, description = "Свои рекомендации: выданные и проверенные", body = Vec<advice::AdviceView>),
        (status = 401, description = "Требуется вход"),
    ),
)]
pub async fn my_advice(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<advice::AdviceView>>> {
    Ok(Json(advice::mine(&state.db, user.id).await.map_err(AppError::Other)?))
}

/// Насколько рекомендации работают в группе.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupAdvice {
    pub efficacy: Efficacy,
    /// Проверенные рекомендации: что советовали и сработало ли
    pub recent: Vec<advice::CheckedAdvice>,
}

#[utoipa::path(
    get,
    path = "/api/advice/group/{id}",
    tag = "Прогноз",
    params(("id" = String, Path, description = "Идентификатор группы")),
    responses(
        (status = 200, description = "Доля сработавших рекомендаций и базовая линия", body = GroupAdvice),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn group_advice(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<GroupAdvice>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let (efficacy, recent) = advice::group_efficacy(&state.db, id)
        .await
        .map_err(AppError::Other)?;
    Ok(Json(GroupAdvice { efficacy, recent }))
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub key: String,
    pub value: Value,
    pub title: String,
    pub hint: String,
}

#[utoipa::path(
    get,
    path = "/api/settings",
    tag = "Прогноз",
    responses(
        (status = 200, description = "Настраиваемые параметры", body = Vec<Setting>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn settings(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<Setting>>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let rows: Vec<Setting> =
        sqlx::query_as("SELECT key, value, title, hint FROM settings ORDER BY key")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewValue {
    pub value: f64,
}

/// Пределы значений. Настраиваемость не означает, что можно выставить
/// что угодно: допуск в сто баллов сделал бы прогноз «точным» всегда.
fn предел(key: &str) -> Option<(f64, f64)> {
    Some(match key {
        "forecast.tolerance" => (1.0, 30.0),
        "forecast.depth" => (3.0, 20.0),
        "forecast.lambda" => (0.0, 10.0),
        "attestation.threshold" => (1.0, 100.0),
        "norm.ack_sec" => (5.0, 600.0),
        "norm.completion_hours" => (1.0, 720.0),
        "advice.window" => (2.0, 20.0),
        _ => return None,
    })
}

#[utoipa::path(
    patch,
    path = "/api/settings/{key}",
    tag = "Прогноз",
    params(("key" = String, Path, description = "Ключ параметра")),
    request_body = NewValue,
    responses(
        (status = 200, description = "Новое значение", body = Setting),
        (status = 400, description = "Значение вне допустимых пределов"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn set_setting(
    State(state): State<AppState>,
    user: AuthUser,
    Path(key): Path<String>,
    Json(req): Json<NewValue>,
) -> AppResult<Json<Setting>> {
    // Нормативы и порог аттестации — administrator's call: они
    // общие для всех групп, а преподаватель отвечает за свою
    user.require(&[Role::Admin])?;

    let Some((низ, верх)) = предел(&key) else {
        return Err(AppError::NotFound);
    };
    if !(низ..=верх).contains(&req.value) {
        return Err(AppError::Validation(format!(
            "значение должно быть от {низ} до {верх}"
        )));
    }

    let обновлено: Option<Setting> = sqlx::query_as(
        "UPDATE settings SET value = $2, updated_at = now(), updated_by = $3
          WHERE key = $1
      RETURNING key, value, title, hint",
    )
    .bind(&key)
    .bind(json!(req.value))
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    let setting = обновлено.ok_or(AppError::NotFound)?;

    audit::log(
        &state.db,
        audit::Event::new("setting.change")
            .from_user(&user)
            .about("setting", Uuid::nil())
            .with(json!({ "key": key, "value": req.value })),
    )
    .await;

    Ok(Json(setting))
}
