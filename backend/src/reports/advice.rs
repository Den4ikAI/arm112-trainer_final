//! Рекомендации: выдача после разбора, проверка следующим разбором.
//!
//! Логика лежит в `domain::advice` и базы не знает. Здесь — всё, что
//! связывает её с данными: откуда история, когда рекомендация пишется
//! и когда ставится вердикт.
//!
//! Порядок — суть проверки. Сначала вердикт по открытым рекомендациям
//! (разбор, начатый после них, — возможность проверить), потом выдача
//! новых. Наоборот нельзя: рекомендация, выданная по этому разбору,
//! тут же им бы и «проверилась».

use crate::domain::advice::{self, Efficacy, Mark, Past, Problem};
use crate::reports::forecast::setting;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Разбор курсанта вместе с моментами, по которым решается порядок.
struct Graded {
    session_id: Uuid,
    started_at: DateTime<Utc>,
    graded_at: DateTime<Utc>,
    past: Past,
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Строка разбора: занятие, начало, время разбора, замечания, службы.
type GradedRow = (Uuid, DateTime<Utc>, DateTime<Utc>, Value, Value);

/// История разборов курсанта, от старых к новым.
async fn history(pool: &PgPool, user_id: Uuid) -> Result<Vec<Graded>> {
    let rows: Vec<GradedRow> = sqlx::query_as(
        "SELECT s.id, s.started_at, g.created_at, g.findings, g.services
           FROM grades g
           JOIN sessions s ON s.id = g.session_id
          WHERE s.user_id = $1
          ORDER BY g.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(session_id, started_at, graded_at, findings, services)| {
            let marks = findings
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|f| {
                            Some(Mark {
                                field: f.get("field")?.as_str()?.to_string(),
                                kind: f.get("kind")?.as_str()?.to_string(),
                                label: f.get("label").and_then(Value::as_str).unwrap_or("").to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Graded {
                session_id,
                started_at,
                graded_at,
                past: Past {
                    marks,
                    expected_services: strings(&services, "expected"),
                    actual_services: strings(&services, "actual"),
                },
            }
        })
        .collect())
}

async fn window(pool: &PgPool) -> usize {
    setting(pool, "advice.window", advice::DEFAULT_WINDOW as f64).await as usize
}

/// Вызывается один раз, после того как разбор занятия записан.
pub async fn after_grade(pool: &PgPool, session_id: Uuid) -> Result<()> {
    let user_id: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE id = $1")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    let window = window(pool).await;
    let hist = history(pool, user_id).await?;

    // 1. Вердикт по открытым: первый разбор, начатый после рекомендации,
    // где у ошибки была возможность повториться
    let open: Vec<(Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, field, kind, made_at FROM advice
          WHERE user_id = $1 AND checked_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    for (id, field, kind, made_at) in open {
        let after: Vec<&Graded> = hist.iter().filter(|g| g.started_at > made_at).collect();
        let pasts: Vec<Past> = after.iter().map(|g| g.past.clone()).collect();
        if let Some((i, worked)) = advice::check(&Problem::new(&field, &kind), &pasts) {
            // Условно по прежнему состоянию: вердикт ставится один раз
            sqlx::query(
                "UPDATE advice SET worked = $2, checked_at = now(), check_session_id = $3
                  WHERE id = $1 AND checked_at IS NULL",
            )
            .bind(id)
            .bind(worked)
            .bind(after[i].session_id)
            .execute(pool)
            .await?;
        }
    }

    // 2. Выдача: ошибки, повторившиеся в этом разборе
    let still_open: Vec<(String, String)> = sqlx::query_as(
        "SELECT field, kind FROM advice WHERE user_id = $1 AND checked_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    let open: Vec<Problem> = still_open.iter().map(|(f, k)| Problem::new(f, k)).collect();

    let pasts: Vec<Past> = hist.iter().map(|g| g.past.clone()).collect();
    for t in advice::to_issue(&pasts, window, &open) {
        sqlx::query(
            "INSERT INTO advice
                 (user_id, source_session_id, field, kind, label, text, occurrences, window_size)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (user_id, field, kind) WHERE checked_at IS NULL DO NOTHING",
        )
        .bind(user_id)
        .bind(session_id)
        .bind(&t.problem.field)
        .bind(&t.problem.kind)
        .bind(&t.label)
        .bind(advice::wording(&t.problem, &t.label))
        .bind(t.occurrences as i32)
        .bind(window as i32)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Рекомендация для экрана.
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AdviceView {
    pub id: Uuid,
    pub text: String,
    pub field: String,
    pub kind: String,
    pub label: String,
    pub occurrences: i32,
    pub window_size: i32,
    pub made_at: DateTime<Utc>,
    /// Разбор, после которого выдана
    pub source_session_id: Uuid,
    /// Разбор, которым проверена; `null` — ещё не проверена
    pub check_session_id: Option<Uuid>,
    pub worked: Option<bool>,
    pub checked_at: Option<DateTime<Utc>>,
}

/// Свои рекомендации курсанта, свежие первыми.
pub async fn mine(pool: &PgPool, user_id: Uuid) -> Result<Vec<AdviceView>> {
    Ok(sqlx::query_as(
        "SELECT id, text, field, kind, label, occurrences, window_size, made_at,
                source_session_id, check_session_id, worked, checked_at
           FROM advice WHERE user_id = $1
          ORDER BY made_at DESC LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Проверенная рекомендация — для ленты преподавателя.
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CheckedAdvice {
    pub fio: String,
    pub text: String,
    pub worked: bool,
    pub made_at: DateTime<Utc>,
    pub checked_at: DateTime<Utc>,
}

/// Насколько рекомендации работают в группе.
///
/// Доля — по фактически проверенным рекомендациям. Рядом базовая
/// линия: те же повторяющиеся ошибки у тех же курсантов, когда
/// рекомендации на них открыто не было, и как часто они уходили сами.
pub async fn group_efficacy(pool: &PgPool, group_id: Uuid) -> Result<(Efficacy, Vec<CheckedAdvice>)> {
    let window = window(pool).await;
    let members: Vec<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(pool)
            .await?;

    let mut checked = 0usize;
    let mut worked = 0usize;
    let mut base_checked = 0usize;
    let mut base_worked = 0usize;

    for user_id in members {
        #[allow(clippy::type_complexity)]
        let issued: Vec<(String, String, DateTime<Utc>, Option<DateTime<Utc>>, Option<bool>)> =
            sqlx::query_as(
                "SELECT field, kind, made_at, checked_at, worked FROM advice WHERE user_id = $1",
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?;

        for (_, _, _, _, w) in &issued {
            if let Some(w) = w {
                checked += 1;
                worked += *w as usize;
            }
        }

        let hist = history(pool, user_id).await?;
        let pasts: Vec<Past> = hist.iter().map(|g| g.past.clone()).collect();
        // Случай «под рекомендацией»: на эту ошибку рекомендация была
        // открыта к моменту разбора — такой случай в базовую линию
        // не идёт, он уже учтён в доле рекомендаций
        let covered = |i: usize, p: &Problem| {
            let at = hist[i].graded_at;
            issued.iter().any(|(f, k, made, done, _)| {
                f == &p.field && k == &p.kind && *made <= at && done.is_none_or(|d| d > at)
            })
        };
        let (c, w) = advice::baseline(&pasts, window, covered);
        base_checked += c;
        base_worked += w;
    }

    let recent: Vec<CheckedAdvice> = sqlx::query_as(
        "SELECT u.fio, a.text, a.worked, a.made_at, a.checked_at
           FROM advice a
           JOIN users u ON u.id = a.user_id
           JOIN group_members m ON m.user_id = a.user_id
          WHERE m.group_id = $1 AND a.checked_at IS NOT NULL
          ORDER BY a.checked_at DESC
          LIMIT 30",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    Ok((advice::efficacy(checked, worked, base_checked, base_worked), recent))
}
