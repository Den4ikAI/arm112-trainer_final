//! Построение опросных карт из классификатора.
//!
//! Опросная карта боевой системы — дерево признаков, а не плоский список:
//!
//! ```text
//! Признак 1 (где)   →  Признак 2 (объект)  →  Признак 3  →  итоговый тип
//! жилой дом            квартира               открытое пламя  пожар: квартира
//! жилой дом            квартира               дым             задымление: квартира
//! на улице             трава                  открытое пламя  пожар: трава
//! ```
//!
//! Скриншоты боевой системы дают точные формулировки, но только для той
//! ветки, что была открыта в момент съёмки. Классификатор даёт полную
//! структуру. Поэтому структуру берём из него, а формулировки накладываем
//! со скриншотов там, где они есть.
//!
//! Сочинять варианты нельзя: если для группы нет данных, карта не строится.

use anyhow::Result;
use sqlx::PgPool;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Позиции с этим значением первого признака не показываются оператору
/// и в опросную карту не попадают.
const HIDDEN_FROM_OPERATOR: &str = "Не отображается оператору 112";

/// Узел дерева признаков.
#[derive(Debug, Default)]
pub struct Branch {
    /// Второй признак → набор третьих признаков.
    pub objects: BTreeMap<String, Vec<String>>,
}

#[derive(Debug)]
pub struct DerivedCard {
    pub group_no: i32,
    pub group_title: String,
    /// Первый признак → его ветка.
    pub branches: BTreeMap<String, Branch>,
    /// Позиции, пропущенные как скрытые от оператора.
    pub skipped_hidden: usize,
}

impl DerivedCard {
    /// Сколько всего комбинаций описывает карта.
    pub fn combinations(&self) -> usize {
        self.branches
            .values()
            .flat_map(|b| b.objects.values())
            .map(|v| v.len().max(1))
            .sum()
    }

    /// Узлы, у которых третий уровень предлагает выбор из нескольких
    /// вариантов. Это НЕ проблема: так устроен обычный вопрос опросной
    /// карты — «открытое пламя» или «дым».
    ///
    /// Отдельно стоит смотреть на варианты, где вариантов больше двух:
    /// там формулировка вопроса из классификатора может оказаться
    /// неудобной для оператора, и стоит сверить со скриншотом, если он есть.
    pub fn choice_nodes(&self) -> Vec<(String, String, Vec<String>)> {
        let mut out = Vec::new();
        for (s1, branch) in &self.branches {
            for (s2, s3s) in &branch.objects {
                if s3s.len() > 1 {
                    out.push((s1.clone(), s2.clone(), s3s.clone()));
                }
            }
        }
        out
    }
}

/// Собирает дерево признаков группы из позиций классификатора.
pub async fn derive(pool: &PgPool, version_id: Uuid, group_no: i32) -> Result<Option<DerivedCard>> {
    let rows: Vec<(Option<String>, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT sign1, sign2, sign3, group_title
           FROM classifier_positions
          WHERE version_id = $1 AND group_no = $2
          ORDER BY number",
    )
    .bind(version_id)
    .bind(group_no)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let group_title = rows[0].3.clone();
    let mut branches: BTreeMap<String, Branch> = BTreeMap::new();
    let mut skipped_hidden = 0usize;

    for (s1, s2, s3, _) in rows {
        let Some(s1) = s1.filter(|s| !s.is_empty()) else { continue };

        if s1 == HIDDEN_FROM_OPERATOR {
            skipped_hidden += 1;
            continue;
        }

        let branch = branches.entry(s1).or_default();

        // Второй признак может отсутствовать: тогда ветка одноуровневая
        let key = s2.filter(|s| !s.is_empty()).unwrap_or_default();
        let slot = branch.objects.entry(key).or_default();

        if let Some(s3) = s3.filter(|s| !s.is_empty()) {
            if !slot.contains(&s3) {
                slot.push(s3);
            }
        }
    }

    // Ветка без объектов и без третьих признаков бесполезна
    branches.retain(|_, b| !b.objects.is_empty());

    if branches.is_empty() {
        return Ok(None);
    }

    Ok(Some(DerivedCard { group_no, group_title, branches, skipped_hidden }))
}

