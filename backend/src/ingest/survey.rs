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
