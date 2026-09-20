//! Точка входа HTTP-сервера.

use anyhow::{Context, Result};
use arm112::{api, api::openapi::ApiDoc, config::Config, jobs, ml::MlSuite, state::AppState, store};
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
    // Партиции аудита и резервные копии. Без первого журнал
    // безопасности перестанет писаться, как только кончатся
    // созданные миграцией месяцы
    tokio::spawn(jobs::maintenance::run(
        state.db.clone(),
        state.config.backup_dir.clone(),
        state.config.backup_keep_days,
    ));
    tracing::info!("фоновые задачи запущены");

    let app = Router::new()
        .merge(api::routes())
        .merge(arm112::ws::routes::routes())
        // Схема и Swagger UI: по ТЗ решение должно уметь встраиваться
        // в стороннюю систему, а для этого описание нужно читаемое
        // не только человеком
        .merge(
            utoipa_swagger_ui::SwaggerUi::new("/api/docs")
                .url("/api/openapi.json", <ApiDoc as utoipa::OpenApi>::openapi()),
        )
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
    // ConnectInfo — запасной источник адреса для аудита, когда
    // сервер стоит без обратного прокси
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().compact())
        .init();
}
