//! Полосы очереди задач: отчёт по группе не ждёт за моделью.
//!
//! Норматив ТЗ на аналитический отчёт — 30 секунд. Отчёт считает база
//! за доли секунды, но стоял в одной очереди с анализом текста моделью —
//! десятки секунд на задачу на процессоре. Класс, разом сохранивший
//! карточки, отодвигал отчёт на минуты: на проверке 29.09 два отчёта
//! ждали за 131 задачей модели.
//!
//! Запуск: `cargo test --test job_lanes`

use arm112::{config::Config, jobs::worker};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn база() -> Option<PgPool> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    Some(pool)
}

async fn поставить(pool: &PgPool, kind: &str, метка: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO ml_jobs (kind, input) VALUES ($1, $2) RETURNING id")
        .bind(kind)
        .bind(json!({ "метка": метка }))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Быстрая полоса берёт отчёт, даже если впереди в очереди задачи
/// модели, и задач модели не трогает.
#[tokio::test]
async fn отчёт_по_группе_не_ждёт_за_задачами_модели() {
    let Some(pool) = база().await else { return };
    let метка = Uuid::new_v4().to_string();

    // Сначала задачи модели — они старше, в общей очереди были бы первыми
    let модели = [
        поставить(&pool, "analysis.text", &метка).await,
        поставить(&pool, "analysis.text", &метка).await,
        поставить(&pool, "insights.group", &метка).await,
    ];
    let отчёт = поставить(&pool, "report.group", &метка).await;

    // В общей базе могут стоять чужие отчёты — старше нашего. Их держим
    // взятыми, пока не дойдём до своего, и только потом возвращаем
    // в очередь: вернуть сразу — следующий вызов отдал бы тот же самый
    let mut взято = None;
    let mut чужие = Vec::new();
    for _ in 0..500 {
        let Some(job) = worker::take_one(&pool, worker::Lane::Fast).await.unwrap() else { break };
        assert!(worker::FAST_KINDS.contains(&job.kind.as_str()),
                "быстрая полоса взяла задачу модели: {}", job.kind);
        if job.id == отчёт {
            взято = Some(job.id);
            break;
        }
        чужие.push(job.id);
    }
    sqlx::query("UPDATE ml_jobs SET status = 'queued' WHERE id = ANY($1)")
        .bind(&чужие).execute(&pool).await.unwrap();
    assert_eq!(взято, Some(отчёт), "отчёт должен взяться, не дожидаясь задач модели");

    let ждут: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ml_jobs WHERE id = ANY($1) AND status = 'queued'")
        .bind(&модели[..]).fetch_one(&pool).await.unwrap();
    assert_eq!(ждут, 3, "задачи модели быстрая полоса не трогает");

    sqlx::query("DELETE FROM ml_jobs WHERE input ->> 'метка' = $1")
        .bind(&метка).execute(&pool).await.unwrap();
}
