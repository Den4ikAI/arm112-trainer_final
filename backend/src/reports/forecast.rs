//! Прогноз: чтение истории, запись прогноза, проверка фактом.
//!
//! Математика лежит в `domain::forecast` и базы не знает. Здесь —
//! всё, что связывает её с данными: откуда берётся история, когда
//! пишется прогноз и когда проставляется факт.
//!
//! Порядок важен и является сутью проверки. Прогноз пишется при
//! старте вызова, факт — после разбора, и ни то ни другое не
//! переписывается. Сравнение имеет смысл только при этом условии.

use crate::domain::forecast::{self, Accuracy, Features, Model, Past};
use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// Числовая настройка из таблицы `settings`.
pub async fn setting(pool: &PgPool, key: &str, по_умолчанию: f64) -> f64 {
    sqlx::query_scalar::<_, serde_json::Value>("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_f64())
        .unwrap_or(по_умолчанию)
}

/// История разборов курсанта, от старых к новым.
///
/// Группу классификатора берём у вызова: балл на знакомой теме —
/// один из признаков.
pub async fn history(pool: &PgPool, user_id: Uuid, до: Option<Uuid>) -> Result<Vec<Past>> {
    let rows: Vec<(i32, serde_json::Value, bool, Option<i32>)> = sqlx::query_as(
        "SELECT g.score, g.findings, g.over_norm,
                (SELECT t.group_no FROM incident_types t
                  WHERE t.id = (sc.expected->'typeIds'->>0)) AS group_no
           FROM grades g
           JOIN sessions s ON s.id = g.session_id
           LEFT JOIN scenarios sc ON sc.id = s.scenario_id
          WHERE s.user_id = $1
            AND ($2::uuid IS NULL OR g.created_at < (
                    SELECT started_at FROM sessions WHERE id = $2))
          ORDER BY g.created_at",
    )
    .bind(user_id)
    .bind(до)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(score, findings, over_norm, group_no)| {
            let (learned, guessed) = счёт(&findings);
            Past { score, learned, guessed, over_norm, group_no }
        })
        .collect())
}

/// Сколько значений выяснено и сколько угадано — из списка замечаний.
fn счёт(findings: &serde_json::Value) -> (i32, i32) {
    let mut learned = 0;
    let mut guessed = 0;
    if let Some(list) = findings.as_array() {
        for f in list {
            match f.get("kind").and_then(|k| k.as_str()) {
                Some("ok") => learned += 1,
                Some("guessed") => guessed += 1,
                _ => {}
            }
        }
    }
    (learned, guessed)
}

/// Обучить модель на всём, что накопилось.
///
/// Пары строятся строго по времени: признаки берутся из разборов,
/// бывших до целевого. Иначе модель подглядывала бы в будущее и
/// показывала точность, которой в работе не будет.
pub async fn train(pool: &PgPool) -> Result<Model> {
    let lambda = setting(pool, "forecast.lambda", 0.1).await;

    let пользователи: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT s.user_id FROM grades g JOIN sessions s ON s.id = g.session_id",
    )
    .fetch_all(pool)
    .await?;

    let mut пары: Vec<(Features, i32)> = Vec::new();
    for user_id in пользователи {
        let история = history(pool, user_id, None).await?;
        for конец in forecast::MIN_HISTORY..история.len() {
            let цель = &история[конец];
            if let Some(f) = forecast::features(&история[..конец], цель.group_no) {
                пары.push((f, цель.score));
            }
        }
    }

    Ok(forecast::fit(&пары, lambda))
}

/// Зафиксировать прогноз на вызов, который курсант только что взял.
///
/// Ничего не делает, если истории мало: прогноз по одному занятию —
/// это не прогноз, и показывать его как прогноз нечестно.
pub async fn record(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    group_no: Option<i32>,
) -> Result<Option<i32>> {
    let история = history(pool, user_id, Some(session_id)).await?;
    let Some(f) = forecast::features(&история, group_no) else {
        return Ok(None);
    };

    let модель = train(pool).await?;
    let прогноз = модель.predict(&f);

    sqlx::query(
        "INSERT INTO predictions (user_id, session_id, predicted, features, basis, trained)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (session_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(прогноз)
    .bind(json!({
        "features": f,
        "contributions": модель.contributions(&f)
            .into_iter()
            .map(|(имя, вклад)| json!({ "label": имя, "points": вклад }))
            .collect::<Vec<_>>(),
        "intercept": (модель.intercept * 100.0).round() as i32,
    }))
    .bind(история.len() as i32)
    .bind(модель.trained())
    .execute(pool)
    .await?;

    Ok(Some(прогноз))
}

/// Проставить факт. Вызывается один раз, после разбора.
pub async fn check(pool: &PgPool, session_id: Uuid, actual: i32) -> Result<()> {
    sqlx::query(
        "UPDATE predictions SET actual = $2, checked_at = now()
          WHERE session_id = $1 AND checked_at IS NULL",
    )
    .bind(session_id)
    .bind(actual)
    .execute(pool)
    .await?;
    Ok(())
}

/// Один проверенный прогноз — для ленты «предсказали / получил».
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    pub session_id: Uuid,
    pub fio: String,
    pub predicted: i32,
    pub actual: i32,
    pub basis: i32,
    pub trained: bool,
    pub made_at: chrono::DateTime<chrono::Utc>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

/// Достоверность прогнозов по группе.
///
/// Считается по фактически сделанным прогнозам, а не по отложенной
/// выборке: заявлять точность, полученную где-то ещё, — это ровно
/// то, что критерий просит проверить.
pub async fn group_accuracy(pool: &PgPool, group_id: Uuid) -> Result<(Accuracy, Vec<Checked>)> {
    let допуск = setting(pool, "forecast.tolerance", 10.0).await as i32;

    // Базовая линия считается тем же запросом: средний балл курсанта
    // на момент, когда прогноз был сделан. Раньше она бралась
    // подзапросом на каждую строку — до двухсот запросов подряд
    type Строка = (Uuid, String, i32, i32, i32, bool,
                   chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>,
                   Option<f64>);
    let rows: Vec<Строка> = sqlx::query_as(
        "SELECT p.session_id, u.fio, p.predicted, p.actual, p.basis, p.trained,
                p.made_at, p.checked_at,
                (SELECT avg(g.score)::float8
                   FROM grades g JOIN sessions s ON s.id = g.session_id
                  WHERE s.user_id = p.user_id AND g.created_at < p.made_at) AS baseline
           FROM predictions p
           JOIN users u ON u.id = p.user_id
           JOIN group_members m ON m.user_id = p.user_id
          WHERE m.group_id = $1 AND p.checked_at IS NOT NULL
          ORDER BY p.checked_at DESC
          LIMIT 200",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    let mut пары = Vec::new();
    let mut базовая = Vec::new();
    let mut checked = Vec::new();
    for (session_id, fio, predicted, actual, basis, trained, made_at, checked_at, средний)
        in rows
    {
        пары.push((predicted, actual));
        // Без истории базовой линии нет — берём сам факт, чтобы она
        // не выглядела хуже, чем есть
        базовая.push(средний.unwrap_or(actual as f64).round() as i32);
        checked.push(Checked {
            session_id, fio, predicted, actual, basis, trained, made_at, checked_at,
        });
    }
    let rows = checked;

    Ok((forecast::accuracy(&пары, &базовая, допуск), rows))
}
