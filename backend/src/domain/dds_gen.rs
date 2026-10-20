//! Учебное происшествие для диспетчера ДДС, собранное системой целиком.
//!
//! Обратная связь экспертов 18.10: «показать возможность формирования
//! искусственным интеллектом карточек для специалиста ДДС». По описанию
//! преподавателя модель собирала их и раньше; здесь карточку не нужно
//! даже описывать — достаточно назвать службу.
//!
//! Модель службы не выбирает — их, как и везде в тренажёре, считает
//! классификатор. Поэтому идём от классификатора: берём случайную
//! позицию, по которой эта служба реагирует, восстанавливаем ответы
//! опросной карты, которые к ней ведут, и проверяем тем же
//! классификатором, что служба действительно в списке. Модель потом
//! только пишет текст карточки под эти ответы (`api::dds_incidents`).
//!
//! «Мослифт» и «Деп. ЖКХ» в матрице классификатора карточку получают
//! только для сведения («карточка-112»): реагирующих позиций у них нет.
//! Для них берём такие позиции, а службу добавляем вручную — так же
//! собраны происшествия учебного набора (`deploy/seed/эталоны.json`).
//! Значение «Карточка-112» с заглавной `classify` считает реагированием
//! (сравнивает с учётом регистра) — тогда служба подобрана им самим.

use crate::domain::classify::classify;
use anyhow::Result;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

/// Значения матрицы, при которых служба получает карточку для сведения.
const ДЛЯ_СВЕДЕНИЯ: [&str; 2] = ["карточка-112", "карточка -112"];

/// Сколько позиций классификатора перебрать, прежде чем сдаться.
const ПОПЫТОК: i64 = 60;

/// Что собрано: тип, ответы карты и что из них вышло у классификатора.
#[derive(Debug, Clone)]
pub struct Plan {
    pub type_id: String,
    pub type_title: String,
    pub answers: HashMap<String, Vec<String>>,
    /// Итоговый тип по классификатору
    pub outcome: String,
    /// Служба получает карточку только для сведения: добавить вручную
    pub informational: bool,
    /// Ответы карты словами — для модели: «Где: Дом»
    pub facts: Vec<String>,
}

#[derive(Deserialize)]
struct ShowWhen {
    question: String,
    values: Vec<String>,
}

struct Question {
    id: String,
    label: String,
    multi: bool,
    show_when: Vec<ShowWhen>,
}

/// Собрать признаки происшествия, на которое реагирует служба.
/// `None` — у службы нет ни одной позиции, которую удалось собрать.
pub async fn plan(pool: &PgPool, version_id: Uuid, service_id: &str) -> Result<Option<Plan>> {
    // Сначала позиции, где служба реагирует, потом — где получает
    // карточку для сведения. Внутри — случайно, но с весом: чаще группы
    // классификатора, где служба реагирует почти на всё (у Мосгаза —
    // запах газа и пожары в газифицированных домах), и позиции, куда она
    // едет одна или почти одна. Без веса диспетчеру Мосгаза доставались
    // провал грунта и падение беспилотника, куда едут девять служб
    // (проверка на модели 18.10). Вес = доля группы², делённая на число
    // служб позиции; выбор с весом — сортировкой по −ln(u)/вес
    let candidates: Vec<(i32, Option<String>, Option<String>, Option<String>, bool,
                         Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "WITH реагируют AS (
             SELECT DISTINCT ps.position_id, ps.service_id
               FROM classifier_position_services ps
               JOIN classifier_positions p ON p.id = ps.position_id AND p.version_id = $1
              WHERE coalesce(btrim(ps.value), '') <> ''
                AND NOT lower(btrim(ps.value)) = ANY($3)
                AND lower(btrim(ps.value)) <> 'нет реагирования'),
         доля AS (
             SELECT p.group_no,
                    count(r.position_id)::float8 / count(*) AS share
               FROM classifier_positions p
               LEFT JOIN реагируют r ON r.position_id = p.id AND r.service_id = $2
              WHERE p.version_id = $1
              GROUP BY p.group_no),
         служб AS (
             SELECT position_id, count(*) AS k FROM реагируют GROUP BY position_id)
         SELECT p.group_no, p.sign1, p.sign2, p.sign3,
                lower(btrim(ps.value)) = ANY($3) AS info,
                cm.kind::text, cm.question_id, cm.match_value
           FROM classifier_positions p
           JOIN classifier_position_services ps ON ps.position_id = p.id
           LEFT JOIN condition_mapping cm ON cm.condition = ps.condition
           LEFT JOIN доля d ON d.group_no = p.group_no
           LEFT JOIN служб k ON k.position_id = p.id
          WHERE p.version_id = $1 AND ps.service_id = $2
            AND coalesce(btrim(p.sign1), '') <> ''
            AND coalesce(btrim(ps.value), '') <> ''
            AND lower(btrim(ps.value)) <> 'нет реагирования'
          ORDER BY 5,
                   -ln(1 - random()) * greatest(coalesce(k.k, 0), 1)
                   / greatest(coalesce(d.share, 0) ^ 2, 0.0001)
          LIMIT $4",
    )
    .bind(version_id)
    .bind(service_id)
    .bind(&ДЛЯ_СВЕДЕНИЯ[..])
    .bind(ПОПЫТОК)
    .fetch_all(pool)
    .await?;

    for (group_no, s1, s2, s3, info, kind, cond_q, cond_v) in candidates {
        let types: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, title FROM incident_types WHERE group_no = $1 ORDER BY random()",
        )
        .bind(group_no)
        .fetch_all(pool)
        .await?;

        for (type_id, type_title) in types {
            let predicate = (kind.as_deref() == Some("predicate"))
                .then(|| cond_q.clone().zip(cond_v.clone()))
                .flatten();
            let Some(answers) =
                answers_for(pool, &type_id, [&s1, &s2, &s3], predicate).await?
            else {
                continue;
            };

            let result = classify(pool, version_id, std::slice::from_ref(&type_id), &answers).await?;
            let Some(outcome) = result.outcome.clone() else { continue };
            let реагирует = result.services.iter().any(|s| s.service_id == service_id);
            let для_сведения =
                info && result.informational.iter().any(|s| s.service_id == service_id);
            if !реагирует && !для_сведения {
                continue;
            }

            let facts = facts(pool, &type_id, &answers).await?;
            return Ok(Some(Plan {
                type_id,
                type_title,
                answers,
                outcome,
                informational: !реагирует,
                facts,
            }));
        }
    }
    Ok(None)
}

