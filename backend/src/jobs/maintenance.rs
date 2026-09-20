//! Обслуживание базы: партиции аудита и резервные копии.
//!
//! Обе задачи незаметны, пока не понадобятся, и обе ломают систему
//! молча. Партиции аудита созданы миграцией до декабря 2026 — как
//! только этот срок пройдёт, любая запись в журнал начнёт падать,
//! а вместе с ней и действие, которое её вызвало.

use crate::audit;
use anyhow::{Context, Result};
use serde_json::json;
use sqlx::PgPool;
use std::{path::PathBuf, time::Duration};

/// Раз в сутки. Обе задачи от точного времени не зависят.
const TICK: Duration = Duration::from_secs(24 * 3600);

pub async fn run(pool: PgPool, backup_dir: Option<PathBuf>, keep_days: i64) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        // Первый такт срабатывает сразу: партиция может понадобиться
        // уже сегодня, а ждать сутки после запуска незачем
        ticker.tick().await;

        if let Err(e) = ensure_partitions(&pool).await {
            tracing::error!(error = %e, "не удалось создать партицию аудита");
        }

        if let Some(dir) = &backup_dir {
            match backup(&pool, dir, keep_days).await {
                Ok(path) => tracing::info!(?path, "резервная копия создана"),
                Err(e) => tracing::error!(error = %e, "резервное копирование не выполнено"),
            }
        }
    }
}

/// Создать партиции на текущий и следующие три месяца.
///
/// С запасом: если сервер простоит выключенным месяц, при следующем
/// запуске партиции всё равно найдутся.
pub async fn ensure_partitions(pool: &PgPool) -> Result<Vec<String>> {
    let mut created = Vec::new();

    for shift in 0..4 {
        let name: Option<String> = sqlx::query_scalar(
            "WITH bounds AS (
                SELECT date_trunc('month', now() + make_interval(months => $1))::date AS lo
             )
             SELECT 'audit_log_' || to_char(lo, 'YYYY_MM') FROM bounds",
        )
        .bind(shift)
        .fetch_one(pool)
        .await?;

        let Some(name) = name else { continue };

        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(&name)
            .fetch_one(pool)
            .await?;
        if exists {
            continue;
        }

        // Имя таблицы в DDL параметром не подставить, поэтому границы
        // считаем запросом, а саму команду собираем строкой. Значения
        // приходят из to_char по датам — подстановки чужого текста тут
        // быть не может
        let (lo, hi): (String, String) = sqlx::query_as(
            "WITH bounds AS (
                SELECT date_trunc('month', now() + make_interval(months => $1))::date AS lo
             )
             SELECT lo::text, (lo + interval '1 month')::date::text FROM bounds",
        )
        .bind(shift)
        .fetch_one(pool)
        .await?;

        sqlx::query(&format!(
            "CREATE TABLE IF NOT EXISTS {name}
             PARTITION OF audit_log FOR VALUES FROM ('{lo}') TO ('{hi}')"
        ))
        .execute(pool)
        .await
        .with_context(|| format!("создание партиции {name}"))?;

        tracing::info!(%name, %lo, %hi, "создана партиция аудита");
        created.push(name);
    }

    Ok(created)
}

/// Резервная копия через `pg_dump`.
///
/// Сжатый дамп в формате `custom`: восстанавливается выборочно и
/// занимает вчетверо меньше обычного SQL.
async fn backup(pool: &PgPool, dir: &PathBuf, keep_days: i64) -> Result<PathBuf> {
    tokio::fs::create_dir_all(dir).await?;

    let url = std::env::var("DATABASE_URL").context("не задан DATABASE_URL")?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("arm112-{stamp}.dump"));

    let output = tokio::process::Command::new("pg_dump")
        .arg("--format=custom")
        .arg("--file")
        .arg(&path)
        .arg(&url)
        .output()
        .await
        .context("не удалось запустить pg_dump — он установлен?")?;

    if !output.status.success() {
        anyhow::bail!("pg_dump: {}", String::from_utf8_lossy(&output.stderr).trim());
    }

    let removed = prune(dir, keep_days).await?;

    audit::log(
        pool,
        audit::Event::new("backup.create").with(json!({
            "file": path.file_name().and_then(|n| n.to_str()),
            "removedOld": removed,
            "keepDays": keep_days,
        })),
    )
    .await;

    Ok(path)
}

/// Удалить копии старше срока хранения. Иначе диск кончится
/// молча — и в тот же день, когда копия понадобится.
async fn prune(dir: &PathBuf, keep_days: i64) -> Result<usize> {
    let deadline = std::time::SystemTime::now()
        - Duration::from_secs(keep_days.max(1) as u64 * 24 * 3600);
    let mut removed = 0;
    let mut entries = tokio::fs::read_dir(dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("arm112-") || !name.ends_with(".dump") {
            continue;
        }
        let modified = entry.metadata().await.and_then(|m| m.modified());
        if matches!(modified, Ok(t) if t < deadline) {
            tokio::fs::remove_file(entry.path()).await?;
            removed += 1;
        }
    }

    Ok(removed)
}
