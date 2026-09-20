//! Проверка живости. Используется docker healthcheck и админкой.

use crate::{error::AppResult, state::AppState};
use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

#[derive(Serialize)]
pub struct Health {
    /// Отвечает ли база
    db: bool,
    /// Сколько секунд прошло с запуска процесса
    uptime_sec: u64,
    /// Какой провайдер моделей поднят
    ml_provider: String,
    version: &'static str,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/admin/health", get(health))
}

async fn health(State(state): State<AppState>) -> AppResult<Json<Health>> {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    Ok(Json(Health {
        db,
        uptime_sec: state.started_at.elapsed().as_secs(),
        ml_provider: format!("{:?}", state.config.ml_provider).to_lowercase(),
        version: env!("CARGO_PKG_VERSION"),
    }))
}
