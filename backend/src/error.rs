//! Единый тип ошибки и её преобразование в HTTP-ответ.
//!
//! Коды соответствуют разделу «Коды ошибок» в BACKEND.md — фронтенд
//! и тесты полагаются именно на строку в поле `error`.

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("validation_error: {0}")]
    Validation(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("not_found")]
    NotFound,

    #[error("conflict: {0}")]
    Conflict(String),

    /// Для «Не принята» и «Отказа от выполнения работ» комментарий обязателен
    #[error("comment_required")]
    CommentRequired,

    /// Статусы реагирования выбираются только последовательно
    #[error("status_not_allowed")]
    StatusNotAllowed { allowed: Vec<String> },

    /// Автоподобранную службу удалить нельзя
    #[error("auto_service_removal")]
    AutoServiceRemoval,

    /// Внешний компонент не подключён или не отвечает — например,
    /// сервер голоса. Клиент должен перейти на запасной путь
    #[error("unavailable: {0}")]
    Unavailable(String),

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, extra) = match &self {
            Self::Validation(m) => (StatusCode::BAD_REQUEST, "validation_error", json!({ "detail": m })),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", json!({})),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden", json!({})),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", json!({})),
            Self::Conflict(m) => (StatusCode::CONFLICT, "conflict", json!({ "detail": m })),
            Self::CommentRequired => (StatusCode::UNPROCESSABLE_ENTITY, "comment_required", json!({})),
            Self::StatusNotAllowed { allowed } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "status_not_allowed",
                json!({ "allowed": allowed }),
            ),
            Self::AutoServiceRemoval => (
                StatusCode::UNPROCESSABLE_ENTITY, "auto_service_removal", json!({}),
            ),
            Self::Unavailable(m) => {
                (StatusCode::SERVICE_UNAVAILABLE, "unavailable", json!({ "detail": m }))
            }
            Self::Db(e) => {
                tracing::error!(error = %e, "ошибка базы данных");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", json!({}))
            }
            Self::Other(e) => {
                tracing::error!(error = %e, "внутренняя ошибка");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", json!({}))
            }
        };

        let mut body = json!({ "error": code });
        if let (Some(obj), Some(ex)) = (body.as_object_mut(), extra.as_object()) {
            obj.extend(ex.clone());
        }
        (status, Json(body)).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
