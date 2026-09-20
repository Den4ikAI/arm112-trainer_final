//! Правка сценария в диалоге с моделью.
//!
//! Требование ТЗ: «в случае некорректного формирования вопроса или
//! ответа у Преподавателя имеется контекстное поле, куда он может
//! ввести комментарий, который должен быть отработан системой для
//! корректировки вопроса или ответа».
//!
//! Устроено как переписка, а не как одна кнопка: правка редко
//! получается с первого раза, и модели нужен контекст предыдущих
//! ходов — иначе на «нет, короче» она не поймёт, что укорачивать.
//!
//! Граница та же, что при генерации: **модель правит только текст.**
//! Признаки она может предложить поменять, но что из этого следует —
//! тип происшествия и список служб — пересчитывает классификатор.
//! Поэтому испортить оценку через диалог нельзя.

use crate::{domain::classify::classify, ml::Llm};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Сценарий целиком — то, что видит преподаватель и что уходит модели.
#[derive(Debug, Serialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioFull {
    pub id: Uuid,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    pub legend: String,
    pub applicant_name: String,
    pub applicant_phone: String,
    pub applicant_status: String,
    pub norm_sec: i32,
    pub status: String,
    pub source: String,
    pub expected: Value,
    pub classify_reason: Option<String>,
    pub hidden: Vec<HiddenRow>,
}

#[derive(Debug, Serialize, Deserialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HiddenRow {
    pub key: String,
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub unlocks: Vec<String>,
}

/// Что модель предлагает изменить. Все поля необязательны: на реплику
/// «а почему тут так?» правки нет вовсе.
#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub legend: Option<String>,
    pub applicant_name: Option<String>,
    pub applicant_status: Option<String>,
    pub norm_sec: Option<i32>,
    pub hidden: Option<Vec<HiddenRow>>,
    /// Признаки опросной карты. Принимаются, но последствия считает
    /// классификатор, а не модель
    pub answers: Option<std::collections::HashMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
struct ModelTurn {
    /// Что сказать преподавателю
    reply: String,
    #[serde(default)]
    patch: Option<Patch>,
}

