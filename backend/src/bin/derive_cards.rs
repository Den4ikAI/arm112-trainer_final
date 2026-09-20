//! Разбор опросных карт, выводимых из классификатора.
//!
//! Утилита показывает, что получается из данных, и где данных не хватает.
//! Сочинять недостающее нельзя — если группа не разворачивается в карту,
//! это надо видеть, а не заполнять правдоподобным.
//!
//! ```bash
//! cargo run --bin derive_cards            # сводка по всем группам
//! cargo run --bin derive_cards -- 1       # дерево конкретной группы
//! ```

use anyhow::{Context, Result};
use arm112::{config::Config, ingest::survey, store};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let pool = store::connect(&config.database_url).await?;

    let version_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY imported_at DESC LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?
    .context("в базе нет ни одной версии классификатора — сначала импортируйте XLSX")?;

    match std::env::args().nth(1) {
        Some(arg) => show_group(&pool, version_id, arg.parse()?).await,
        None => show_summary(&pool, version_id).await,
    }
}

async fn show_summary(pool: &sqlx::PgPool, version_id: Uuid) -> Result<()> {
    println!("{:<4} {:<46} {:>6} {:>7} {:>8}", "гр.", "название", "ветвей", "комб.", "скрытых");
    println!("{}", "─".repeat(76));

    let mut total_choices = 0;

    for group_no in 1..=24 {
        match survey::derive(pool, version_id, group_no).await? {
            Some(card) => {
                let amb = card.choice_nodes().len();
                total_choices += amb;
                println!(
                    "{:<4} {:<46} {:>6} {:>7} {:>8}{}",
                    group_no,
                    truncate(&card.group_title, 44),
                    card.branches.len(),
                    card.combinations(),
                    card.skipped_hidden,
                    if amb > 0 { format!("   с выбором: {amb}") } else { String::new() },
                );
            }
            None => println!("{group_no:<4} {:<46} {:>6}", "— не разворачивается в карту", "—"),
        }
    }

    println!("\nУзлов с выбором на третьем уровне: {total_choices}");
    println!("Это нормальные вопросы опросной карты вида «открытое пламя»");
    println!("или «дым» — не проблема, а структура.");
    Ok(())
}

async fn show_group(pool: &sqlx::PgPool, version_id: Uuid, group_no: i32) -> Result<()> {
    let Some(card) = survey::derive(pool, version_id, group_no).await? else {
        println!("Группа {group_no} не разворачивается в опросную карту");
        return Ok(());
    };

    println!("Группа {}: {}\n", card.group_no, card.group_title);

    for (s1, branch) in &card.branches {
        println!("  {s1}");
        for (s2, s3s) in &branch.objects {
            let name = if s2.is_empty() { "(без второго признака)" } else { s2 };
            if s3s.is_empty() {
                println!("      {name}");
            } else {
                println!("      {name} → {}", s3s.join(" · "));
            }
        }
        println!();
    }

    if card.skipped_hidden > 0 {
        println!("Пропущено позиций «Не отображается оператору 112»: {}", card.skipped_hidden);
    }

    let amb = card.choice_nodes();
    if !amb.is_empty() {
        println!("\nУзлы с выбором ({}):", amb.len());
        for (s1, s2, s3s) in amb.iter().take(10) {
            println!("  {s1} → {s2} → {}", s3s.join(" / "));
        }
    }

    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).collect::<String>() + "…" }
}
