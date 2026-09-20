//! Сверка снятых со скриншотов опросных карт с классификатором.
//!
//! Три карты — 101, 104 и Взрыв — воспроизведены дословно по кадрам
//! боевой системы. Утилита показывает, где их варианты сходятся
//! с признаками классификатора, а где расходятся, и почему.
//!
//! Расхождение не обязательно ошибка: на кадре была открыта одна ветка
//! дерева, остальные просто не попали в объектив. Но каждое расхождение
//! должен посмотреть человек — иначе мы либо потеряем вариант,
//! либо начнём сочинять.
//!
//! ```bash
//! cargo run --bin check_cards
//! ```

use anyhow::{Context, Result};
use arm112::{config::Config, store};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Варианты снятых карт. Источник — refs/СКРИНШОТ КАРТОЧКИ 112ГСИ.docx,
/// изображения 2, 4, 7, 9. Формулировки дословные.
const CARDS: &[(&str, i32, &[(&str, &[&str])])] = &[
    (
        "Происшествие 101",
        1,
        &[
            ("Где", &["Улица", "Транспорт", "Дом", "Здание / объект", "Опасный объект"]),
            ("Признак пожара (дом)", &["Открытое пламя / Дым", "Запах гари", "Сработала пожарная сигнализация"]),
            ("Дом (пламя, дым)", &["Дом многоквартирный", "Дом частный", "Дача", "Сарай / бытовка / хоз. постройка", "Выселенное здание"]),
            ("Внутридомовые объекты (пламя, дым)", &[
                "Квартира", "Балкон", "Газовая колонка", "Газовая плита", "Лифт",
                "Мусоропровод", "Подъезд", "Счетчик электричества", "Электрическая проводка",
                "Электрощит", "Лестничная клетка", "Подвал", "Прочие внутридомовые объекты", "Крыша",
            ]),
        ],
    ),
    (
        "Происшествие 104",
        13,
        &[("Признаки происшествия", &[
            "Запах газа вне помещения (на улице)",
            "Запах газа в помещении (в квартире, в доме)",
            "Нарушение в работе газового оборудования",
            "Повреждение газопровода",
            "Повышенное давление газа",
        ])],
    ),
    (
        "П: Взрыв",
        3,
        &[("Где взрыв", &[
            "Здание / Объект",
            "Транспорт",
            "Звуки похожие на взрыв, что взорвалось сообщить не может",
        ])],
    ),
];

fn norm(s: &str) -> String {
    s.to_lowercase()
        .replace('ё', "е")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::from_env()?;
    let pool = store::connect(&config.database_url).await?;

    let version_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY imported_at DESC LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?
    .context("нет ни одной версии классификатора")?;

    let mut total_hit = 0usize;
    let mut total = 0usize;
    let mut split_candidates = Vec::new();

    for (title, group_no, questions) in CARDS {
        let signs: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT s FROM (
               SELECT unnest(ARRAY[sign1, sign2, sign3]) AS s
                 FROM classifier_positions
                WHERE version_id = $1 AND group_no = $2
                  AND coalesce(sign1,'') <> 'Не отображается оператору 112'
             ) t WHERE s IS NOT NULL AND trim(s) <> ''",
        )
        .bind(version_id)
        .bind(group_no)
        .fetch_all(&pool)
        .await?;

        let normed: BTreeSet<String> = signs.iter().map(|s| norm(s)).collect();

        println!("\n{title}  (группа {group_no}, признаков в классификаторе: {})", signs.len());
        println!("{}", "─".repeat(70));

        for (question, options) in *questions {
            println!("  {question}");
            for opt in *options {
                total += 1;
                let n = norm(opt);

                if normed.contains(&n) {
                    total_hit += 1;
                    println!("      ✓ {opt}");
                    continue;
                }

                // Вариант вида «Открытое пламя / Дым» на кадре может
                // соответствовать двум отдельным признакам классификатора
                let parts: Vec<&str> = opt.split('/').map(str::trim).filter(|p| !p.is_empty()).collect();
                if parts.len() > 1 {
                    let matched: Vec<&&str> =
                        parts.iter().filter(|p| normed.contains(&norm(p))).collect();
                    if matched.len() == parts.len() {
                        total_hit += 1;
                        split_candidates.push((title, opt, parts.clone()));
                        println!("      ≈ {opt}  → в классификаторе это {} отдельных признака", parts.len());
                        continue;
                    }
                }

                println!("      ✗ {opt}");
            }
        }
    }

    println!("\n{}", "═".repeat(70));
    println!("Сошлось: {total_hit} из {total}");

    if !split_candidates.is_empty() {
        println!("\nТребует решения человека — один вариант на кадре против");
        println!("нескольких признаков в классификаторе:");
        for (card, opt, parts) in &split_candidates {
            println!("  {card}: «{opt}» → {}", parts.join(" + "));
        }
        println!("\nРазделить на два варианта нельзя: на кадре боевой системы");
        println!("это одна кнопка. Как она разводится — по скриншоту не видно.");
    }

    println!("\nНесошедшиеся варианты делятся на три вида, и все три —");
    println!("не повод что-либо сочинять:");
    println!();
    println!("  1. Другая формулировка того же: «Улица» против «на улице»,");
    println!("     «Дом» против «жилой дом». Нужна таблица соответствий.");
    println!();
    println!("  2. Другая ветка дерева, не попавшая в кадр. Полная структура");
    println!("     берётся из классификатора — см. derive_cards.");
    println!();
    println!("  3. Информационный признак, в классификации не участвует.");
    println!("     Пример: «Дом многоквартирный» в группе 1 не встречается,");
    println!("     а на сохранённой карточке ДДС он показан при том, что");
    println!("     класс определён как «пожар: квартира» — то есть по паре");
    println!("     квартира + открытое пламя. Такие поля едут в службы как");
    println!("     информация, но на список оповещения не влияют.");

    Ok(())
}