/// Ответы карты типа, которые ведут к признакам позиции.
///
/// Вопрос, который в карте появляется только при определённом ответе
/// на другой («Дом (пламя, дым)» — только при «Где: Дом»), без этого
/// ответа не задают: дописываем и его, иначе карточка противоречит
/// сама себе.
async fn answers_for(
    pool: &PgPool,
    type_id: &str,
    signs: [&Option<String>; 3],
    predicate: Option<(String, String)>,
) -> Result<Option<HashMap<String, Vec<String>>>> {
    let questions = questions(pool, type_id).await?;
    let by_id: HashMap<&str, &Question> = questions.iter().map(|q| (q.id.as_str(), q)).collect();
    let mut answers: HashMap<String, Vec<String>> = HashMap::new();

    for (level, sign) in signs.into_iter().enumerate() {
        let Some(sign) = sign.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let found: Option<(String, String)> = sqlx::query_as(
            "SELECT question_id, option_value FROM sign_mapping
              WHERE type_id = $1 AND sign_level = $2 AND sign_text = $3
              ORDER BY random() LIMIT 1",
        )
        .bind(type_id)
        .bind(level as i16 + 1)
        .bind(sign)
        .fetch_optional(pool)
        .await?;
        let Some((q, v)) = found else { return Ok(None) };
        if !добавить(&mut answers, &by_id, q, v) {
            return Ok(None);
        }
    }

    // Подколонка службы с условием («угроза людям»): без ответа на него
    // служба по позиции не реагирует
    if let Some((q, v)) = predicate {
        let есть: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM survey_options
                             WHERE type_id = $1 AND question_id = $2 AND value = $3)",
        )
        .bind(type_id)
        .bind(&q)
        .bind(&v)
        .fetch_one(pool)
        .await?;
        if есть && !добавить(&mut answers, &by_id, q, v) {
            return Ok(None);
        }
    }

    // Условия показа — до неподвижной точки: дописанный ответ сам
    // может требовать ответа выше по карте
    for _ in 0..8 {
        let mut changed = false;
        let answered: Vec<String> = answers.keys().cloned().collect();
        for id in answered {
            let Some(q) = by_id.get(id.as_str()) else { continue };
            if q.show_when.is_empty() {
                continue;
            }
            let выполнено = q.show_when.iter().any(|c| {
                answers.get(&c.question).is_some_and(|a| a.iter().any(|v| c.values.contains(v)))
            });
            if выполнено {
                continue;
            }
            let Some(c) = q.show_when.iter().find(|c| !answers.contains_key(&c.question)) else {
                return Ok(None);
            };
            let Some(v) = c.values.first() else { return Ok(None) };
            answers.insert(c.question.clone(), vec![v.clone()]);
            changed = true;
        }
        if !changed {
            return Ok(Some(answers));
        }
    }
    Ok(None)
}

/// Записать ответ. У вопроса с одним ответом второе значение — противоречие.
fn добавить(
    answers: &mut HashMap<String, Vec<String>>,
    by_id: &HashMap<&str, &Question>,
    q: String,
    v: String,
) -> bool {
    let multi = by_id.get(q.as_str()).is_some_and(|q| q.multi);
    let values = answers.entry(q).or_default();
    if values.contains(&v) {
        return true;
    }
    if !values.is_empty() && !multi {
        return false;
    }
    values.push(v);
    true
}

async fn questions(pool: &PgPool, type_id: &str) -> Result<Vec<Question>> {
    let rows: Vec<(String, String, String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT id, label, kind::text, show_when FROM survey_questions
          WHERE type_id = $1 ORDER BY sort_order",
    )
    .bind(type_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, label, kind, show_when)| Question {
            id,
            label,
            multi: kind == "multi",
            show_when: show_when
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
        })
        .collect())
}

/// Ответы карты словами, в порядке карты: «Где: Дом».
async fn facts(
    pool: &PgPool,
    type_id: &str,
    answers: &HashMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    Ok(questions(pool, type_id)
        .await?
        .into_iter()
        .filter_map(|q| {
            let values = answers.get(&q.id)?;
            // «Признак 2 (Здание / объект)» — служебное имя уровня
            // классификатора; модели понятнее без скобок
            let label = q.label.split(" (").next().unwrap_or(&q.label).to_string();
            Some(format!("{label}: {}", values.join(", ")))
        })
        .collect())
}
