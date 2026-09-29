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
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Сценарий целиком — то, что видит преподаватель и что уходит модели.
#[derive(Debug, Serialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioFull {
    pub id: Uuid,
    /// Тема вызова, как её назвал преподаватель. У подготовленных из
    /// билетов её нет — там подписью служит номер билета
    pub title: Option<String>,
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
    /// Первая фраза заявителя утверждениями — по одному на сведение.
    /// `None` — ещё не подготовлены (domain/fact_statements.rs)
    pub legend_statements: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HiddenRow {
    pub key: String,
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub unlocks: Vec<String>,
    /// Ответ заявителя утверждением — для сверки описания курсанта
    /// кросс-энкодером. Готовит модель, правит преподаватель; модель
    /// в правке вызова это поле не присылает и не видит
    #[serde(default, skip_deserializing)]
    pub statement: Option<String>,
}

/// Что модель предлагает изменить. Все поля необязательны: на реплику
/// «а почему тут так?» правки нет вовсе.
#[derive(Debug, Deserialize, Default, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    /// Тема вызова. Модель её не присылает — правит преподаватель
    #[serde(default)]
    pub title: Option<String>,
    /// Типы происшествия эталона. Модель их не меняет: смена типа меняет
    /// всю опросную карту, это решение преподавателя
    #[serde(default)]
    pub type_ids: Option<Vec<String>>,
    /// Адрес, который оператор должен выяснить: те же поля, что в карточке
    #[serde(default)]
    pub address: Option<Value>,
    pub legend: Option<String>,
    pub applicant_name: Option<String>,
    pub applicant_status: Option<String>,
    pub norm_sec: Option<i32>,
    pub hidden: Option<Vec<HiddenRow>>,
    /// Признаки опросной карты. Принимаются, но последствия считает
    /// классификатор, а не модель
    pub answers: Option<std::collections::HashMap<String, Vec<String>>>,
}

/// Разбор ответа модели.
///
/// Нарочно снисходительный. Модель на 10 миллиардов параметров
/// регулярно кладёт ФИО в `hidden` или заворачивает факт в строку —
/// и это не повод отвечать преподавателю «internal». Реплику
/// показываем всегда, из патча берём то, что разобралось, остальное
/// называем вслух.
fn parse_turn(raw: &Value) -> (String, Option<Patch>, Vec<String>) {
    let reply = raw
        .get("reply")
        .and_then(|v| v.as_str())
        .unwrap_or("Модель ответила не по форме.")
        .to_string();

    let Some(obj) = raw.get("patch").and_then(|v| v.as_object()) else {
        return (reply, None, Vec::new());
    };

    let mut patch = Patch::default();
    let mut ignored = Vec::new();

    let text = |v: &Value| v.as_str().map(str::to_string);
    patch.legend = obj.get("legend").and_then(text);
    patch.applicant_name = obj.get("applicantName").and_then(text);
    patch.applicant_status = obj.get("applicantStatus").and_then(text);
    patch.norm_sec = obj.get("normSec").and_then(|v| v.as_i64()).map(|n| n as i32);

    if let Some(answers) = obj.get("answers").and_then(|v| v.as_object()) {
        let mut map = std::collections::HashMap::new();
        for (k, v) in answers {
            let values: Vec<String> = match v {
                Value::Array(a) => a.iter().filter_map(text).collect(),
                Value::String(one) => vec![one.clone()],
                _ => continue,
            };
            if !values.is_empty() {
                map.insert(k.clone(), values);
            }
        }
        if !map.is_empty() {
            patch.answers = Some(map);
        }
    }

    if let Some(list) = obj.get("hidden").and_then(|v| v.as_array()) {
        let mut facts = Vec::new();
        for item in list {
            // Иногда факт приезжает строкой с JSON внутри
            let item = match item {
                Value::String(inner) => serde_json::from_str::<Value>(inner)
                    .unwrap_or_else(|_| item.clone()),
                other => other.clone(),
            };
            match serde_json::from_value::<HiddenRow>(item.clone()) {
                Ok(fact) if !fact.question.trim().is_empty()
                         && !fact.answer.trim().is_empty() => facts.push(fact),
                _ => ignored.push(
                    serde_json::to_string(&item).unwrap_or_default().chars().take(60).collect()),
            }
        }
        // Пустой список означал бы «удали все факты» — а это почти
        // наверняка не то, чего просил преподаватель
        if !facts.is_empty() {
            patch.hidden = Some(facts);
        }
    }

    let empty = patch.legend.is_none()
        && patch.applicant_name.is_none()
        && patch.applicant_status.is_none()
        && patch.norm_sec.is_none()
        && patch.answers.is_none()
        && patch.hidden.is_none();

    (reply, if empty { None } else { Some(patch) }, ignored)
}

