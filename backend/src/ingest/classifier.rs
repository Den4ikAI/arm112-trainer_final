//! Разбор XLSX Единого классификатора происшествий.
//!
//! Структура файла (проверена на обеих версиях из материалов задачи):
//!
//! ```text
//! строка 1   названия служб, начиная с колонки 12 (59 именованных)
//! строка 2   подписи служебных колонок и условия оповещения
//! строка 3   продолжение условий для части служб
//! строка 4+  данные
//! ```
//!
//! Колонки служебной части:
//! `0=Г 1=п1 2=п2 3=п3 4=Номер 5=Группа(стат.) 6..8=Признаки
//!  9=Доп.признаки 10=Итоговый тип 11=ЕКП35 12=Главная служба 13+=службы`
//!
//! Служба занимает **диапазон** колонок: от своего заголовка до следующего.
//! Внутри диапазона каждая подколонка — отдельное условие оповещения
//! («признак не выбран», «угроза людям», «пострадавшие»). Отсюда 104
//! колонки при 59 службах.
//!
//! Строка считается заголовком группы, если колонка «Г» пуста, а в «Номер»
//! и «Группа» что-то есть.

use anyhow::{bail, Context, Result};
use calamine::{open_workbook_auto, Data, Range, Reader};
use std::collections::BTreeMap;
use std::path::Path;

/// Первая колонка, в которой начинаются службы.
const FIRST_SERVICE_COL: usize = 12;

/// Индексы служебных колонок.
mod col {
    pub const G: usize = 0;
    pub const NUMBER: usize = 4;
    pub const GROUP: usize = 5;
    pub const SIGN1: usize = 6;
    pub const SIGN2: usize = 7;
    pub const SIGN3: usize = 8;
    pub const EXTRA: usize = 9;
    pub const OUTCOME: usize = 10;
    pub const EKP: usize = 11;
}

/// Служба и диапазон её колонок.
#[derive(Debug, Clone)]
pub struct ServiceColumn {
    pub id: String,
    pub title: String,
    /// Подколонки: индекс и условие оповещения.
    pub conditions: Vec<(usize, String)>,
}

/// Одна позиция классификатора со всеми оповещениями.
#[derive(Debug, Clone)]
pub struct Position {
    pub number: String,
    pub group_no: i32,
    pub group_title: String,
    pub sign1: Option<String>,
    pub sign2: Option<String>,
    pub sign3: Option<String>,
    pub extra_signs: Option<String>,
    pub outcome_type: Option<String>,
    pub ekp_type: Option<String>,
    /// (service_id, условие, значение ячейки)
    pub services: Vec<(String, String, String)>,
}

#[derive(Debug)]
pub struct Parsed {
    pub services: Vec<ServiceColumn>,
    pub positions: Vec<Position>,
    /// Номер группы → название, для отчёта после импорта.
    pub groups: BTreeMap<i32, String>,
}

pub fn parse(path: &Path) -> Result<Parsed> {
    let mut wb = open_workbook_auto(path)
        .with_context(|| format!("не удалось открыть {}", path.display()))?;

    let sheet = wb
        .worksheet_range_at(0)
        .context("в книге нет листов")?
        .context("не удалось прочитать первый лист")?;

    let services = parse_service_columns(&sheet)?;
    let (positions, groups) = parse_positions(&sheet, &services)?;

    Ok(Parsed { services, positions, groups })
}

fn text(sheet: &Range<Data>, row: usize, col: usize) -> String {
    sheet
        .get((row, col))
        .map(|c| c.to_string().replace('\n', " ").trim().to_string())
        .unwrap_or_default()
}

fn opt(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}

/// Разбирает шапку: какие службы есть и какие колонки им принадлежат.
fn parse_service_columns(sheet: &Range<Data>) -> Result<Vec<ServiceColumn>> {
    let width = sheet.width();

    // Индексы колонок, где в первой строке стоит название службы
    let heads: Vec<usize> = (FIRST_SERVICE_COL..width)
        .filter(|&i| !text(sheet, 0, i).is_empty())
        .collect();

    if heads.is_empty() {
        bail!("в шапке не найдено ни одной службы — файл не похож на классификатор");
    }

    let mut out = Vec::new();

    for (n, &start) in heads.iter().enumerate() {
        let title = text(sheet, 0, start);

        // «Главная служба» — служебная колонка с кодом основной службы,
        // а не участник оповещения
        if title.eq_ignore_ascii_case("Главная служба") {
            continue;
        }

        let end = heads.get(n + 1).copied().unwrap_or(width);

        let conditions = (start..end)
            .map(|j| {
                // Условие пишут во второй строке, у части служб — в третьей
                let cond = match text(sheet, 1, j) {
                    s if !s.is_empty() => s,
                    _ => text(sheet, 2, j),
                };
                (j, cond)
            })
            .collect();

        out.push(ServiceColumn {
            id: super::slug::service_id(&title),
            title,
            conditions,
        });
    }

    Ok(out)
}

