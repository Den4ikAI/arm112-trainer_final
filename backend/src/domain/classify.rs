//! Конвейер классификации: ответы опросной карты → позиция → службы.
//!
//! Воспроизводит логику Единого классификатора происшествий. Нейросетей
//! здесь нет: для каждой комбинации признаков есть ровно один правильный
//! ответ, и он записан в ЕКП.
//!
//! ```text
//! ответы карты ──①──► признаки ──②──► позиция ──③──► службы
//! ```
//!
//! Подробности и обоснования — DESIGN-core.md.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// Значения, означающие, что служба получает лишь копию карточки
/// и в списке оповещения не показывается. Памятка: «Службы, которые
/// не реагируют, а собирают информацию для анализа, не отображаются».
///
/// Регистр и пробелы — как в исходном файле, включая опечатку у ОАТИ.
const INFORMATIONAL_VALUES: [&str; 3] = ["карточка-112", "карточка -112", "нет реагирования"];

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotifiedService {
    pub service_id: String,
    pub title: String,
    /// Дословное значение из матрицы: «пожар: квартира» или «карточка-112»
    pub value: String,
    /// Реагирует по-настоящему, а не получает копию
    pub responding: bool,
    /// По какому условию служба попала в список
    pub condition: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyResult {
    /// Итоговый тип происшествия, строка «Класс.» в карточке
    pub outcome: Option<String>,
    /// Номера сработавших позиций классификатора
    pub positions: Vec<String>,
    /// Список оповещения: только реагирующие службы
    pub services: Vec<NotifiedService>,
    /// Службы, получающие копию. Не показываются оператору,
    /// но хранятся: оценка не должна за них штрафовать.
    pub informational: Vec<NotifiedService>,
    /// Если позиция не определена — чего не хватило
    pub reason: Option<String>,
}

/// Признаки, извлечённые из ответов, по уровням.
#[derive(Debug, Default)]
struct Signs {
    level1: BTreeSet<String>,
    level2: BTreeSet<String>,
    level3: BTreeSet<String>,
}

impl Signs {
    fn is_empty(&self) -> bool {
        self.level1.is_empty() && self.level2.is_empty() && self.level3.is_empty()
    }
}

/// ① Сопоставление вариантов карты с признаками классификатора.
async fn extract_signs(
    pool: &PgPool,
    type_id: &str,
    answers: &HashMap<String, Vec<String>>,
) -> Result<Signs> {
    let rows: Vec<(String, String, i16, String)> = sqlx::query_as(
        "SELECT question_id, option_value, sign_level, sign_text
           FROM sign_mapping WHERE type_id = $1",
    )
    .bind(type_id)
    .fetch_all(pool)
    .await?;

    let mut signs = Signs::default();

    for (question_id, option_value, level, sign_text) in rows {
        let selected = answers.get(&question_id).is_some_and(|v| v.contains(&option_value));
        if !selected {
            continue;
        }
        match level {
            1 => signs.level1.insert(sign_text),
            2 => signs.level2.insert(sign_text),
            _ => signs.level3.insert(sign_text),
        };
    }

    Ok(signs)
}

/// Строка матрицы оповещения.
struct MatrixRow {
    service_id: String,
    title: String,
    condition: String,
    kind: String,
    cond_question: Option<String>,
    cond_value: Option<String>,
    value: String,
}

/// Сообщение о неоднозначности.
///
/// Раньше писали только количество вариантов — по такой подсказке
/// не понять, что выбирать. Перечисляем сами значения: их читает
/// и курсант на экране, и модель при подготовке сценария.
fn ambiguity(level: &str, options: &BTreeSet<String>, outcomes: usize) -> String {
    // Длинный список в подсказке бесполезен: он не помещается
    // и не читается
    const SHOWN: usize = 6;
    let shown: Vec<&str> = options.iter().take(SHOWN).map(String::as_str).collect();

    if shown.is_empty() {
        return format!("уточните признак {level} уровня: подходит {outcomes} вариантов");
    }

    let tail = if options.len() > SHOWN {
        format!(" и ещё {}", options.len() - SHOWN)
    } else {
        String::new()
    };
    format!("уточните признак {level} уровня: {}{tail}", shown.join(", "))
}

/// ② + ③ Поиск позиций и вычисление условий.
pub async fn classify(
    pool: &PgPool,
    version_id: Uuid,
    type_ids: &[String],
    answers: &HashMap<String, Vec<String>>,
) -> Result<ClassifyResult> {
    let mut all_positions: Vec<(String, Option<String>)> = Vec::new();
    let mut position_ids: Vec<Uuid> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();

    for type_id in type_ids {
        let group_no: Option<i32> =
            sqlx::query_scalar("SELECT group_no FROM incident_types WHERE id = $1")
                .bind(type_id)
                .fetch_optional(pool)
                .await?
                .flatten();

        let Some(group_no) = group_no else {
            reasons.push(format!("у типа «{type_id}» не задана группа классификатора"));
            continue;
        };

        let signs = extract_signs(pool, type_id, answers).await?;

        if signs.is_empty() {
            reasons.push("не выбрано ни одного признака".into());
            continue;
        }
        if signs.level1.is_empty() {
            reasons.push("не выбран признак первого уровня («где»)".into());
            continue;
        }

        // Признак уровня, который курсант не заполнил, не ограничивает
        // поиск: позиция подойдёт с любым значением на этом уровне.
        let l1: Vec<String> = signs.level1.iter().cloned().collect();
        let l2: Vec<String> = signs.level2.iter().cloned().collect();
        let l3: Vec<String> = signs.level3.iter().cloned().collect();

        // Признаки уровней тянем вместе с позицией: если комбинация
        // окажется неоднозначной, надо будет назвать, между чем
        // именно выбирать
        let found: Vec<(Uuid, String, Option<String>, Option<String>, Option<String>)> =
            sqlx::query_as(
            "SELECT id, number, outcome_type, sign2, sign3
               FROM classifier_positions
              WHERE version_id = $1
                AND group_no = $2
                AND sign1 = ANY($3)
                AND ($4::text[] = '{}' OR sign2 = ANY($4))
                AND ($5::text[] = '{}' OR sign3 = ANY($5))
              ORDER BY number",
        )
        .bind(version_id)
        .bind(group_no)
        .bind(&l1)
        .bind(&l2)
        .bind(&l3)
        .fetch_all(pool)
        .await?;

        if found.is_empty() {
            reasons.push(format!(
                "в группе {group_no} нет позиции для такой комбинации признаков"
            ));
            continue;
        }

        // Уровень, на который курсант не ответил, поиск не ограничивает.
        // Но если у классификатора есть позиция, где этого признака нет
        // вовсе, — ответ именно она: «ДТП» без уточнений. Раньше её было
        // не выбрать: незаполненный уровень давал все позиции ветки, и
        // классификатор просил уточнить то, чего заявитель не говорил
        let пусто = |v: &Option<String>| v.as_deref().is_none_or(str::is_empty);
        let mut found = found;
        if l2.is_empty() && found.iter().any(|f| пусто(&f.3)) && found.iter().any(|f| !пусто(&f.3)) {
            found.retain(|f| пусто(&f.3));
        }
        // На третьем уровне — только когда второй уже определён. Иначе
        // «на улице» без уточнений превращалась в «запах гари на улице»:
        // это единственная позиция ветки без третьего признака, но
        // второй уровень курсант при этом не выбирал вовсе
        let второй_определён = found
            .iter()
            .map(|f| f.3.as_deref().unwrap_or(""))
            .collect::<BTreeSet<_>>()
            .len()
            == 1;
        if l3.is_empty()
            && второй_определён
            && found.iter().any(|f| пусто(&f.4))
            && found.iter().any(|f| !пусто(&f.4))
        {
            found.retain(|f| пусто(&f.4));
        }

        // Незаполненный уровень не ограничивает поиск, и если позиции
        // на нём расходятся — комбинация неоднозначна. Раньше движок
        // молча брал первую по номеру: для «Запах газа в помещении»
        // получался класс «в кухне», хотя про кухню никто не говорил.
        //
        // Боевая система в таком состоянии список оповещения тоже
        // не показывает, поэтому честнее попросить уточнение.
        //
        // Если же второй признак у всех оставшихся позиций один и тот же
        // (у «метро» его нет вовсе), уточнять на втором уровне нечего:
        // расхождение, если оно есть, — ниже
        if l2.is_empty() && !второй_определён && found.len() > 1 {
            let distinct: BTreeSet<_> = found.iter().map(|(_, _, o, _, _)| o.clone()).collect();
            if distinct.len() > 1 {
                let options: BTreeSet<_> =
                    found.iter().filter_map(|(_, _, _, s2, _)| s2.clone()).collect();
                reasons.push(ambiguity("второго", &options, distinct.len()));
                continue;
            }
        }
        if l3.is_empty() && found.len() > 1 {
            let distinct: BTreeSet<_> = found.iter().map(|(_, _, o, _, _)| o.clone()).collect();
            if distinct.len() > 1 {
                let options: BTreeSet<_> =
                    found.iter().filter_map(|(_, _, _, _, s3)| s3.clone()).collect();
                reasons.push(ambiguity("третьего", &options, distinct.len()));
                continue;
            }
        }

        for (id, number, outcome, _, _) in found {
            position_ids.push(id);
            all_positions.push((number, outcome));
        }
    }

    if position_ids.is_empty() {
        return Ok(ClassifyResult {
            outcome: None,
            positions: vec![],
            services: vec![],
            informational: vec![],
            reason: Some(reasons.join("; ")),
        });
    }

    // Итоговый тип — от самой тяжёлой позиции. Тяжесть закодирована
    // номером: у 1050101 последняя цифра 1 (открытое пламя, «пожар»),
    // у 1050102 — 2 (дым, «задымление»). Меньше номер — тяжелее.
    all_positions.sort_by(|a, b| a.0.cmp(&b.0));
    let outcome = all_positions.iter().find_map(|(_, o)| o.clone());

    let matrix: Vec<MatrixRow> = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
        "SELECT ps.service_id, s.title, ps.condition, ps.value,
                cm.kind::text, cm.question_id, cm.match_value
           FROM classifier_position_services ps
           JOIN services s ON s.id = ps.service_id
           -- Только службы окна «Добавьте службы» боевой системы. Столбец
           -- классификатора, которого в окне нет («Территориальные ОИВ»),
           -- не подбирается и в эталон разбора не входит: курсант не смог
           -- бы добавить такую службу сам, а разбор штрафовал бы за неё.
           -- Эталон считается этой же функцией, так что правило одно
           JOIN service_catalog sc ON sc.service_id = s.id
           LEFT JOIN condition_mapping cm ON cm.condition = ps.condition
          WHERE ps.position_id = ANY($1)",
    )
    .bind(&position_ids)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(service_id, title, condition, value, kind, cond_question, cond_value)| MatrixRow {
        service_id,
        title,
        condition,
        value: value.unwrap_or_default(),
        // Условие, которого нет в справочнике, считаем базовым.
        // Раньше здесь было «always», и движок выбирал у СМП подколонку
        // «выбран признак Пострадавшие» даже без пострадавших.
        kind: kind.unwrap_or_else(|| "base".into()),
        cond_question,
        cond_value,
    })
    .collect();

    // Группируем по службе: у одной службы несколько подколонок с разными условиями
    let mut by_service: BTreeMap<String, Vec<&MatrixRow>> = BTreeMap::new();
    for row in &matrix {
        by_service.entry(row.service_id.clone()).or_default().push(row);
    }

    let mut services = Vec::new();
    let mut informational = Vec::new();

    for (_, rows) in by_service {
        let chosen = pick_row(&rows, answers);
        let Some(row) = chosen else { continue };
        if row.value.is_empty() {
            continue;
        }

        let responding = !INFORMATIONAL_VALUES.contains(&row.value.trim());
        let item = NotifiedService {
            service_id: row.service_id.clone(),
            title: row.title.clone(),
            value: row.value.clone(),
            responding,
            condition: row.condition.clone(),
        };

        if responding { services.push(item) } else { informational.push(item) }
    }

    services.sort_by(|a, b| a.title.cmp(&b.title));
    informational.sort_by(|a, b| a.title.cmp(&b.title));

    Ok(ClassifyResult {
        outcome,
        positions: all_positions.into_iter().map(|(n, _)| n).collect(),
        services,
        informational,
        reason: None,
    })
}

/// Выбирает подколонку службы: сначала сработавший предикат,
/// потом служебная, потом базовая.
fn pick_row<'a>(
    rows: &[&'a MatrixRow],
    answers: &HashMap<String, Vec<String>>,
) -> Option<&'a MatrixRow> {
    // Порядок строк из базы не гарантирован, поэтому сортируем:
    // иначе при нескольких подходящих подколонках результат плавает.
    let mut rows: Vec<&MatrixRow> = rows.to_vec();
    rows.sort_by(|a, b| a.condition.cmp(&b.condition));

    let predicate_hit = rows.iter().find(|r| {
        r.kind == "predicate"
            && match (&r.cond_question, &r.cond_value) {
                (Some(q), Some(v)) => answers.get(q).is_some_and(|a| a.contains(v)),
                _ => false,
            }
    });
    if let Some(r) = predicate_hit {
        return Some(r);
    }

    rows.iter()
        .find(|r| r.kind == "always")
        .or_else(|| rows.iter().find(|r| r.kind == "base"))
        .copied()
}
