//! Рекомендации диспетчеру ДДС: выдача после закрытой карточки,
//! проверка следующей.
//!
//! Логика лежит в `domain::dds_advice` и базы не знает. Здесь —
//! откуда история (попытка ДДС = назначение бригады курсантом по
//! карточке, как в `api::grading::attempts_dds`), когда рекомендация
//! пишется и когда ставится вердикт.
//!
//! В историю идут только карточки, по которым служба закончила работу
//! («Работы завершены» или «Отказ»): пока работа идёт, разбор неполный,
//! и рекомендация по нему была бы преждевременной.
//!
//! Порядок — суть проверки: сначала вердикт по открытым, потом выдача
//! новых. Наоборот нельзя: рекомендация, выданная по этой карточке,
//! тут же ею бы и «проверилась».

use crate::api::brigade;
use crate::domain::advice::Efficacy;
use crate::domain::brigade::DdsFindingKind;
use crate::domain::dds_advice::{self, Norms, Past};
use crate::reports::advice::CheckedAdvice;
use crate::reports::forecast::setting;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

struct Graded {
    card_id: Uuid,
    /// Начало попытки — назначение бригады
    started_at: DateTime<Utc>,
    past: Past,
}

fn kind_str(k: DdsFindingKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn kind_of(s: &str) -> Option<DdsFindingKind> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// Закрытые попытки курсанта, от старых к новым.
async fn history(pool: &PgPool, user_id: Uuid) -> Result<Vec<Graded>> {
    let rows: Vec<(Uuid, String, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT ba.card_id, ba.service_id, ba.assigned_at,
                EXISTS (SELECT 1 FROM lesson_cards lc JOIN lessons l ON l.id = lc.lesson_id
                         WHERE lc.card_id = ba.card_id AND l.segment = $2)
           FROM brigade_assignments ba
          WHERE ba.user_id = $1
            AND EXISTS (SELECT 1 FROM response_entries r
                         WHERE r.card_id = ba.card_id AND r.service_id = ba.service_id
                           AND r.status::text IN ('Работы завершены', 'Отказ от выполнения работ'))
          ORDER BY ba.assigned_at
          LIMIT 100",
    )
    .bind(user_id)
    .bind(crate::domain::drill::SEGMENT_DDS)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for (card_id, service_id, started_at, blitz) in rows {
        let view = brigade::load(pool, card_id, &service_id, true).await?;
        let kinds: Vec<DdsFindingKind> = view.debrief.findings.iter().map(|f| f.kind).collect();
        let stages = view.debrief.findings.iter().map(|f| (f.kind, f.stage.clone())).collect();
        out.push(Graded {
            card_id,
            started_at,
            past: Past { kinds, stages, intake_graded: !blitz },
        });
    }
    Ok(out)
}

async fn norms(pool: &PgPool) -> Norms {
    Norms {
        update_sec: setting(pool, "brigade.update_norm_sec", 30.0).await as i64,
        open_sec: setting(pool, "norm.ack_sec", 30.0).await as i64,
        first_record_sec: setting(pool, "norm.first_record_sec", 180.0).await as i64,
    }
}

async fn window(pool: &PgPool) -> usize {
    setting(pool, "advice.window", dds_advice::DEFAULT_WINDOW as f64).await as usize
}

/// Вызывается после того, как служба закрыла карточку, — и перед выдачей
/// списка курсанту: повторный вызов ничего не меняет.
pub async fn after_close(pool: &PgPool, user_id: Uuid) -> Result<()> {
    let hist = history(pool, user_id).await?;
    let window = window(pool).await;

    // 1. Вердикт по открытым: первая карточка, начатая после рекомендации,
    // где у ошибки была возможность повториться
    let open: Vec<(Uuid, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, kind, made_at FROM dds_advice WHERE user_id = $1 AND checked_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    for (id, kind, made_at) in open {
        let Some(kind) = kind_of(&kind) else { continue };
        let after: Vec<&Graded> = hist.iter().filter(|g| g.started_at > made_at).collect();
        let pasts: Vec<Past> = after.iter().map(|g| g.past.clone()).collect();
        if let Some((i, worked)) = dds_advice::check(kind, &pasts) {
            // Условно по прежнему состоянию: вердикт ставится один раз
            sqlx::query(
                "UPDATE dds_advice SET worked = $2, checked_at = now(), check_card_id = $3
                  WHERE id = $1 AND checked_at IS NULL",
            )
            .bind(id)
            .bind(worked)
            .bind(after[i].card_id)
            .execute(pool)
            .await?;
        }
    }

    // 2. Выдача: ошибки последней закрытой карточки
    let Some(last) = hist.last() else { return Ok(()) };
    let still_open: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM dds_advice WHERE user_id = $1 AND checked_at IS NULL")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    let open: Vec<DdsFindingKind> = still_open.iter().filter_map(|k| kind_of(k)).collect();
    let service_id: String =
        sqlx::query_scalar("SELECT service_id FROM brigade_assignments WHERE card_id = $1 AND user_id = $2")
            .bind(last.card_id)
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    let n = norms(pool).await;
    let pasts: Vec<Past> = hist.iter().map(|g| g.past.clone()).collect();
    for t in dds_advice::to_issue(&pasts, window, &open) {
        sqlx::query(
            "INSERT INTO dds_advice
                 (user_id, service_id, source_card_id, kind, stage, label, text, occurrences, window_size)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (user_id, kind) WHERE checked_at IS NULL DO NOTHING",
        )
        .bind(user_id)
        .bind(&service_id)
        .bind(last.card_id)
        .bind(kind_str(t.kind))
        .bind(&t.stage)
        .bind(dds_advice::label(t.kind))
        .bind(dds_advice::wording(t.kind, &n))
        .bind(t.occurrences as i32)
        .bind(window as i32)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Рекомендация диспетчеру для экрана.
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DdsAdviceView {
    pub id: Uuid,
    pub text: String,
    pub kind: String,
    pub stage: String,
    pub label: String,
    pub occurrences: i32,
    pub window_size: i32,
    pub made_at: DateTime<Utc>,
    /// Карточка, по разбору которой выдана
    pub source_card_id: Uuid,
    /// Карточка, которой проверена; `null` — ещё не проверена
    pub check_card_id: Option<Uuid>,
    pub worked: Option<bool>,
    pub checked_at: Option<DateTime<Utc>>,
    /// Задание от ИИ, выданное для отработки этой рекомендации
    pub next_card_id: Option<Uuid>,
}

/// Свои рекомендации курсанта, свежие первыми.
pub async fn mine(pool: &PgPool, user_id: Uuid) -> Result<Vec<DdsAdviceView>> {
    Ok(sqlx::query_as(
        "SELECT id, text, kind, stage, label, occurrences, window_size, made_at,
                source_card_id, check_card_id, worked, checked_at, next_card_id
           FROM dds_advice WHERE user_id = $1
          ORDER BY made_at DESC LIMIT 50",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

/// Отметить, что для рекомендации выдано задание от ИИ.
/// `false` — рекомендация не этого курсанта или уже проверена.
pub async fn mark_next(pool: &PgPool, advice_id: Uuid, user_id: Uuid, card_id: Uuid) -> Result<bool> {
    let rows = sqlx::query(
        "UPDATE dds_advice SET next_card_id = $3
          WHERE id = $1 AND user_id = $2 AND checked_at IS NULL",
    )
    .bind(advice_id)
    .bind(user_id)
    .bind(card_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

/// Насколько рекомендации ДДС работают в группе; рядом базовая линия.
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
        let issued: Vec<(String, DateTime<Utc>, Option<DateTime<Utc>>, Option<bool>)> = sqlx::query_as(
            "SELECT kind, made_at, checked_at, worked FROM dds_advice WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        if issued.is_empty() {
            continue;
        }
        for (_, _, _, w) in &issued {
            if let Some(w) = w {
                checked += 1;
                worked += *w as usize;
            }
        }

        let hist = history(pool, user_id).await?;
        let pasts: Vec<Past> = hist.iter().map(|g| g.past.clone()).collect();
        // Закрытие карточки — момент разбора: рекомендация, открытая к
        // нему, выводит случай из базовой линии
        let covered = |i: usize, k: DdsFindingKind| {
            let at = hist[i].started_at;
            let k = kind_str(k);
            issued.iter().any(|(kk, made, done, _)| {
                *kk == k && *made <= at && done.is_none_or(|d| d > at)
            })
        };
        let (c, w) = dds_advice::baseline(&pasts, window, covered);
        base_checked += c;
        base_worked += w;
    }

    let recent: Vec<CheckedAdvice> = sqlx::query_as(
        "SELECT u.fio, a.text, a.worked, a.made_at, a.checked_at
           FROM dds_advice a
           JOIN users u ON u.id = a.user_id
           JOIN group_members m ON m.user_id = a.user_id
          WHERE m.group_id = $1 AND a.checked_at IS NOT NULL
          ORDER BY a.checked_at DESC
          LIMIT 30",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    Ok((dds_advice::summary(checked, worked, base_checked, base_worked), recent))
}
