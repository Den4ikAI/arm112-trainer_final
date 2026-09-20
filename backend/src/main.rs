//! Точка входа HTTP-сервера.

use anyhow::{Context, Result};
use arm112::{api, config::Config, jobs, ml::MlSuite, state::AppState, store};
use axum::Router;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    init_tracing();

    tracing::info!(provider = ?config.ml_provider, "поднимаю адаптеры моделей");
    let ml = MlSuite::build(config.ml_provider)?;

    tracing::info!("подключаюсь к базе");
    let db = store::connect(&config.database_url).await?;
    store::migrate(&db).await?;
    tracing::info!("миграции применены");

    let bind_addr = config.bind_addr.clone();
    let state = AppState::new(db, config, ml);

    // Сторож нормативов: 30 секунд на подтверждение карточки,
    // 48 часов на завершение работ (памятка ДДС)
    tokio::spawn(jobs::watchdog::run(state.db.clone(), state.ws.clone()));
    // Очередь отчётов и сводок: норматив ТЗ на отчёт по группе
    // 30 секунд, в HTTP-запросе его держать незачем
    tokio::spawn(jobs::worker::run(state.clone()));
    tracing::info!("фоновые задачи запущены");

    let app = Router::new()
        .merge(api::routes())
        .merge(arm112::ws::routes::routes())
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        // Dev-режим: фронтенд живёт на другом порту.
        // В проде оба отдаются с одного адреса и CORS не нужен.
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("не удалось занять адрес {bind_addr}"))?;

    tracing::info!("слушаю http://{bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().compact())
        .init();
}