pub async fn load(pool: &PgPool, id: Uuid) -> Result<ScenarioFull> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        Option<i32>, Option<i32>, String, String, String, String,
        i32, String, String, Value, Option<String>, Option<Vec<String>>, Option<String>,
    )> = sqlx::query_as(
        "SELECT ticket, call_no, legend, applicant_name, applicant_phone,
                applicant_status, norm_sec, status::text, source, expected, classify_reason,
                legend_statements, title
           FROM scenarios WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some((ticket, call_no, legend, applicant_name, applicant_phone,
              applicant_status, norm_sec, status, source, expected, classify_reason,
              legend_statements, title)) = row
    else {
        anyhow::bail!("сценарий не найден");
    };

    #[allow(clippy::type_complexity)]
    let hidden: Vec<HiddenRow> = sqlx::query_as::<_, (String, String, String, Vec<String>, Option<String>)>(
        "SELECT key, question, answer, unlocks, statement FROM scenario_hidden_facts
          WHERE scenario_id = $1 AND NOT retired ORDER BY sort_order",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(key, question, answer, unlocks, statement)| HiddenRow { key, question, answer, unlocks, statement })
    .collect();

    Ok(ScenarioFull {
        id, title, ticket, call_no, legend, applicant_name, applicant_phone,
        applicant_status, norm_sec, status, source, expected, classify_reason, hidden,
        legend_statements,
    })
}

const SYSTEM_TEMPLATE: &str = "\
Ты помогаешь преподавателю учебного центра править учебные вызовы для \
тренажёра операторов Системы-112.

Вызов состоит из легенды (первая фраза заявителя) и скрытых фактов — \
того, что заявитель сообщает только в ответ на вопрос оператора.

Правь ровно то, о чём просят. Не переписывай остальное.

Отвечай только JSON с двумя полями:
  reply — одна короткая фраза о том, что ты сделал, до двадцати слов. Своими словами, по делу.
  patch — изменившиеся поля вызова.

В patch допустимы только такие поля:
  legend — строка, первая фраза заявителя
  applicantName — строка, ФИО заявителя
  applicantStatus — одно из: {СТАТУСЫ}
  normSec — число, норматив в секундах
  hidden — список объектов с полями key, question, answer

hidden — это ПОЛНЫЙ новый список скрытых фактов, а не один изменённый. \
ФИО заявителя кладётся в applicantName, а не в hidden. Если правка \
не нужна — patch не указывай.";

/// Подсказка с подставленным справочником статусов.
fn system() -> String {
    SYSTEM_TEMPLATE.replace("{СТАТУСЫ}", &crate::domain::applicant::listed())
}

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

    let mut messages = vec![json!({ "role": "system", "content": system() })];
    messages.push(json!({
        "role": "user",
        "content": format!(
            "Вот текущий вызов:\n{}\n\nДальше — замечания преподавателя.",
            // Показываем все поля, которые модель вправе менять.
            // Раньше ФИО и статус заявителя в контекст не попадали:
            // на «поменяй ФИО» модель не видела текущего значения и
            // правила то, что видела, — первую фразу и норматив
            serde_json::to_string_pretty(&json!({
                "legend": scenario.legend,
                "applicantName": scenario.applicant_name,
                "applicantStatus": scenario.applicant_status,
                "normSec": scenario.norm_sec,
                // Утверждения — служебное поле сверки, модели правки
                // оно не нужно и только сбивало бы её
                "hidden": scenario.hidden.iter()
                    .map(|f| json!({
                        "key": f.key, "question": f.question,
                        "answer": f.answer, "unlocks": f.unlocks,
                    }))
                    .collect::<Vec<_>>(),
                "answers": scenario.expected.get("answers"),
            }))?)
    }));
    for (role, content) in history {
        let role = if role == "teacher" { "user" } else { "assistant" };
        messages.push(json!({ "role": role, "content": content }));
    }
    messages.push(json!({ "role": "user", "content": comment }));

    // Схема ответа. Раньше модель клала ФИО в hidden, заворачивала
    // факт в строку и роняла разбор — при ограничении вывода схемой
    // это структурно невозможно.
    //
    // Длины строк ограничены не для красоты. Qwen на видеокарте начинал
    // рассуждать прямо внутри `reply` («Готово. Вывожу JSON…») и ходил
    // по кругу, пока не кончались токены: JSON не закрывался, сервер
    // отдавал 500. С `maxLength` движок сам закрывает строку
    let schema = json!({
        "type": "object",
        "properties": {
            "patch": {
                "type": "object",
                "properties": {
                    "legend":          { "type": "string", "minLength": 5, "maxLength": 600 },
                    "applicantName":   { "type": "string", "maxLength": 80 },
                    "applicantStatus": { "type": "string",
                        "enum": crate::domain::applicant::STATUSES },
                    "normSec":         { "type": "integer", "minimum": 15, "maximum": 600 },
                    "hidden": {
                        "type": "array", "minItems": 1, "maxItems": 8,
                        "items": {
                            "type": "object",
                            "properties": {
                                "key":      { "type": "string", "minLength": 2, "maxLength": 40 },
                                "question": { "type": "string", "minLength": 5, "maxLength": 200 },
                                "answer":   { "type": "string", "minLength": 2, "maxLength": 400 },
                            },
                            "required": ["key", "question", "answer"],
                            "additionalProperties": false,
                        },
                    },
                },
                "additionalProperties": false,
            },
            "reply": { "type": "string", "minLength": 3, "maxLength": 300 },
        },
        // patch обязателен, хоть и может быть пустым: ключи идут по
        // алфавиту, и модель сначала решает, что править, а потом
        // говорит, что сделала. Пока patch был необязательным, Qwen
        // пропускал его и расписывал в reply правку, которой не было
        "required": ["patch", "reply"],
        "additionalProperties": false,
    });

    // Потолок с запасом: правка несёт и ответ, и изменённые поля вызова
    // (легенду, до восьми скрытых ответов) — при 900 токенах полная
    // правка могла не уместиться
    let raw = llm.chat_json(Value::Array(messages), 2000, Some(schema)).await?;
    let (mut reply, patch, ignored) = parse_turn(&raw);

    if !ignored.is_empty() {
        tracing::info!(?ignored, "часть правки не разобралась, пропускаю");
        reply.push_str(&format!(
            "\n\n(часть правки не удалось применить: {})",
            ignored.join("; ")));
    }

    Ok((reply, patch, raw))
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
            -- Утверждения легенды устаревают вместе с ней
            legend_statements = CASE WHEN $2 IS NOT NULL AND $2 <> legend
                                     THEN NULL ELSE legend_statements END,
            legend           = COALESCE($2, legend),
            applicant_name   = COALESCE($3, applicant_name),
            applicant_status = COALESCE($4, applicant_status),
            norm_sec         = COALESCE($5, norm_sec),
            -- Пустая тема — снять её
            title            = CASE WHEN $6::text IS NULL THEN title
                                    ELSE NULLIF(btrim($6), '') END
          WHERE id = $1",
    )
    .bind(id)
    .bind(patch.legend.as_ref())
    .bind(patch.applicant_name.as_ref())
    .bind(patch.applicant_status.as_ref())
    .bind(patch.norm_sec)
    .bind(patch.title.as_ref())
    .execute(pool)
    .await?;

    // ФИО заявителя живёт в двух местах: полем вызова и ответом на
    // вопрос «как вас зовут». Если поправить только поле, заявитель
    // в разговоре продолжит называть старое имя, а эталон будет ждать
    // новое — курсант не сможет выяснить то, что от него требуют.
    // Модель об этом помнить не обязана, поэтому чиним здесь
    if let Some(новое) = &patch.applicant_name {
        let старое = current.applicant_name.trim();
        if !старое.is_empty() && старое != новое.trim() {
            sqlx::query(
                "UPDATE scenario_hidden_facts
                    SET answer = replace(answer, $2, $3),
                        statement = replace(statement, $2, $3)
                  WHERE scenario_id = $1 AND NOT retired AND answer LIKE '%' || $2 || '%'",
            )
            .bind(id)
            .bind(старое)
            .bind(новое.trim())
            .execute(pool)
            .await?;
        }
    }

    if let Some(hidden) = &patch.hidden {
        // Заменяем целиком: модель присылает новый список, а не дельту.
        // Сопоставлять по ключам надёжнее было бы, но модель их
        // переименовывает, и правка «убери второй вопрос» тогда не
        // работает
        let keys: Vec<String> = hidden.iter().map(|f| f.key.clone()).collect();

        // Выбывшие снимаем с учёта, а не удаляем: на них может
        // ссылаться история занятия, из которой считается «выяснил».
        // Удаление роняло правку отыгранного вызова целиком
        sqlx::query(
            "UPDATE scenario_hidden_facts SET retired = true
              WHERE scenario_id = $1 AND NOT retired AND key <> ALL($2)",
        )
        .bind(id)
        .bind(&keys)
        .execute(pool)
        .await?;

        // Те, на которые так никто и не сослался, убираем совсем —
        // иначе таблица копила бы мусор от каждой правки черновика
        sqlx::query(
            "DELETE FROM scenario_hidden_facts f
              WHERE f.scenario_id = $1 AND f.retired
                AND NOT EXISTS (
                    SELECT 1 FROM revealed_facts r WHERE r.fact_id = f.id)",
        )
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
                         sort_order = EXCLUDED.sort_order,
                         -- утверждение готовилось к прежнему ответу
                         statement = CASE WHEN scenario_hidden_facts.answer = EXCLUDED.answer
                                          THEN scenario_hidden_facts.statement END,
                         -- ключ мог быть снят с учёта прошлой правкой
                         retired = false",
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
    let type_ids: Vec<String> = match &patch.type_ids {
        Some(t) => t.clone(),
        None => current
            .expected
            .get("typeIds")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    };
    let address = patch
        .address
        .clone()
        .or_else(|| current.expected.get("address").cloned())
        .unwrap_or(json!({}));

    let result = classify(pool, version_id, &type_ids, &answers).await?;
    let expected = json!({
        "typeIds": type_ids,
        "answers": answers,
        "address": address,
        "services": result.services.iter().map(|s| s.service_id.clone()).collect::<Vec<_>>(),
        "outcome": result.outcome,
    });

    sqlx::query("UPDATE scenarios SET expected = $2, classify_reason = $3 WHERE id = $1")
        .bind(id)
        .bind(&expected)
        .bind(result.reason.as_ref())
        .execute(pool)
        .await?;

    // Сброшенные правкой утверждения готовятся заново в очереди. Ошибка
    // постановки правку не отменяет: утверждения дозаполнит и запуск
    if let Err(e) = crate::domain::fact_statements::enqueue(pool, id, None).await {
        tracing::warn!(error = %e, scenario = %id, "утверждения фактов не поставлены в очередь");
    }

    load(pool, id).await
}