/// Подписи вопросов выведенной карты — заголовки столбцов классификатора
/// («112 - Признак.1 (тип происшествия)», «112-Признак.2»,
/// «112-Признак.3»), без служебного префикса «112».
const LABELS: [&str; 3] = ["Признак 1 (тип происшествия)", "Признак 2", "Признак 3"];

/// Идентификатор вопроса выведенной карты.
///
/// Ответы карточки лежат одним словарём на все выбранные типы, поэтому
/// идентификатор включает тип: у ДТП и «Человека в опасности» первый
/// признак разный, и общий ключ `p1` смешал бы их ответы.
pub fn derived_question_id(type_id: &str, level: usize) -> String {
    format!("{type_id}-p{level}")
}

/// Разложить выведенные карты по таблицам опросных карт.
///
/// Карты кладутся туда же, где лежат снятые со скриншотов, — тогда
/// справочник, классификация, подписи в разборе и подготовка вызовов
/// работают с ними без отдельных веток. Снятые со скриншотов карты
/// не трогаются: у них дословные формулировки боевой системы.
///
/// Выведенные карты пересобираются целиком: при смене версии
/// классификатора дерево признаков могло поменяться.
///
/// Возвращает, сколько карт построено.
pub async fn materialise(pool: &PgPool, version_id: Uuid) -> Result<usize> {
    let mut tx = pool.begin().await?;

    // Два пересбора одновременно столкнулись бы на ключах: каждый
    // удаляет прежние карты и вставляет новые. Такое бывает и при
    // двух активациях подряд, и в параллельных проверках
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('survey.materialise'))")
        .execute(&mut *tx)
        .await?;

    // Каскад снимет и варианты, и сопоставления с признаками
    sqlx::query("DELETE FROM survey_questions WHERE derived")
        .execute(&mut *tx)
        .await?;

    let types: Vec<(String, i32)> = sqlx::query_as(
        "SELECT t.id, t.group_no FROM incident_types t
          WHERE t.group_no IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM survey_questions q WHERE q.type_id = t.id)
          ORDER BY t.id",
    )
    .fetch_all(&mut *tx)
    .await?;

    // Дерево одной группы строим один раз: на группу 23 завязаны восемь типов
    let mut trees: BTreeMap<i32, Option<DerivedCard>> = BTreeMap::new();
    let mut built = 0usize;

    for (type_id, group_no) in types {
        if let std::collections::btree_map::Entry::Vacant(slot) = trees.entry(group_no) {
            slot.insert(derive(pool, version_id, group_no).await?);
        }
        let Some(card) = trees.get(&group_no).and_then(Option::as_ref) else {
            continue;
        };

        // Уровень 1: все первые признаки группы
        let level1: Vec<(String, Vec<String>)> =
            card.branches.keys().map(|s1| (s1.clone(), Vec::new())).collect();

        // Уровень 2: второй признак и первые, под которыми он встречается.
        // Пустой ключ — позиция без второго признака, как вариант он не
        // нужен: такую позицию выбирают, не отвечая на вопрос
        let mut level2: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // Уровень 3: третий признак и вторые, под которыми он встречается
        let mut level3: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (s1, branch) in &card.branches {
            for (s2, s3s) in &branch.objects {
                if !s2.is_empty() {
                    let parents = level2.entry(s2.clone()).or_default();
                    if !parents.contains(s1) {
                        parents.push(s1.clone());
                    }
                }
                for s3 in s3s {
                    let parents = level3.entry(s3.clone()).or_default();
                    if !s2.is_empty() && !parents.contains(s2) {
                        parents.push(s2.clone());
                    }
                }
            }
        }

        let levels: [Vec<(String, Vec<String>)>; 3] = [
            level1,
            level2.into_iter().collect(),
            level3.into_iter().collect(),
        ];

        for (i, options) in levels.iter().enumerate() {
            if options.is_empty() {
                continue;
            }
            let level = i + 1;
            let qid = derived_question_id(&type_id, level);
            let depends_on = (level > 1).then(|| derived_question_id(&type_id, level - 1));

            sqlx::query(
                "INSERT INTO survey_questions
                     (type_id, id, label, kind, sort_order, derived, depends_on)
                 VALUES ($1, $2, $3, 'single', $4, true, $5)",
            )
            .bind(&type_id)
            .bind(&qid)
            .bind(LABELS[i])
            .bind(level as i32)
            .bind(&depends_on)
            .execute(&mut *tx)
            .await?;

            for (order, (value, parents)) in options.iter().enumerate() {
                // Третий признак без второго (у ветки без объектов)
                // от второго уровня не зависит
                let parents = (!parents.is_empty()).then_some(parents);
                sqlx::query(
                    "INSERT INTO survey_options (type_id, question_id, value, sort_order, parents)
                     VALUES ($1, $2, $3, $4, $5)",
                )
                .bind(&type_id)
                .bind(&qid)
                .bind(value)
                .bind(order as i32 + 1)
                .bind(parents)
                .execute(&mut *tx)
                .await?;

                // Вариант и есть признак классификатора, дословно
                sqlx::query(
                    "INSERT INTO sign_mapping
                         (type_id, question_id, option_value, sign_level, sign_text)
                     VALUES ($1, $2, $3, $4, $3)",
                )
                .bind(&type_id)
                .bind(&qid)
                .bind(value)
                .bind(level as i16)
                .execute(&mut *tx)
                .await?;
            }
        }
        built += 1;
    }

    supplement_screenshot_cards(&mut tx, pool, version_id, &mut trees).await?;

    tx.commit().await?;
    Ok(built)
}

