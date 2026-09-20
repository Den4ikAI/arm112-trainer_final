//! Импорт Единого классификатора происшествий из XLSX.
//!
//! ```bash
//! cargo run --bin import_classifier -- <файл.xlsx> ["название версии"]
//! ```
//!
//! Версия создаётся неактивной. Активация — отдельное действие,
//! она запускает пересчёт сценариев и переаттестацию.

use anyhow::{Context, Result};
use arm112::{config::Config, ingest::classifier, store};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let path: PathBuf = args
        .next()
        .context("укажите путь к xlsx: cargo run --bin import_classifier -- файл.xlsx")?
        .into();

    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let name = args.next().unwrap_or_else(|| file_name.clone());

    println!("Читаю {}", path.display());
    let parsed = classifier::parse(&path)?;

    println!("  служб:      {}", parsed.services.len());
    println!("  групп:      {}", parsed.groups.len());
    println!("  позиций:    {}", parsed.positions.len());

    let config = Config::from_env()?;
    let pool = store::connect(&config.database_url).await?;
    store::migrate(&pool).await?;

    let report = classifier::store(&pool, &parsed, &name, &file_name).await?;

    println!("\nЗагружено как версия «{name}»");
    println!("  id:            {}", report.version_id);
    println!("  оповещений:    {}", report.notifications);
    println!("\nГруппы:");
    for (no, title) in &parsed.groups {
        let count = parsed.positions.iter().filter(|p| p.group_no == *no).count();
        println!("  {no:>2}. {title}  — позиций: {count}");
    }

    Ok(())
}
