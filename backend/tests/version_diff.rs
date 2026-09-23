//! Сравнение версий классификатора.
//!
//! Критерий приёмки задачи задан точно: дифф `v046_11` → `v046_24`
//! на настоящих файлах должен дать восемь новых служб, две новые
//! позиции и исправленную опечатку. Тест проверяет ровно это.
//!
//! Запуск: `cargo test --test version_diff`

use arm112::domain::version_diff::{diff, impact};
use sqlx::PgPool;
use uuid::Uuid;

/// Куда ходят проверки.
///
/// Раньше здесь по умолчанию стояла база стенда `arm112`, а `.env` не
/// читался вовсе — и прогон накатывал на стенд миграции. Новая миграция
/// из ветки попадала туда раньше кода, и сервер стенда при следующем
/// запуске падал с «migration … was previously applied but is missing».
/// Теперь — как в остальных проверках: отдельная база из `.env`.
async fn pool() -> Option<PgPool> {
    let _ = dotenvy::dotenv();
    let url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()?;
    let pool = PgPool::connect(&url).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    Some(pool)
}

/// Идентификаторы двух версий из материалов задачи.
async fn versions(pool: &PgPool) -> Option<(Uuid, Uuid)> {
    let old = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM classifier_versions WHERE name LIKE 'v046_11%' LIMIT 1")
        .fetch_optional(pool).await.ok()??;
    let new = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM classifier_versions WHERE name LIKE 'v046_24%' LIMIT 1")
        .fetch_optional(pool).await.ok()??;
    Some((old, new))
}

/// Восемь служб, появившихся между версиями. Список из условия задачи.
#[tokio::test]
async fn новые_службы_совпадают_с_условием() {
    let Some(pool) = pool().await else { return };
    let Some((old, new)) = versions(&pool).await else { return };

    let d = diff(&pool, old, new).await.unwrap();
    let mut titles: Vec<&str> = d.services_added.iter().map(|s| s.title.as_str()).collect();
    titles.sort();

    assert_eq!(titles, vec![
        "ГКУ Организатор перевозок",
        "ГПБУ Мосэкомониторинг",
        // «Депортамент» — опечатка в самом классификаторе, не наша.
        // Исправлять её здесь нельзя: тогда наш справочник разойдётся
        // с тем, что у заказчика
        "Депортамент гражданского строительства",
        "Министерство обороны РХБЗ",
        "ООО Ситиэнерго",
        "РЖД (МосковскаяЖД)",
        "ЦУКБ Министерство обороны",
        "ЦУКБ.БПЛА Министерство обороны",
    ]);
}

/// Две новые позиции: БПЛА и «Прочие (от 102)».
#[tokio::test]
async fn новые_позиции_совпадают_с_условием() {
    let Some(pool) = pool().await else { return };
    let Some((old, new)) = versions(&pool).await else { return };

    let d = diff(&pool, old, new).await.unwrap();
    let mut numbers: Vec<&str> = d.positions_added.iter().map(|p| p.number.as_str()).collect();
    numbers.sort();
    assert_eq!(numbers, vec!["23060001", "24120200"]);

    let bpla = d.positions_added.iter().find(|p| p.number == "24120200").unwrap();
    assert_eq!(bpla.group_title, "БПЛА");
    assert_eq!(bpla.outcome_type.as_deref(), Some("БВС Регион"));
}

/// Единственная правка формулировки во всём классификаторе.
#[tokio::test]
async fn исправленная_опечатка_видна_в_диффе() {
    let Some(pool) = pool().await else { return };
    let Some((old, new)) = versions(&pool).await else { return };

    let d = diff(&pool, old, new).await.unwrap();
    let p = d.positions_changed.iter()
        .find(|p| p.number == "13010200")
        .expect("позиция 13010200 не попала в изменённые");

    let f = p.fields.iter().find(|f| f.field == "outcomeType").expect("тип не изменился");
    assert_eq!(f.before.as_deref(), Some("Запах бытового газа в метро (тонелле)"));
    assert_eq!(f.after.as_deref(), Some("Запах бытового газа в метро (тоннеле)"));

    // И она же — единственная во всём справочнике
    assert_eq!(d.summary.positions_retitled, 1);
}

/// Изменения формулировок и изменения списка оповещения разделены.
///
/// Новая служба добавляется сразу во все позиции, и без разделения
/// дифф выглядит как «изменилось всё» — вычитывать такое бесполезно.
#[tokio::test]
async fn правки_текста_отделены_от_правок_оповещения() {
    let Some(pool) = pool().await else { return };
    let Some((old, new)) = versions(&pool).await else { return };

    let d = diff(&pool, old, new).await.unwrap();
    assert_eq!(
        d.summary.positions_retitled + d.summary.positions_renotified,
        d.summary.positions_changed,
    );
    assert!(d.summary.positions_renotified > 1000,
        "новые службы должны были разойтись по всему справочнику");
}

/// Дифф версии с самой собой пуст. Проверка на то, что сравнение
/// не выдумывает изменений на ровном месте.
#[tokio::test]
async fn версия_не_отличается_от_самой_себя() {
    let Some(pool) = pool().await else { return };
    let Some((old, _)) = versions(&pool).await else { return };

    let d = diff(&pool, old, old).await.unwrap();
    assert_eq!(d.summary.services_added, 0);
    assert_eq!(d.summary.services_removed, 0);
    assert_eq!(d.summary.positions_added, 0);
    assert_eq!(d.summary.positions_removed, 0);
    assert_eq!(d.summary.positions_changed, 0);
}

/// Дифф в обратную сторону зеркалит добавленное и убранное.
#[tokio::test]
async fn обратный_дифф_зеркален() {
    let Some(pool) = pool().await else { return };
    let Some((old, new)) = versions(&pool).await else { return };

    let forward = diff(&pool, old, new).await.unwrap();
    let back = diff(&pool, new, old).await.unwrap();

    assert_eq!(forward.summary.services_added, back.summary.services_removed);
    assert_eq!(forward.summary.positions_added, back.summary.positions_removed);
}

/// Смена версии — это смена правил, по которым оценивают людей.
/// Кого она задевает, должно быть видно до переключения.
#[tokio::test]
async fn переход_на_новую_версию_показывает_последствия() {
    let Some(pool) = pool().await else { return };
    let Some((old, new)) = versions(&pool).await else { return };

    let i = impact(&pool, old, new).await.unwrap();

    // Новые позиции — БПЛА и «Прочие». Сценариев на них нет ни одного,
    // значит отрабатывать их некому и не на чем
    let numbers: Vec<&str> = i.positions_untrained.iter().map(|p| p.number.as_str()).collect();
    assert!(numbers.contains(&"24120200"), "БПЛА не отмечен как неотработанный");

    // Раз есть неотработанные позиции — есть и курсанты без покрытия
    assert!(!i.students_uncovered.is_empty());
}
