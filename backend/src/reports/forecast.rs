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
          ORDER BY g.created_at, g.id",
    )
    .bind(user_id)
    .bind(до)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(score, findings, over_norm, group_no)| past(score, &findings, over_norm, group_no))
        .collect())
}

fn past(score: i32, findings: &serde_json::Value, over_norm: bool, group_no: Option<i32>) -> Past {
    let (learned, guessed) = счёт(findings);
    Past { score, learned, guessed, over_norm, group_no }
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

/// Пары «признаки → балл» по всей истории всех курсантов.
///
/// Пары строятся строго по времени: признаки берутся из разборов,
/// бывших до целевого. Иначе модель подглядывала бы в будущее и
/// показывала точность, которой в работе не будет.
///
/// Вся история читается одним запросом. Раньше — отдельным запросом
/// на каждого курсанта, и при сотне курсантов начало вызова под
/// нагрузкой занимало до двух с половиной секунд
/// (`backend/docs/замеры/НАГРУЗКА.md`).
pub async fn pairs(pool: &PgPool) -> Result<Vec<(Features, i32)>> {
    let rows: Vec<(Uuid, i32, serde_json::Value, bool, Option<i32>)> = sqlx::query_as(
        "SELECT s.user_id, g.score, g.findings, g.over_norm,
                (SELECT t.group_no FROM incident_types t
                  WHERE t.id = (sc.expected->'typeIds'->>0)) AS group_no
           FROM grades g
           JOIN sessions s ON s.id = g.session_id
           LEFT JOIN scenarios sc ON sc.id = s.scenario_id
          ORDER BY s.user_id, g.created_at, g.id",
    )
    .fetch_all(pool)
    .await?;

    Ok(build_pairs(rows.into_iter().map(|(user_id, score, findings, over_norm, group_no)| {
        (user_id, past(score, &findings, over_norm, group_no))
    })))
}

/// Пары из разборов, упорядоченных по курсанту, затем по времени.
fn build_pairs(rows: impl Iterator<Item = (Uuid, Past)>) -> Vec<(Features, i32)> {
    let mut пары: Vec<(Features, i32)> = Vec::new();
    let mut история: Vec<Past> = Vec::new();
    let mut чья: Option<Uuid> = None;
    for (user_id, цель) in rows {
        if чья != Some(user_id) {
            история.clear();
            чья = Some(user_id);
        }
        if история.len() >= forecast::MIN_HISTORY {
            if let Some(f) = forecast::features(&история, цель.group_no) {
                пары.push((f, цель.score));
            }
        }
        история.push(цель);
    }
    пары
}

/// Обучить модель на всём, что накопилось.
pub async fn train(pool: &PgPool) -> Result<Model> {
    let lambda = setting(pool, "forecast.lambda", 0.1).await;
    Ok(forecast::fit(&pairs(pool).await?, lambda))
}

/// Сколько живёт обученная модель, прежде чем её переобучат.
///
/// Прогноз пишется при каждом начале вызова, а обучение проходит по
/// всей истории всех курсантов — делать его на каждый вызов значит
/// замедлять начало вызова с ростом базы. Минута запаздывания модели
/// на прогноз не влияет: за минуту история меняется на единицы
/// разборов из тысяч.
const MODEL_TTL: std::time::Duration = std::time::Duration::from_secs(60);

struct Cached {
    at: std::time::Instant,
    lambda: f64,
    model: Model,
}

static CACHE: std::sync::Mutex<Option<Cached>> = std::sync::Mutex::new(None);
/// Первое обучение после запуска: одно на всех, кто пришёл разом
static COLD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Фоновое переобучение уже идёт — второе не запускаем
static REFRESHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Модель из кэша и признак, что её пора переобучить.
fn cached(lambda: f64) -> Option<(Model, bool)> {
    let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.as_ref()
        .filter(|c| c.lambda == lambda)
        .map(|c| (c.model.clone(), c.at.elapsed() >= MODEL_TTL))
}

async fn refresh(pool: &PgPool, lambda: f64) -> Result<Model> {
    let model = forecast::fit(&pairs(pool).await?, lambda);
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(Cached { at: std::time::Instant::now(), lambda, model: model.clone() });
    Ok(model)
}

/// Модель для прогноза.
///
/// Устаревшая модель отдаётся сразу, а переобучение идёт в фоне:
/// иначе вызов, которому выпало оказаться первым после истечения
/// минуты, ждал бы обучения на всей истории, а с ним — все, кто
/// начал вызов в ту же секунду. На замере со ста пользователями это
/// давало до двух секунд на начало вызова. Ждут обучения только
/// самые первые вызовы после запуска сервера или смены параметра.
async fn model(pool: &PgPool) -> Result<Model> {
    use std::sync::atomic::Ordering;

    let lambda = setting(pool, "forecast.lambda", 0.1).await;
    if let Some((model, stale)) = cached(lambda) {
        if stale && !REFRESHING.swap(true, Ordering::SeqCst) {
            let pool = pool.clone();
            tokio::spawn(async move {
                if let Err(e) = refresh(&pool, lambda).await {
                    tracing::warn!(error = %e, "модель прогноза не переобучилась");
                }
                REFRESHING.store(false, Ordering::SeqCst);
            });
        }
        return Ok(model);
    }

    let _first = COLD.lock().await;
    if let Some((model, _)) = cached(lambda) {
        return Ok(model);
    }
    refresh(pool, lambda).await
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

    let модель = model(pool).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn разбор(score: i32, learned: i32, group_no: Option<i32>) -> Past {
        Past { score, learned, guessed: 1, over_norm: score < 60, group_no }
    }

    /// Пары из общего запроса — те же, что прежним способом: по
    /// истории каждого курсанта отдельно. Иначе ускорение начала
    /// вызова поменяло бы сам прогноз.
    #[test]
    fn пары_одним_проходом_совпадают_с_парами_по_курсантам() {
        let первый = Uuid::from_u128(1);
        let второй = Uuid::from_u128(2);
        let история_а: Vec<Past> = (0..7).map(|i| разбор(50 + i * 5, i, Some(1 + i % 2))).collect();
        let история_б: Vec<Past> = (0..4).map(|i| разбор(90 - i * 10, 2, Some(3))).collect();

        let mut ожидаемые = Vec::new();
        for история in [&история_а, &история_б] {
            for конец in forecast::MIN_HISTORY..история.len() {
                let цель = &история[конец];
                if let Some(f) = forecast::features(&история[..конец], цель.group_no) {
                    ожидаемые.push((f, цель.score));
                }
            }
        }

        let строки = история_а.iter().cloned().map(|p| (первый, p))
            .chain(история_б.iter().cloned().map(|p| (второй, p)));
        let получено = build_pairs(строки);

        assert!(!ожидаемые.is_empty());
        assert_eq!(serde_json::to_value(&получено).unwrap(),
                   serde_json::to_value(&ожидаемые).unwrap());
    }
}