/// Сравнение признаков без учёта регистра и пробелов по краям:
/// «Аэропорт» и «аэропорт» — один признак.
fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Дополнить карты, снятые со скриншотов, уровнями, которых на кадрах
/// не было.
///
/// Кадр показывает одну открытую ветку: у 104 — только «Признаки
/// происшествия», у «Взрыва» — только «Где взрыв», у 101 для «Здание /
/// объект» — вопросы про жилой дом. Второй и третий признаки классификатора
/// под этими вариантами оператор выбрать не мог, и позицию было не
/// определить. Дополняем из классификатора: вопрос «Признак 2 (вариант)»
/// появляется, когда выбран вариант первого уровня, «Признак 3» зависит
/// от второго. Формулировки со скриншотов не трогаем; дописанные вопросы
/// помечены как выведенные и пересобираются вместе с остальными.
///
/// Дописываем только непокрытое: если второй признак уже есть в карте
/// (у 101 «квартира», «балкон» во внутридомовых объектах), второй раз
/// его не предлагаем. Вариант, который сам задаёт второй признак
/// («Опасный объект» → «опасный»), не дополняем.
async fn supplement_screenshot_cards(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    pool: &PgPool,
    version_id: Uuid,
    trees: &mut BTreeMap<i32, Option<DerivedCard>>,
) -> Result<usize> {
    use std::collections::BTreeSet;

    let types: Vec<(String, i32)> = sqlx::query_as(
        "SELECT t.id, t.group_no FROM incident_types t
          WHERE t.group_no IS NOT NULL
            AND EXISTS (SELECT 1 FROM survey_questions q
                         WHERE q.type_id = t.id AND NOT q.derived)
          ORDER BY t.id",
    )
    .fetch_all(&mut **tx)
    .await?;

    let mut added = 0usize;
    for (type_id, group_no) in types {
        if let std::collections::btree_map::Entry::Vacant(slot) = trees.entry(group_no) {
            slot.insert(derive(pool, version_id, group_no).await?);
        }
        let Some(card) = trees.get(&group_no).and_then(Option::as_ref) else {
            continue;
        };

        let mappings: Vec<(String, String, i16, String, i32)> = sqlx::query_as(
            "SELECT m.question_id, m.option_value, m.sign_level, m.sign_text, q.sort_order
               FROM sign_mapping m
               JOIN survey_questions q ON q.type_id = m.type_id AND q.id = m.question_id
              WHERE m.type_id = $1 AND NOT q.derived
              ORDER BY q.sort_order, m.question_id, m.option_value",
        )
        .bind(&type_id)
        .fetch_all(&mut **tx)
        .await?;

        let covered = |level: i16| -> BTreeSet<String> {
            mappings.iter().filter(|m| m.2 == level).map(|m| norm(&m.3)).collect()
        };
        let covered2 = covered(2);
        let covered3 = covered(3);
        // Варианты, которые сами задают второй признак
        let fixed2: BTreeSet<(String, String)> = mappings
            .iter()
            .filter(|m| m.2 == 2)
            .map(|m| (m.0.clone(), m.1.clone()))
            .collect();

        let mut n = 0usize;
        for (qid, option, _, sign, sort) in mappings.iter().filter(|m| m.2 == 1) {
            if fixed2.contains(&(qid.clone(), option.clone())) {
                continue;
            }
            let Some(branch) = card
                .branches
                .iter()
                .find(|(s1, _)| norm(s1) == norm(sign))
                .map(|(_, b)| b)
            else {
                continue;
            };
            let missing: Vec<&String> = branch
                .objects
                .keys()
                .filter(|s2| !s2.is_empty() && !covered2.contains(&norm(s2)))
                .collect();
            if missing.is_empty() {
                continue;
            }

            n += 1;
            let p2 = format!("{type_id}-x{n}-p2");
            let show_when = serde_json::json!([{ "question": qid, "values": [option] }]);
            sqlx::query(
                "INSERT INTO survey_questions
                     (type_id, id, label, kind, sort_order, derived, depends_on, show_when)
                 VALUES ($1, $2, $3, 'single', $4, true, NULL, $5)",
            )
            .bind(&type_id)
            .bind(&p2)
            .bind(format!("Признак 2 ({option})"))
            .bind(sort)
            .bind(&show_when)
            .execute(&mut **tx)
            .await?;

            let mut level3: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (order, s2) in missing.iter().enumerate() {
                insert_option(tx, &type_id, &p2, s2, order, None, 2).await?;
                for s3 in &branch.objects[*s2] {
                    if covered3.contains(&norm(s3)) {
                        continue;
                    }
                    let parents = level3.entry(s3.clone()).or_default();
                    if !parents.contains(s2) {
                        parents.push((*s2).clone());
                    }
                }
            }

            if !level3.is_empty() {
                let p3 = format!("{type_id}-x{n}-p3");
                sqlx::query(
                    "INSERT INTO survey_questions
                         (type_id, id, label, kind, sort_order, derived, depends_on)
                     VALUES ($1, $2, $3, 'single', $4, true, $5)",
                )
                .bind(&type_id)
                .bind(&p3)
                .bind(format!("Признак 3 ({option})"))
                .bind(sort)
                .bind(&p2)
                .execute(&mut **tx)
                .await?;
                for (order, (s3, parents)) in level3.iter().enumerate() {
                    insert_option(tx, &type_id, &p3, s3, order, Some(parents), 3).await?;
                }
            }
            added += 1;
        }
    }
    Ok(added)
}

/// Вариант выведенного вопроса и его сопоставление с признаком
/// классификатора: вариант и есть признак, дословно.
async fn insert_option(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    type_id: &str,
    question_id: &str,
    value: &str,
    order: usize,
    parents: Option<&Vec<String>>,
    level: i16,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO survey_options (type_id, question_id, value, sort_order, parents)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(type_id)
    .bind(question_id)
    .bind(value)
    .bind(order as i32 + 1)
    .bind(parents)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO sign_mapping (type_id, question_id, option_value, sign_level, sign_text)
         VALUES ($1, $2, $3, $4, $3)",
    )
    .bind(type_id)
    .bind(question_id)
    .bind(value)
    .bind(level)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
