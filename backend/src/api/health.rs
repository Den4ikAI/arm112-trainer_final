//! Проверка живости. Используется docker healthcheck и админкой.

use crate::{error::AppResult, state::AppState};
use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct Health {
    /// Отвечает ли база
    db: bool,
    /// Сколько секунд прошло с запуска процесса
    uptime_sec: u64,
    /// Какой провайдер моделей поднят
    ml_provider: String,
    /// Открытых соединений канала класса
    ws_listeners: usize,
    /// Задач в очереди и в работе
    jobs_queued: i64,
    jobs_running: i64,
    version: &'static str,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/admin/health", get(health))
}

#[utoipa::path(
    get,
    path = "/api/admin/health",
    tag = "Состояние",
    responses(
        (status = 200, description = "Живость сервиса", body = Health),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn health(State(state): State<AppState>) -> AppResult<Json<Health>> {
    let db = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    // Очередь показываем отдельно: залипшая задача видна по тому,
    // что running не уменьшается
    let (queued, running): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'queued'),
                count(*) FILTER (WHERE status = 'running')
           FROM ml_jobs",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or((0, 0));

    Ok(Json(Health {
        db,
        uptime_sec: state.started_at.elapsed().as_secs(),
        ws_listeners: state.ws.listeners(),
        jobs_queued: queued,
        jobs_running: running,
        ml_provider: format!("{:?}", state.config.ml_provider).to_lowercase(),
        version: env!("CARGO_PKG_VERSION"),
    }))
}
