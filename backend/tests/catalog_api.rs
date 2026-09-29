//! Проверки справочников. Требуют поднятой базы с применёнными миграциями.
//!
//! Запуск: `cargo test --test catalog_api`
//! Если база недоступна, тесты пропускаются — чтобы не падать на машине,
//! где не поднят docker compose.

use sqlx::PgPool;

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

/// Формулировки в опросных картах менять нельзя: курсант должен видеть
/// на экране ровно то же, что на рабочем АРМ. Тест фиксирует это.
#[tokio::test]
async fn формулировки_карты_101_дословны() {
    let Some(pool) = pool().await else { return };

    let options: Vec<String> = sqlx::query_scalar(
        "SELECT value FROM survey_options
          WHERE type_id = '101' AND question_id = 'fire-sign' ORDER BY sort_order",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        options,
        vec![
            "Открытое пламя / Дым",
            "Запах гари",
            "Сработала пожарная сигнализация",
        ],
        "формулировки сняты с кадра kartochka_02 и должны совпадать дословно"
    );
}

/// В версии сентября 2026 карта 104 — это ОДИН вопрос со всеми вариантами.
/// В инструкции 2019 года было два вопроса; если кто-то перенесёт оттуда,
/// тест это поймает.
#[tokio::test]
async fn карта_104_соответствует_версии_2026() {
    let Some(pool) = pool().await else { return };

    // Только снятое с кадра: вторые и третьи признаки, дописанные из
    // классификатора (ingest/survey.rs), помечены как выведенные
    let questions: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM survey_questions WHERE type_id = '104' AND NOT derived
          ORDER BY sort_order",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        questions,
        vec!["gas-sign", "people-threat"],
        "на кадре сентября 2026 два вопроса: признаки и угроза людям. \
         Вариант из инструкции 2019 («Где ощущается запах» + «Пострадавший») \
         переносить нельзя — это другая версия системы"
    );

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM survey_options
          WHERE type_id = '104' AND question_id = 'gas-sign'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 5, "все пять вариантов в одном вопросе");
}

/// Со скриншотов сняты карты ровно трёх типов. Остальные выводятся
/// из классификатора (`derived`), а не сочиняются, — поэтому считаем
/// только снятые: выведенных теперь десятки.
#[tokio::test]
async fn опросные_карты_только_со_скриншотов() {
    let Some(pool) = pool().await else { return };

    let types: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT type_id FROM survey_questions WHERE NOT derived ORDER BY type_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(
        types,
        vec!["101", "104", "explosion"],
        "все 29 референсных кадров просмотрены, опросных карт на них ровно три"
    );
}

#[tokio::test]
async fn частые_типы_совпадают_с_кадром_пустой_карточки() {
    let Some(pool) = pool().await else { return };

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM incident_types WHERE frequent",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(count, 10, "на кадре kartochka_01 десять плашек");
}
