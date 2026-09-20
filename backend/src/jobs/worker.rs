//! Воркер очереди `ml_jobs`.
//!
//! Очередь на таблице, без брокера: на железе из ТЗ (6 ядер, без GPU)
//! лишний процесс дороже, чем один `SELECT … FOR UPDATE SKIP LOCKED`.
//! `SKIP LOCKED` позволяет поднять несколько экземпляров сервера,
//! не выдавая одну задачу двоим.
//!
//! Сюда уходит всё, что не должно ждать в HTTP-запросе: отчёты по
//! группе (норматив ТЗ — 30 секунд), сводки по типичным ошибкам,
//! генерация сценариев.

use crate::{ml::MlSuite, reports, state::AppState};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

/// Пауза, когда очередь пуста. Отчёт по группе ждать 30 секунд
/// нельзя, а держать постоянный опрос базы незачем.
const IDLE: Duration = Duration::from_millis(500);

pub async fn run(state: AppState) {
    loop {
        match take_one(&state.db).await {
            Ok(Some(job)) => {
                let id = job.id;
                let result = execute(&state, &job).await;
                if let Err(e) = finish(&state.db, id, result).await {
                    tracing::error!(error = %e, %id, "не удалось записать результат задачи");
                }
            }
            Ok(None) => tokio::time::sleep(IDLE).await,
            Err(e) => {
                tracing::error!(error = %e, "воркер не смог взять задачу");
                tokio::time::sleep(IDLE).await;
            }
        }
    }
}

pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub input: Value,
}

/// Взять одну задачу и сразу пометить её взятой. `SKIP LOCKED`
/// пропускает строки, которые уже держит другой экземпляр.
async fn take_one(pool: &PgPool) -> anyhow::Result<Option<Job>> {
    let row: Option<(Uuid, String, Value)> = sqlx::query_as(
        "UPDATE ml_jobs SET status = 'running'
          WHERE id = (
            SELECT id FROM ml_jobs
             WHERE status = 'queued'
             ORDER BY created_at
             FOR UPDATE SKIP LOCKED
             LIMIT 1
          )
        RETURNING id, kind, input",
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, kind, input)| Job { id, kind, input }))
}

async fn execute(state: &AppState, job: &Job) -> anyhow::Result<Value> {
    tracing::info!(id = %job.id, kind = %job.kind, "выполняю задачу");

    match job.kind.as_str() {
        "report.group" => reports::group::build(state, &job.input).await,
        "insights.group" => group_insights(&state.db, &state.ml, &job.input).await,
        other => anyhow::bail!("неизвестный вид задачи: {other}"),
    }
}

/// Сводка по типичным ошибкам группы. Статистику считаем сами,
/// модель только формулирует: числа она не выдумывает.
async fn group_insights(
    pool: &PgPool,
    ml: &Arc<MlSuite>,
    input: &Value,
) -> anyhow::Result<Value> {
    let stats = reports::group::stats(pool, input).await?;
    let text = ml.llm.group_insights(serde_json::to_value(&stats)?).await?;
    Ok(json!({ "stats": stats, "text": text }))
}

async fn finish(pool: &PgPool, id: Uuid, result: anyhow::Result<Value>) -> anyhow::Result<()> {
    match result {
        Ok(output) => {
            sqlx::query(
                "UPDATE ml_jobs SET status = 'done', output = $2, finished_at = now()
                  WHERE id = $1",
            )
            .bind(id)
            .bind(output)
            .execute(pool)
            .await?;
        }
        Err(e) => {
            // Текст ошибки сохраняем: преподаватель должен увидеть,
            // почему отчёт не собрался, а не бесконечный «в работе»
            tracing::error!(error = %e, %id, "задача не выполнена");
            sqlx::query(
                "UPDATE ml_jobs SET status = 'failed', error = $2, finished_at = now()
                  WHERE id = $1",
            )
            .bind(id)
            .bind(e.to_string())
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// Поставить задачу в очередь.
pub async fn enqueue(
    pool: &PgPool,
    kind: &str,
    input: Value,
    created_by: Uuid,
) -> anyhow::Result<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO ml_jobs (kind, input, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(kind)
    .bind(input)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(id)
}
