//! Подготовка учебного вызова из билета.
//!
//! Главное ограничение, ради которого всё и устроено так:
//!
//! **Модель пишет только текст. Эталон считает классификатор.**
//!
//! Признаки происшествия модель не придумывает, а выбирает из
//! закрытого словаря опросной карты — тех самых формулировок, что
//! видит курсант на экране. Выбранное прогоняется через движок ЕКП,
//! и уже он решает, какой это тип происшествия и какие службы едут.
//!
//! Поэтому модель физически не может создать вызов, который будет
//! оценён неверно: худшее, что она сделает — выберет не те признаки,
//! и это увидит преподаватель при проверке. Придумать службу, которой
//! нет в матрице, или тип, которого нет в классификаторе, она не может.
//!
//! Свободно она пишет ровно одно: как человек говорит. Первую фразу
//! заявителя и его ответы на вопросы.

use crate::{
    domain::classify::classify,
    ingest::tickets::TicketCall,
    ml::Llm,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Что модель выбрала из закрытых словарей.
#[derive(Debug, Deserialize)]
pub struct Picked {
    #[serde(default)]
    pub type_ids: Vec<String>,
    #[serde(default)]
    pub answers: std::collections::HashMap<String, Vec<String>>,
}

/// Что модель написала своими словами.
#[derive(Debug, Deserialize, Serialize)]
pub struct Written {
    pub legend: String,
    #[serde(default)]
    pub hidden: Vec<HiddenFact>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HiddenFact {
    pub key: String,
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub unlocks: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Generated {
    pub ticket: i32,
    pub call_no: i32,
    pub legend: String,
    pub applicant_name: String,
    pub applicant_phone: String,
    pub hidden: Vec<HiddenFact>,
    pub expected: Value,
    /// Чего не хватило классификатору, если позиция не определилась.
    /// Такой вызов сохраняем черновиком: преподаватель разберётся
    pub reason: Option<String>,
}

/// Словарь признаков одного типа происшествия — то, из чего модель
/// выбирает. Берём из базы, а не из промпта: формулировки должны
/// совпадать с экраном курсанта дословно.
async fn card_vocabulary(pool: &PgPool, type_id: &str) -> Result<Value> {
    let rows: Vec<(String, String, String, Option<Vec<String>>)> = sqlx::query_as(
        "SELECT q.id, q.label, q.kind::text,
                (SELECT array_agg(o.value ORDER BY o.sort_order)
                   FROM survey_options o
                  WHERE o.type_id = q.type_id AND o.question_id = q.id)
           FROM survey_questions q
          WHERE q.type_id = $1
          ORDER BY q.sort_order",
    )
    .bind(type_id)
    .fetch_all(pool)
    .await?;

    Ok(Value::Array(
        rows.into_iter()
            .map(|(id, label, kind, options)| {
                json!({ "id": id, "label": label, "kind": kind,
                        "options": options.unwrap_or_default() })
            })
            .collect(),
    ))
}

/// Типы происшествия, которые вообще можно выбрать.
///
/// Названия у части типов — просто номера службы: «101», «104».
/// Модели по ним не догадаться, что это пожар и запах газа, поэтому
/// подставляем название группы из классификатора.
///
/// Заодно помечаем, у каких типов есть опросная карта: без неё
/// признаки задать нечем и эталон не построится.
async fn incident_types(pool: &PgPool) -> Result<Value> {
    let rows: Vec<(String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT t.id, t.title,
                (SELECT DISTINCT p.group_title FROM classifier_positions p
                  WHERE p.group_no = t.group_no LIMIT 1),
                (SELECT count(*) FROM survey_questions q WHERE q.type_id = t.id)
           FROM incident_types t
          ORDER BY (SELECT count(*) FROM survey_questions q WHERE q.type_id = t.id) DESC,
                   t.frequent DESC, t.id",
    )
    .fetch_all(pool)
    .await?;

    Ok(Value::Array(
        rows.into_iter()
            .map(|(id, title, group, questions)| {
                // «101» без пояснения — бесполезная подсказка
                let name = match &group {
                    Some(g) if g != &title => format!("{title} — {g}"),
                    _ => title,
                };
                json!({ "id": id, "title": name, "hasCard": questions > 0 })
            })
            .collect(),
    ))
}

const SYSTEM: &str = "\
Ты помогаешь преподавателю учебного центра готовить вызовы для тренажёра \
операторов Системы-112. Отвечай только JSON, без пояснений и без разметки.";

/// Шаг 1: выбрать тип происшествия и признаки из закрытых словарей.
async fn pick_signs(
    llm: &dyn Llm,
    pool: &PgPool,
    call: &TicketCall,
    complaint: Option<&str>,
) -> Result<Picked> {
    let types = incident_types(pool).await?;

    let chosen: Value = llm
        .chat_json(
            json!([
                { "role": "system", "content": SYSTEM },
                { "role": "user", "content": format!(
                    "Происшествие: {}\n\nВыбери из списка подходящий тип. \
                     Обычно один. Смотри на поле title — там написано, \
                     что это за происшествие.\n\nСписок: {}\n\n\
                     Ответь: {{\"typeIds\": [\"...\"]}}",
                    call.situation, serde_json::to_string(&types)?) },
            ]),
            200,
            Some(json!({
                "type": "object",
                "properties": { "typeIds": { "type": "array", "items": { "type": "string" } } },
                "required": ["typeIds"],
                "additionalProperties": false,
            })),
        )
        .await?;

    let type_ids: Vec<String> = chosen
        .get("typeIds")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    // Тип, которого нет в справочнике, отбрасываем молча: модель
    // иногда придумывает правдоподобный идентификатор
    let known: Vec<String> = sqlx::query_scalar("SELECT id FROM incident_types")
        .fetch_all(pool)
        .await?;
    let type_ids: Vec<String> = type_ids.into_iter().filter(|t| known.contains(t)).collect();

    if type_ids.is_empty() {
        return Ok(Picked { type_ids, answers: Default::default() });
    }

    let mut answers = std::collections::HashMap::new();
    for type_id in &type_ids {
        let vocab = card_vocabulary(pool, type_id).await?;
        if vocab.as_array().is_some_and(|a| a.is_empty()) {
            continue;
        }

        let picked: Value = llm
            .chat_json(
                json!([
                    { "role": "system", "content": SYSTEM },
                    { "role": "user", "content": format!(
                        "Происшествие: {situation}\n\nЗаполни опросную карту. \
                         Значения бери ТОЛЬКО из options дословно. Что из описания \
                         неизвестно — пропусти, не придумывай.{hint}\n\n\
                         Карта: {vocab}\n\n\
                         Ответь: {{\"answers\": {{\"идентификатор_вопроса\": [\"значение\"]}}}}",
                        situation = call.situation,
                        vocab = serde_json::to_string(&vocab)?,
                        hint = complaint.map(|c| format!(
                            "\n\nПрошлая попытка не прошла проверку классификатором: \
                             «{c}». Добери недостающее — оно наверняка есть в описании.")).
                            unwrap_or_default()) },
                ]),
                600,
                Some(json!({
                    "type": "object",
                    "properties": { "answers": { "type": "object" } },
                    "required": ["answers"],
                    "additionalProperties": false,
                })),
            )
            .await?;

        if let Some(obj) = picked.get("answers").and_then(|v| v.as_object()) {
            for (question, value) in obj {
                let values: Vec<String> = match value {
                    Value::Array(a) => {
                        a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
                    }
                    Value::String(s) => vec![s.clone()],
                    _ => continue,
                };
                if !values.is_empty() {
                    answers.insert(question.clone(), values);
                }
            }
        }
    }

    Ok(Picked { type_ids, answers })
}

/// Отбросить выбранное, чего нет в словаре карты.
///
/// Модель иногда «улучшает» формулировку — пишет «Частный дом» вместо
/// «Дом частный». Для классификатора это разные строки, и позиция
/// не найдётся. Сверяем дословно.
async fn keep_known_answers(
    pool: &PgPool,
    type_ids: &[String],
    answers: std::collections::HashMap<String, Vec<String>>,
) -> Result<(std::collections::HashMap<String, Vec<String>>, Vec<String>)> {
    let mut kept = std::collections::HashMap::new();
    let mut dropped = Vec::new();

    for (question, values) in answers {
        let known: Vec<String> = sqlx::query_scalar(
            "SELECT value FROM survey_options
              WHERE question_id = $1 AND type_id = ANY($2)",
        )
        .bind(&question)
        .bind(type_ids)
        .fetch_all(pool)
        .await?;

        // Вопрос со свободным вводом (этажность, описание) словаря
        // не имеет — такое пропускаем как есть
        if known.is_empty() {
            kept.insert(question, values);
            continue;
        }

        let (ok, bad): (Vec<String>, Vec<String>) =
            values.into_iter().partition(|v| known.contains(v));
        if !ok.is_empty() {
            kept.insert(question.clone(), ok);
        }
        for b in bad {
            dropped.push(format!("{question}: «{b}»"));
        }
    }

    Ok((kept, dropped))
}

/// Образец нужной ФОРМЫ.
///
/// Показать регистр речи примером дешевле, чем описать словами. Но
/// содержание модель норовит списать с образца — поэтому взят вызов
/// из совсем другой области: происшествие с лифтом, а генерируем мы
/// в основном пожары. Совпадений, которые можно было бы скопировать,
/// в нём нет.
const EXAMPLE: &str = r#"{
  "legend": "Девушка, помогите! Мы в лифте застряли, четвёртый подъезд, не открывается!",
  "hidden": [
    {"key": "people", "question": "Сколько человек в кабине?",
     "answer": "Трое нас, я и двое детей."},
    {"key": "floor", "question": "Между какими этажами стоите?",
     "answer": "Да кто ж его знает, где-то между третьим и четвёртым."},
    {"key": "health", "question": "Кому-нибудь плохо?",
     "answer": "Нет, все нормально, только младший перепугался."}
  ]
}"#;

/// Шаг 2: написать, как человек говорит.
///
/// Единственное место, где модель пишет свободно. Всё остальное она
/// выбирает из готовых списков.
async fn write_dialogue(llm: &dyn Llm, call: &TicketCall) -> Result<Written> {
    let hidden_hint = match &call.address_hidden {
        Some(h) => format!(
            "\n\nВАЖНО: настоящий адрес — «{h}». Заявитель его сам НЕ называет: \
             в легенде он говорит только ориентир. Точный адрес он сообщает \
             единственным скрытым фактом, и только если оператор спросит."),
        None => String::new(),
    };

    let raw = llm
        .chat_json(
            json!([
                { "role": "system", "content": SYSTEM },
                { "role": "user", "content": format!(
                    "Напиши учебный вызов в службу 112.\n\n\
                     ОПИСАНИЕ ПРОИСШЕСТВИЯ: {situation}\n\
                     ОРИЕНТИР, который заявитель называет сам: {address}{hidden_hint}\n\n\
                     Правила:\n\
                     1. legend — первая фраза заявителя в трубку. Живая речь \
                        взволнованного человека, одно-два предложения. В ней только \
                        то, что он выпаливает сразу: что случилось и где примерно.\n\
                     2. В легенде НЕ должно быть того, что оператор обязан выяснить: \
                        ни города, ни числа пострадавших, ни газификации, ни этажей.\n\
                     3. hidden — три-пять РАЗНЫХ фактов. question — что спрашивает \
                        ОПЕРАТОР. answer — что отвечает ЗАЯВИТЕЛЬ. Каждый факт про \
                        своё: не делай четыре вопроса об адресе.\n\
                     4. Не добавляй ничего, чего нет в описании.\n\n\
                     Ниже образец — он показывает только ФОРМУ и манеру речи. \
                     Происшествие там другое, содержание из него не бери:\n{example}\n\n\
                     Ответь JSON той же формы, но по описанию выше.",
                    situation = call.situation,
                    address = call.address,
                    hidden_hint = hidden_hint,
                    example = EXAMPLE) },
            ]),
            800,
            Some(json!({
                "type": "object",
                "properties": {
                    "legend": { "type": "string", "minLength": 10 },
                    "hidden": {
                        "type": "array", "minItems": 3, "maxItems": 6,
                        "items": {
                            "type": "object",
                            "properties": {
                                "key":      { "type": "string", "minLength": 2 },
                                "question": { "type": "string", "minLength": 5 },
                                "answer":   { "type": "string", "minLength": 2 },
                            },
                            "required": ["key", "question", "answer"],
                            "additionalProperties": false,
                        },
                    },
                },
                "required": ["legend", "hidden"],
                "additionalProperties": false,
            })),
        )
        .await?;

    let mut written: Written =
        serde_json::from_value(raw).context("модель вернула JSON не той формы")?;

    // Модель иногда меняет роли местами и пишет вопрос от лица
    // заявителя. Одинаковые ключи тоже случаются — оставляем первый
    let mut seen = std::collections::BTreeSet::new();
    written.hidden.retain(|f| {
        !f.question.trim().is_empty()
            && !f.answer.trim().is_empty()
            && seen.insert(f.key.clone())
    });

    Ok(written)
}

/// Собрать вызов целиком.
pub async fn generate(
    llm: &dyn Llm,
    pool: &PgPool,
    version_id: Uuid,
    call: &TicketCall,
) -> Result<Generated> {
    let picked = pick_signs(llm, pool, call, None).await?;
    let (answers, dropped) = keep_known_answers(pool, &picked.type_ids, picked.answers).await?;

    if !dropped.is_empty() {
        tracing::info!(ticket = call.ticket, call = call.call_no,
                       ?dropped, "признаки вне словаря отброшены");
    }

    // Вот она, граница: дальше решает классификатор, а не модель
    let mut result = classify(pool, version_id, &picked.type_ids, &answers).await?;
    let mut answers = answers;

    // Классификатор не просто отвергает — он говорит, какого признака
    // не хватило. Возвращаем эту жалобу модели: пусть добирает.
    // Судьёй остаётся ЕКП, модель только предлагает
    if let Some(reason) = result.reason.clone() {
        if !picked.type_ids.is_empty() {
            tracing::info!(ticket = call.ticket, call = call.call_no, %reason,
                           "классификатор просит уточнения, спрашиваю модель ещё раз");

            let retry = pick_signs(llm, pool, call, Some(&reason)).await?;
            let (mut merged, _) =
                keep_known_answers(pool, &picked.type_ids, retry.answers).await?;
            // Уже выбранное не затираем: добираем недостающее
            for (k, v) in answers.clone() {
                merged.entry(k).or_insert(v);
            }

            let second = classify(pool, version_id, &picked.type_ids, &merged).await?;
            // Принимаем вторую попытку, только если стало лучше
            if second.reason.is_none() || second.outcome.is_some() {
                answers = merged;
                result = second;
            }
        }
    }

    // Опросные карты сняты с кадров боевой системы, и есть они пока
    // у трёх типов. Для остальных признаки задать нечем — вызов
    // сохраняем черновиком и прямо говорим почему
    if answers.is_empty() && !picked.type_ids.is_empty() {
        let with_card: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM survey_questions WHERE type_id = ANY($1)")
            .bind(&picked.type_ids)
            .fetch_one(pool)
            .await?;
        if with_card == 0 {
            result.reason = Some(format!(
                "для типа «{}» нет опросной карты — признаки задать нечем, \
                 эталон не построен",
                picked.type_ids.join(", ")));
        }
    }

    let written = write_dialogue(llm, call).await?;

    let mut address = json!({});
    if let Some(obj) = address.as_object_mut() {
        obj.insert("descriptive".into(), json!(call.address));
        if let Some(hidden) = &call.address_hidden {
            obj.insert("raw".into(), json!(hidden));
        }
    }

    let expected = json!({
        "typeIds": picked.type_ids,
        "answers": answers,
        "address": address,
        "services": result.services.iter().map(|s| s.service_id.clone()).collect::<Vec<_>>(),
        "outcome": result.outcome,
    });

    Ok(Generated {
        ticket: call.ticket,
        call_no: call.call_no,
        legend: written.legend,
        applicant_name: call.applicant_name.clone().unwrap_or_default(),
        applicant_phone: call.applicant_phone.clone().unwrap_or_default(),
        hidden: written.hidden,
        expected,
        reason: result.reason,
    })
}