/// Что в вызове изменилось на самом деле.
///
/// Модель охотно отчитывается о правке, которой не сделала: на просьбу
/// «убери вопрос про подъезд» возвращает тот же список из четырёх
/// вопросов и пишет «убрал». Если верить ей на слово, преподаватель
/// видит «изменено: hidden», смотрит на вызов — а там всё по-старому,
/// и выглядит это как незакрывшееся окно, а не как упрямство модели.
///
/// Поэтому в переписку пишем не то, что прислала модель, а разницу
/// между вызовом до и после.
pub fn changed_fields(before: &ScenarioFull, after: &ScenarioFull) -> Value {
    let mut out = serde_json::Map::new();

    if before.legend != after.legend {
        out.insert("legend".into(), json!(after.legend));
    }
    if before.applicant_name != after.applicant_name {
        out.insert("applicantName".into(), json!(after.applicant_name));
    }
    if before.applicant_status != after.applicant_status {
        out.insert("applicantStatus".into(), json!(after.applicant_status));
    }
    if before.norm_sec != after.norm_sec {
        out.insert("normSec".into(), json!(after.norm_sec));
    }

    // Факты сравниваем по содержимому, а не по порядку следования:
    // модель иногда переставляет их местами, ничего не меняя по сути
    let свод = |s: &ScenarioFull| -> Vec<(String, String, String)> {
        let mut v: Vec<_> = s
            .hidden
            .iter()
            .map(|f| (f.key.clone(), f.question.clone(), f.answer.clone()))
            .collect();
        v.sort();
        v
    };
    if свод(before) != свод(after) {
        out.insert("hidden".into(), json!(after.hidden));
    }

    // Эталон пересчитывается классификатором и меняется сам,
    // когда правка задела признаки
    if before.expected.get("outcome") != after.expected.get("outcome") {
        out.insert("outcome".into(), after.expected.get("outcome").cloned().unwrap_or(Value::Null));
    }

    Value::Object(out)
}
