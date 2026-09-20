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
use anyhow::Context;
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
        "scenario.generate" => generate_scenarios(state, &job.input).await,
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

    // Модели отдаём выжимку, а не всю статистику. В полной лежат
    // двести точек графика и тепловая карта — это двадцать тысяч
    // токенов, которые не влезают в контекст и ничего не добавляют:
    // для сводки нужны агрегаты, а не сырые занятия
    // Ключи по-русски и развёрнуто — не из вежливости. На коротком
    // `guessedShare: 0.449` модель написала «из них 44,9% оказались
    // верными», хотя это доля угаданного среди совпавшего. Число,
    // которое нельзя прочитать двояко, соврать труднее
    let digest = json!({
        "участников в группе": stats.members,
        "занятий с оценкой": stats.sessions,
        "средний балл (из 100)": stats.avg_score.map(|v| v.round()),
        "среднее время заполнения карточки, секунд": stats.avg_elapsed_sec.map(|v| v.round()),
        "норматив заполнения, секунд": stats.avg_norm_sec.map(|v| v.round()),
        "занятий с превышением норматива": stats.over_norm,
        "всего значений совпало с эталоном": stats.guessed + stats.ok,
        "из них выяснено у заявителя": stats.ok,
        "из них угадано без вопроса": stats.guessed,
        "доля угаданного среди совпавшего, процентов":
            (stats.guessed_share * 100.0).round(),
        // Десяти самых частых замечаний хватает: остальные в сводку
        // всё равно не попадут
        "частые замечания": stats.top_problems.iter().take(10).map(|p| json!({
            "поле": p.label,
            "вид": match p.kind.as_str() {
                "guessed" => "угадал, а не выяснил",
                "wrong" => "ошибка в значении",
                "missing" => "не заполнено",
                "extra" => "лишняя служба",
                other => other,
            },
            "сколько раз": p.count,
        })).collect::<Vec<_>>(),
    });

    let text = ml.llm.group_insights(digest).await?;
    Ok(json!({ "stats": stats, "text": text }))
}

/// Подготовка учебных вызовов из билетов.
///
/// Долгая задача: один вызов — это три обращения к модели и около
/// минуты на процессоре. Поэтому пакетом и в очереди.
///
/// Каждый вызов сохраняется сразу, а не в конце: если задача упадёт
/// на сороковом билете, тридцать девять уже готовых останутся.
async fn generate_scenarios(state: &AppState, input: &Value) -> anyhow::Result<Value> {
    use crate::{domain::scenario_gen, ingest::tickets, ml::http::HttpLlm};

    let wanted: Vec<i32> = input
        .get("tickets")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_i64().map(|n| n as i32)).collect())
        .unwrap_or_default();
    let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

    let path = std::env::var("TICKETS_FILE")
        .unwrap_or_else(|_| "../extracted/bilety_96_scenarios.md".into());
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("не найден файл билетов {path}"))?;
    let calls = tickets::parse(&text)?;

    let llm = HttpLlm::new(
        std::env::var("ML_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8090".into()),
        std::env::var("ML_MODEL").unwrap_or_else(|_| "local".into()),
    )?;

    let version_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    )
    .fetch_one(&state.db)
    .await?;

    let selected: Vec<_> = calls
        .iter()
        .filter(|c| wanted.is_empty() || wanted.contains(&c.ticket))
        .take(count)
        .collect();

    let mut made = Vec::new();
    let mut failed = Vec::new();

    for call in selected {
        // Дважды один билет не заводим: задачу можно запустить повторно
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM scenarios
                             WHERE ticket = $1 AND call_no = $2 AND status <> 'archived')",
        )
        .bind(call.ticket)
        .bind(call.call_no)
        .fetch_one(&state.db)
        .await?;
        if exists {
            continue;
        }

        match scenario_gen::generate(&llm, &state.db, version_id, call).await {
            Ok(g) => {
                let id = save_generated(&state.db, &g).await?;
                made.push(json!({
                    "id": id, "ticket": g.ticket, "callNo": g.call_no,
                    "outcome": g.expected.get("outcome"), "reason": g.reason,
                }));
            }
            Err(e) => {
                tracing::error!(ticket = call.ticket, call = call.call_no, error = %e,
                                "вызов не подготовлен");
                failed.push(json!({
                    "ticket": call.ticket, "callNo": call.call_no, "error": e.to_string(),
                }));
            }
        }
    }

    Ok(json!({ "created": made, "failed": failed }))
}

/// Сохранить подготовленный вызов черновиком.
async fn save_generated(
    pool: &PgPool,
    g: &crate::domain::scenario_gen::Generated,
) -> anyhow::Result<Uuid> {
    let mut tx = pool.begin().await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO scenarios
             (ticket, call_no, legend, applicant_name, applicant_phone,
              applicant_status, expected, status, source, source_ref, classify_reason)
         VALUES ($1, $2, $3, $4, $5, 'очевидец', $6, 'draft', 'generated', $7, $8)
         RETURNING id",
    )
    .bind(g.ticket)
    .bind(g.call_no)
    .bind(&g.legend)
    .bind(&g.applicant_name)
    .bind(&g.applicant_phone)
    .bind(&g.expected)
    .bind(format!("билет {} вызов {}", g.ticket, g.call_no))
    .bind(g.reason.as_ref())
    .fetch_one(&mut *tx)
    .await?;

    for (i, fact) in g.hidden.iter().enumerate() {
        sqlx::query(
            "INSERT INTO scenario_hidden_facts
                 (scenario_id, key, question, answer, unlocks, sort_order)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (scenario_id, key) DO NOTHING",
        )
        .bind(id)
        .bind(&fact.key)
        .bind(&fact.question)
        .bind(&fact.answer)
        .bind(&fact.unlocks)
        .bind(i as i32 + 1)
        .execute(&mut *tx)
        .await?;
    }

    // Первая запись в переписке — как сценарий появился
    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content)
         VALUES ($1, 'system', $2)",
    )
    .bind(id)
    .bind(format!(
        "Вызов подготовлен из билета {} (вызов {}). Признаки выбраны моделью \
         из опросной карты, эталон посчитан классификатором.{}",
        g.ticket, g.call_no,
        g.reason.as_ref().map(|r| format!(" Эталон неполон: {r}")).unwrap_or_default()
    ))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(id)
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