/// Разбирает строки данных. Заголовки групп задают контекст для
/// последующих позиций.
fn parse_positions(
    sheet: &Range<Data>,
    services: &[ServiceColumn],
) -> Result<(Vec<Position>, BTreeMap<i32, String>)> {
    let mut positions = Vec::new();
    let mut groups = BTreeMap::new();

    // Заголовок группы, который ещё не привязан к номеру.
    //
    // В файле 23 заголовка из 24 несут номер группы в колонке «Номер»,
    // а заголовок «БПЛА» его не имеет. Наивный парсер на этом молча
    // отправляет всю группу в предыдущую — а это 204 позиции, включая
    // то самое БПЛА, ради которого нужна дельта версий.
    //
    // Поэтому номер группы берём из колонки «Г» строки данных: она
    // заполнена всегда и является источником истины. Заголовок лишь
    // даёт название, которое привязывается к первому же номеру после него.
    let mut pending_title: Option<String> = None;
    let mut current_group_title = String::new();
    let mut current_group_no = 0i32;

    // Данные начинаются с четвёртой строки
    for row in 3..sheet.height() {
        let g = text(sheet, row, col::G);
        let number = text(sheet, row, col::NUMBER);

        if g.is_empty() {
            let group_title = text(sheet, row, col::GROUP);
            if !group_title.is_empty() {
                pending_title = Some(group_title);
            }
            continue;
        }

        if number.is_empty() {
            continue;
        }

        let group_no: i32 = g
            .trim()
            .parse()
            .with_context(|| format!("строка {}: колонка «Г» = {g:?}, ожидалось число", row + 1))?;

        if group_no != current_group_no {
            current_group_no = group_no;
            if let Some(title) = pending_title.take() {
                current_group_title = title;
            }
            groups.insert(group_no, current_group_title.clone());
        }

        let mut svc = Vec::new();
        for s in services {
            for (idx, cond) in &s.conditions {
                let value = text(sheet, row, *idx);
                if !value.is_empty() {
                    svc.push((s.id.clone(), cond.clone(), value));
                }
            }
        }

        positions.push(Position {
            number,
            group_no,
            group_title: current_group_title.clone(),
            sign1: opt(text(sheet, row, col::SIGN1)),
            sign2: opt(text(sheet, row, col::SIGN2)),
            sign3: opt(text(sheet, row, col::SIGN3)),
            extra_signs: opt(text(sheet, row, col::EXTRA)),
            outcome_type: opt(text(sheet, row, col::OUTCOME)),
            ekp_type: opt(text(sheet, row, col::EKP)),
            services: svc,
        });
    }

    Ok((positions, groups))
}

// ─── Запись в базу ───────────────────────────────────────────────────

use sqlx::PgPool;
use uuid::Uuid;

pub struct ImportReport {
    pub version_id: Uuid,
    pub services: usize,
    pub positions: usize,
    pub groups: usize,
    pub notifications: usize,
}

/// Загружает разобранный файл в базу как новую версию классификатора.
///
/// Версия создаётся неактивной: включить её — отдельное действие,
/// потому что активация запускает пересчёт затронутых сценариев
/// и переаттестацию.
pub async fn store(pool: &PgPool, parsed: &Parsed, name: &str, source: &str) -> Result<ImportReport> {
    let mut tx = pool.begin().await?;

    // Службы общие для всех версий: если появилась новая, добавляем,
    // существующим обновляем название
    for s in &parsed.services {
        sqlx::query(
            "INSERT INTO services (id, title) VALUES ($1, $2)
             ON CONFLICT (id) DO UPDATE SET title = EXCLUDED.title",
        )
        .bind(&s.id)
        .bind(&s.title)
        .execute(&mut *tx)
        .await?;
    }

    let version_id: Uuid = sqlx::query_scalar(
        "INSERT INTO classifier_versions (name, source_file, positions_count)
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(name)
    .bind(source)
    .bind(parsed.positions.len() as i32)
    .fetch_one(&mut *tx)
    .await?;

    let mut notifications = 0usize;

    for p in &parsed.positions {
        let position_id: Uuid = sqlx::query_scalar(
            "INSERT INTO classifier_positions
               (version_id, number, group_no, group_title,
                sign1, sign2, sign3, extra_signs, outcome_type, ekp_type)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
        )
        .bind(version_id)
        .bind(&p.number)
        .bind(p.group_no)
        .bind(&p.group_title)
        .bind(&p.sign1)
        .bind(&p.sign2)
        .bind(&p.sign3)
        .bind(&p.extra_signs)
        .bind(&p.outcome_type)
        .bind(&p.ekp_type)
        .fetch_one(&mut *tx)
        .await?;

        for (service_id, condition, value) in &p.services {
            // Одна служба может встретиться в позиции несколько раз
            // с разными условиями — это нормально. Одинаковые пары
            // условие+служба в пределах позиции схлопываем.
            sqlx::query(
                "INSERT INTO classifier_position_services
                   (position_id, service_id, condition, value)
                 VALUES ($1,$2,$3,$4)
                 ON CONFLICT (position_id, service_id, condition) DO NOTHING",
            )
            .bind(position_id)
            .bind(service_id)
            .bind(condition)
            .bind(value)
            .execute(&mut *tx)
            .await?;
            notifications += 1;
        }
    }

    tx.commit().await?;

    Ok(ImportReport {
        version_id,
        services: parsed.services.len(),
        positions: parsed.positions.len(),
        groups: parsed.groups.len(),
        notifications,
    })
}
