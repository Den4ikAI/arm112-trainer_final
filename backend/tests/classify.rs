//! Проверки движка эталона.
//!
//! Проверяем не «работает ли код», а **совпадает ли результат
//! с классификатором**. Поэтому ожидания записаны дословными значениями
//! из ЕКП, а не вычислены тем же кодом.
//!
//! Требуют поднятой базы с импортированным классификатором.
//! Без неё тихо пропускаются.

use arm112::domain::classify::classify;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

async fn setup() -> Option<(PgPool, Uuid)> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://arm112:arm112@localhost:5432/arm112".into());
    let pool = PgPool::connect(&url).await.ok()?;
    let version: Uuid = sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .ok()??;
    Some((pool, version))
}

fn answers(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
        .collect()
}

#[tokio::test]
async fn пожар_в_квартире_даёт_верный_класс_и_мчс() {
    let Some((pool, v)) = setup().await else { return };

    let r = classify(&pool, v, &["101".into()], &answers(&[
        ("where", &["Дом"]),
        ("inhouse", &["Квартира"]),
        ("fire-sign", &["Открытое пламя / Дым"]),
    ])).await.unwrap();

    assert_eq!(r.outcome.as_deref(), Some("пожар: квартира"));
    assert!(r.services.iter().any(|s| s.service_id == "mchs"),
            "МЧС обязан быть в списке оповещения");
    assert!(r.services.iter().any(|s| s.value == "пожар: квартира"),
            "значение в матрице — дословный тип происшествия");
}

/// Кнопка «Открытое пламя / Дым» одна, а признака в классификаторе два.
/// Побеждает более тяжёлый: позиция 1050101 (пламя), а не 1050102 (дым).
/// Выведено из кадра сохранённой карточки ДДС.
#[tokio::test]
async fn при_двух_признаках_побеждает_тяжёлый() {
    let Some((pool, v)) = setup().await else { return };

    let r = classify(&pool, v, &["101".into()], &answers(&[
        ("where", &["Дом"]),
        ("inhouse", &["Квартира"]),
        ("fire-sign", &["Открытое пламя / Дым"]),
    ])).await.unwrap();

    assert!(r.positions.contains(&"1050101".to_string()));
    assert!(r.positions.contains(&"1050102".to_string()));
    assert_eq!(r.outcome.as_deref(), Some("пожар: квартира"),
               "«задымление» не должно побеждать «пожар»");
}

/// Часть служб получает лишь копию карточки и в списке оповещения
/// не показывается. Памятка: «Службы, которые не реагируют, а собирают
/// информацию для анализа, не отображаются».
#[tokio::test]
async fn информационные_копии_отделены_от_реагирующих() {
    let Some((pool, v)) = setup().await else { return };

    let r = classify(&pool, v, &["101".into()], &answers(&[
        ("where", &["Дом"]),
        ("inhouse", &["Квартира"]),
        ("fire-sign", &["Открытое пламя / Дым"]),
    ])).await.unwrap();

    assert!(!r.informational.is_empty(), "копии должны быть");
    assert!(r.services.iter().all(|s| s.responding));
    assert!(r.informational.iter().all(|s| !s.responding));
    assert!(r.informational.iter().any(|s| s.value.contains("карточка")));
}

/// У СМП три подколонки по признаку пострадавших. Движок обязан выбрать
/// ту, что соответствует карточке, а не первую попавшуюся.
#[tokio::test]
async fn смп_вызывается_только_при_пострадавших() {
    let Some((pool, v)) = setup().await else { return };

    let base: &[(&str, &[&str])] = &[
        ("where", &["Дом"]),
        ("inhouse", &["Квартира"]),
        ("fire-sign", &["Открытое пламя / Дым"]),
    ];

    let without = classify(&pool, v, &["101".into()], &answers(base)).await.unwrap();
    assert!(
        !without.services.iter().any(|s| s.service_id == "smp"),
        "без пострадавших у позиции нет подколонки СМП, значит и вызова нет"
    );

    let mut with = base.to_vec();
    with.push(("_victims", &["да"]));
    let with = classify(&pool, v, &["101".into()], &answers(&with)).await.unwrap();

    let smp = with.services.iter().find(|s| s.service_id == "smp")
        .expect("с пострадавшими СМП обязана быть");
    assert_eq!(smp.value, "пожар");
}

#[tokio::test]
async fn газификация_поднимает_мосгаз() {
    let Some((pool, v)) = setup().await else { return };

    let r = classify(&pool, v, &["101".into()], &answers(&[
        ("where", &["Дом"]),
        ("inhouse", &["Квартира"]),
        ("fire-sign", &["Открытое пламя / Дым"]),
        ("gasified", &["Да"]),
    ])).await.unwrap();

    assert!(r.services.iter().any(|s| s.service_id == "mosgaz"),
            "при газификации Мосгаз обязан оповещаться");
}

/// Незаполненный уровень не должен молча выбирать случайный вариант.
/// Раньше «Запах газа в помещении» давал класс «в кухне», хотя про кухню
/// никто не говорил.
#[tokio::test]
async fn неоднозначная_комбинация_просит_уточнения() {
    let Some((pool, v)) = setup().await else { return };

    let r = classify(&pool, v, &["104".into()], &answers(&[
        ("gas-sign", &["Запах газа в помещении (в квартире, в доме)"]),
    ])).await.unwrap();

    assert!(r.outcome.is_none(), "класс не должен определяться наугад");
    assert!(r.services.is_empty());
    let reason = r.reason.unwrap_or_default();
    assert!(reason.contains("второго уровня"), "причина: {reason}");
}

#[tokio::test]
async fn без_признаков_возвращается_причина_а_не_ошибка() {
    let Some((pool, v)) = setup().await else { return };

    let r = classify(&pool, v, &["101".into()], &HashMap::new()).await.unwrap();

    assert!(r.outcome.is_none());
    assert!(r.services.is_empty());
    assert!(r.reason.is_some(), "фронтенд показывает это как «тип не определён»");
}

/// Каждый вариант опросной карты обязан быть либо сопоставлен признаку,
/// либо явно помечен информационным. Молча потерянных быть не должно.
#[tokio::test]
async fn все_варианты_карт_разобраны() {
    let Some((pool, _)) = setup().await else { return };

    let unresolved: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT o.type_id, o.question_id, o.value FROM survey_options o
          WHERE NOT EXISTS (SELECT 1 FROM sign_mapping m
                             WHERE m.type_id=o.type_id AND m.question_id=o.question_id
                               AND m.option_value=o.value)
            AND NOT EXISTS (SELECT 1 FROM informational_options i
                             WHERE i.type_id=o.type_id AND i.question_id=o.question_id
                               AND i.option_value=o.value)",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert!(unresolved.is_empty(), "без решения остались: {unresolved:?}");
}
