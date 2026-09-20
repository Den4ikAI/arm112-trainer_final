//! Работа с PostgreSQL: пул соединений и миграции.

use anyhow::{Context, Result};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Пул на 32 соединения: ТЗ требует 20 одновременных сессий,
/// плюс запас на фоновые задачи и вебсокеты.
const MAX_CONNECTIONS: u32 = 32;

pub async fn connect(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .context("не удалось подключиться к PostgreSQL")
}

/// Накатывает миграции из каталога `migrations/`.
///
/// Файлы именуются с timestamp-префиксом и **никогда не редактируются**
/// после мержа — только добавляется следующий. Так двое разработчиков
/// физически не могут столкнуться в одной миграции.
pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("не удалось применить миграции")?;
    Ok(())
}