pub async fn load(pool: &PgPool, id: Uuid) -> Result<ScenarioFull> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        Option<i32>, Option<i32>, String, String, String, String,
        i32, String, String, Value, Option<String>,
    )> = sqlx::query_as(
        "SELECT ticket, call_no, legend, applicant_name, applicant_phone,
                applicant_status, norm_sec, status::text, source, expected, classify_reason
           FROM scenarios WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some((ticket, call_no, legend, applicant_name, applicant_phone,
              applicant_status, norm_sec, status, source, expected, classify_reason)) = row
    else {
        anyhow::bail!("сценарий не найден");
    };

    let hidden: Vec<HiddenRow> = sqlx::query_as::<_, (String, String, String, Vec<String>)>(
        "SELECT key, question, answer, unlocks FROM scenario_hidden_facts
          WHERE scenario_id = $1 ORDER BY sort_order",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(key, question, answer, unlocks)| HiddenRow { key, question, answer, unlocks })
    .collect();

    Ok(ScenarioFull {
        id, ticket, call_no, legend, applicant_name, applicant_phone,
        applicant_status, norm_sec, status, source, expected, classify_reason, hidden,
    })
}

const SYSTEM: &str = "\
Ты помогаешь преподавателю учебного центра править учебные вызовы для \
тренажёра операторов Системы-112.

Вызов состоит из легенды (первая фраза заявителя) и скрытых фактов — \
того, что заявитель сообщает только в ответ на вопрос оператора.

Правь ровно то, о чём просят. Не переписывай остальное. Отвечай \
только JSON вида {\"reply\": \"что ты сделал, одной фразой\", \
\"patch\": {...}}. Если правка не нужна — patch не указывай. \
В patch клади только изменившиеся поля.";

/// Один ход переписки.
pub async fn turn(
    llm: &dyn Llm,
    pool: &PgPool,
    id: Uuid,
    comment: &str,
) -> Result<(String, Option<Patch>, Value)> {
    let scenario = load(pool, id).await?;

    // История нужна, чтобы «нет, короче» имело смысл. Берём последние
    // ходы: дальше контекст растёт быстрее, чем приносит пользы
    let history: Vec<(String, String)> = sqlx::query_as(
        "SELECT role::text, content FROM (
            SELECT role, content, at, id FROM scenario_chat
             WHERE scenario_id = $1 AND role <> 'system'
             ORDER BY at DESC, id DESC LIMIT 8
         ) t ORDER BY at, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let mut messages = vec![json!({ "role": "system", "content": SYSTEM })];
    messages.push(json!({
        "role": "user",
        "content": format!(
            "Вот текущий вызов:\n{}\n\nДальше — замечания преподавателя.",
            serde_json::to_string_pretty(&json!({
                "legend": scenario.legend,
                "hidden": scenario.hidden,
                "answers": scenario.expected.get("answers"),
            }))?)
    }));
    for (role, content) in history {
        let role = if role == "teacher" { "user" } else { "assistant" };
        messages.push(json!({ "role": role, "content": content }));
    }
    messages.push(json!({ "role": "user", "content": comment }));

    let raw = llm.chat_json(Value::Array(messages), 900).await?;
    let turn: ModelTurn = serde_json::from_value(raw.clone())
        .with_context(|| format!("модель вернула не тот JSON: {raw}"))?;

    Ok((turn.reply, turn.patch, raw))
}

/// Применить правку и пересчитать эталон.
///
/// Возвращает новое состояние сценария. Эталон не сохраняется из
/// патча: он всегда выводится классификатором из признаков.
pub async fn apply(
    pool: &PgPool,
    version_id: Uuid,
    id: Uuid,
    patch: &Patch,
) -> Result<ScenarioFull> {
    let current = load(pool, id).await?;

    sqlx::query(
        "UPDATE scenarios SET
            legend           = COALESCE($2, legend),
            applicant_name   = COALESCE($3, applicant_name),
            applicant_status = COALESCE($4, applicant_status),
            norm_sec         = COALESCE($5, norm_sec)
          WHERE id = $1",
    )
    .bind(id)
    .bind(patch.legend.as_ref())
    .bind(patch.applicant_name.as_ref())
    .bind(patch.applicant_status.as_ref())
    .bind(patch.norm_sec)
    .execute(pool)
    .await?;

    if let Some(hidden) = &patch.hidden {
        // Заменяем целиком: модель присылает новый список, а не дельту.
        // Сопоставлять по ключам надёжнее было бы, но модель их
        // переименовывает, и правка «убери второй вопрос» тогда не
        // работает
        sqlx::query("DELETE FROM scenario_hidden_facts WHERE scenario_id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        for (i, fact) in hidden.iter().enumerate() {
            sqlx::query(
                "INSERT INTO scenario_hidden_facts
                     (scenario_id, key, question, answer, unlocks, sort_order)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (scenario_id, key) DO UPDATE
                     SET question = EXCLUDED.question,
                         answer = EXCLUDED.answer,
                         unlocks = EXCLUDED.unlocks,
                         sort_order = EXCLUDED.sort_order",
            )
            .bind(id)
            .bind(&fact.key)
            .bind(&fact.question)
            .bind(&fact.answer)
            .bind(&fact.unlocks)
            .bind(i as i32 + 1)
            .execute(pool)
            .await?;
        }
    }

    // Эталон пересчитываем всегда: даже если правили только текст,
    // сценарий мог быть сохранён с неполным эталоном
    let answers = match &patch.answers {
        Some(a) => a.clone(),
        None => current
            .expected
            .get("answers")
            .map(crate::api::sessions::answers_to_map)
            .unwrap_or_default(),
    };
    let type_ids: Vec<String> = current
        .expected
        .get("typeIds")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let result = classify(pool, version_id, &type_ids, &answers).await?;
    let expected = json!({
        "typeIds": type_ids,
        "answers": answers,
        "address": current.expected.get("address").cloned().unwrap_or(json!({})),
        "services": result.services.iter().map(|s| s.service_id.clone()).collect::<Vec<_>>(),
        "outcome": result.outcome,
    });

    sqlx::query("UPDATE scenarios SET expected = $2, classify_reason = $3 WHERE id = $1")
        .bind(id)
        .bind(&expected)
        .bind(result.reason.as_ref())
        .execute(pool)
        .await?;

    load(pool, id).await
}
