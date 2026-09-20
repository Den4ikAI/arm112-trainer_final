//! Прогон генерации одного вызова на живой модели.
//!
//! Запуск: `ML_BASE_URL=http://127.0.0.1:8090 cargo run --bin try_generate -- 3 1`

use arm112::{config::Config, domain::scenario_gen, ingest::tickets, ml::http::HttpLlm};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let ticket: i32 = std::env::args().nth(1).unwrap_or_else(|| "3".into()).parse()?;
    let call_no: i32 = std::env::args().nth(2).unwrap_or_else(|| "1".into()).parse()?;

    let config = Config::from_env()?;
    let pool = sqlx::PgPool::connect(&config.database_url).await?;

    let version_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1")
        .fetch_one(&pool).await?;

    let text = std::fs::read_to_string("../extracted/bilety_96_scenarios.md")?;
    let calls = tickets::parse(&text)?;
    let call = calls.iter()
        .find(|c| c.ticket == ticket && c.call_no == call_no)
        .ok_or_else(|| anyhow::anyhow!("нет такого вызова"))?;

    println!("── билет {ticket}, вызов {call_no} ──");
    println!("{}\n", call.situation);

    let llm = HttpLlm::new(
        std::env::var("ML_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8090".into()),
        "local".into(),
    )?;

    let started = std::time::Instant::now();
    let out = scenario_gen::generate(&llm, &pool, version_id, call).await?;
    println!("{}", serde_json::to_string_pretty(&out)?);
    println!("\nзаняло {:.0} с", started.elapsed().as_secs_f32());
    Ok(())
}
