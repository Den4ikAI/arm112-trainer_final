//! Демонстрационная статистика: заполнить программу историей обучения
//! или очистить её до «голой» программы.
//!
//! Зачем. Панели преподавателя и администратора — аналитика, ход
//! занятий, рекомендации, журнал звонков — пустые на свежем стенде, и
//! показать их нечем. Чистая программа тоже нужна: начать с нуля.
//!
//! Откуда история. Не генерируется здесь и не пишется руками: её
//! набрала учебная группа из 10 симулированных курсантов, которая прошла
//! занятия через настоящий API тренажёра в реальном времени (разный
//! уровень, рост от занятия к занятию, свои задержки и ошибки) — баллы,
//! разбор, рекомендации и их проверку посчитал сам тренажёр. Эта история
//! сохраняется в базе образцом (`demo_snapshot`) перед первой очисткой;
//! «Заполнить» возвращает образец, сдвигая все даты на целое число дней
//! так, чтобы последнее занятие было вчера, — дни недели, часы и минуты
//! каждого курсанта остаются своими.
//!
//! Что считается статистикой: начатые и личные занятия, копии
//! происшествий в них и всё, что на копии ссылается (статусы, бригады,
//! доклады, звонки, IP-телефон, доклады начальнику, экспертные оценки,
//! анализ текста), рекомендации ДДС, попытки 112. Учётные записи,
//! группы, учебный материал, незапущенные занятия, справочники,
//! параметры и журнал аудита остаются.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/demo-stats", get(view))
        .route("/api/demo-stats/clear", post(clear))
        .route("/api/demo-stats/fill", post(fill))
}

/// Таблицы статистики в порядке внешних ключей: родители раньше детей.
/// Условие — какие строки таблицы относятся к статистике; временные
/// таблицы `ds_*` собирает [`mark`].
const TABLES: &[(&str, &str)] = &[
    ("lessons", "id IN (SELECT id FROM ds_lessons)"),
    ("assignments", "lesson_id IN (SELECT id FROM ds_lessons)"),
    ("sessions", "TRUE"),
    ("cards", "id IN (SELECT id FROM ds_cards)"),
    ("lesson_cards", "lesson_id IN (SELECT id FROM ds_lessons)"),
    ("card_services", "card_id IN (SELECT id FROM ds_cards)"),
    ("response_entries", "card_id IN (SELECT id FROM ds_cards)"),
    ("brigade_assignments", "card_id IN (SELECT id FROM ds_cards)"),
    ("brigade_reports", "assignment_id IN (SELECT id FROM ds_brigades)"),
    ("brigade_calls", "assignment_id IN (SELECT id FROM ds_brigades)"),
    ("chief_reports", "card_id IN (SELECT id FROM ds_cards)"),
    ("chief_report_lines", "report_id IN (SELECT id FROM chief_reports WHERE card_id IN (SELECT id FROM ds_cards))"),
    ("phone_calls", "card_id IN (SELECT id FROM ds_cards)"),
    ("phone_call_lines", "call_id IN (SELECT id FROM phone_calls WHERE card_id IN (SELECT id FROM ds_cards))"),
    ("expert_grades", "card_id IN (SELECT id FROM ds_cards)"),
    ("text_analyses", "card_id IN (SELECT id FROM ds_cards)"),
    ("dds_advice", "TRUE"),
    ("grades", "TRUE"),
    ("transcript_lines", "TRUE"),
    ("revealed_facts", "TRUE"),
    ("predictions", "TRUE"),
    ("advice", "TRUE"),
];

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DemoStats {
    /// Проведённые занятия групп (без личных тренировок)
    pub lessons: i64,
    /// Попытки диспетчеров ДДС
    pub attempts: i64,
    /// Курсанты, у которых есть попытки
    pub students: i64,
    /// Вызовы в журнале IP-телефона
    pub calls: i64,
    /// Рекомендации диспетчерам ДДС
    pub advice: i64,
    /// Сохранён ли образец, которым заполняется программа
    pub sample: bool,
}

async fn counts(pool: &PgPool) -> AppResult<DemoStats> {
    let (lessons, attempts, students, calls, advice): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM lessons WHERE started_at IS NOT NULL AND solo_user_id IS NULL),
                (SELECT count(*) FROM brigade_assignments WHERE user_id IS NOT NULL),
                (SELECT count(DISTINCT user_id) FROM brigade_assignments WHERE user_id IS NOT NULL),
                (SELECT count(*) FROM phone_calls),
                (SELECT count(*) FROM dds_advice)",
    )
    .fetch_one(pool)
    .await?;
    Ok(DemoStats { lessons, attempts, students, calls, advice, sample: has_sample(pool).await? })
}

async fn has_sample(pool: &PgPool) -> AppResult<bool> {
    let ok: bool = sqlx::query_scalar("SELECT to_regclass('demo_snapshot.lessons') IS NOT NULL")
        .fetch_one(pool)
        .await?;
    Ok(ok)
}

/// Отметить строки статистики во временных таблицах транзакции —
/// один раз в начале, до снятия образца и удаления.
async fn mark(tx: &mut Transaction<'_, Postgres>) -> AppResult<()> {
    for q in [
        "CREATE TEMP TABLE ds_lessons ON COMMIT DROP AS
           SELECT id FROM lessons WHERE started_at IS NOT NULL OR solo_user_id IS NOT NULL",
        // Карточки занятий: копии происшествий и задания, собранные ИИ
        // для курсанта; утверждённый учебный материал (оригинал без
        // source_card_id) не трогаем. Плюс карточки попыток 112
        "CREATE TEMP TABLE ds_cards ON COMMIT DROP AS
           SELECT c.id FROM cards c
            WHERE ((c.source_card_id IS NOT NULL OR c.approved_at IS NULL)
                   AND c.id IN (SELECT card_id FROM lesson_cards WHERE lesson_id IN (SELECT id FROM ds_lessons)))
               OR c.session_id IS NOT NULL",
        "CREATE TEMP TABLE ds_brigades ON COMMIT DROP AS
           SELECT id FROM brigade_assignments WHERE card_id IN (SELECT id FROM ds_cards)",
    ] {
        sqlx::query(q).execute(&mut **tx).await?;
    }
    Ok(())
}

/// Сохранить текущую статистику образцом, если образца ещё нет и есть
/// что сохранять. Так первая очистка не теряет историю безвозвратно.
async fn capture(tx: &mut Transaction<'_, Postgres>) -> AppResult<bool> {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('demo_snapshot.lessons') IS NOT NULL")
        .fetch_one(&mut **tx)
        .await?;
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM brigade_assignments WHERE user_id IS NOT NULL")
        .fetch_one(&mut **tx)
        .await?;
    if exists || attempts == 0 {
        return Ok(false);
    }
    sqlx::query("CREATE SCHEMA IF NOT EXISTS demo_snapshot").execute(&mut **tx).await?;
    for (table, cond) in TABLES {
        sqlx::query(&format!("CREATE TABLE demo_snapshot.{table} AS SELECT * FROM public.{table} WHERE {cond}"))
            .execute(&mut **tx)
            .await?;
    }
    Ok(true)
}

/// Удалить статистику: дети раньше родителей.
async fn wipe(tx: &mut Transaction<'_, Postgres>) -> AppResult<()> {
    for (table, cond) in TABLES.iter().rev() {
        sqlx::query(&format!("DELETE FROM public.{table} WHERE {cond}"))
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Вернуть образец со сдвигом дат на целое число дней: последнее
/// занятие — вчера.
async fn restore(tx: &mut Transaction<'_, Postgres>) -> AppResult<()> {
    let days: i32 = sqlx::query_scalar(
        "SELECT (current_date - 1 - (SELECT max(started_at) FROM demo_snapshot.lessons)::date)::int",
    )
    .fetch_one(&mut **tx)
    .await?;
    for (table, _) in TABLES {
        // Столбцы образца, которые есть и в рабочей таблице: миграции
        // после снятия образца могли добавить столбцы с умолчаниями
        let cols: Vec<(String, String)> = sqlx::query_as(
            "SELECT s.column_name::text, s.data_type::text
               FROM information_schema.columns s
               JOIN information_schema.columns p
                 ON p.table_schema = 'public' AND p.table_name = s.table_name
                AND p.column_name = s.column_name
              WHERE s.table_schema = 'demo_snapshot' AND s.table_name = $1
              ORDER BY s.ordinal_position",
        )
        .bind(table)
        .fetch_all(&mut **tx)
        .await?;
        if cols.is_empty() {
            continue;
        }
        let names: Vec<String> = cols.iter().map(|(c, _)| format!("\"{c}\"")).collect();
        let exprs: Vec<String> = cols
            .iter()
            .map(|(c, t)| {
                if t.starts_with("timestamp") || t == "date" {
                    format!("\"{c}\" + make_interval(days => {days})")
                } else {
                    format!("\"{c}\"")
                }
            })
            .collect();
        sqlx::query(&format!(
            "INSERT INTO public.{table} ({}) OVERRIDING SYSTEM VALUE SELECT {} FROM demo_snapshot.{table}",
            names.join(", "),
            exprs.join(", ")
        ))
        .execute(&mut **tx)
        .await?;
        // Счётчики bigserial — дальше вставленных номеров
        for (c, _) in &cols {
            let seq: Option<String> = sqlx::query_scalar("SELECT pg_get_serial_sequence($1, $2)")
                .bind(format!("public.{table}"))
                .bind(c)
                .fetch_one(&mut **tx)
                .await?;
            if let Some(seq) = seq {
                sqlx::query(&format!(
                    "SELECT setval('{seq}', GREATEST((SELECT COALESCE(max(\"{c}\"), 0) FROM public.{table}), 1))"
                ))
                .execute(&mut **tx)
                .await?;
            }
        }
    }
    Ok(())
}

async fn begin(state: &AppState) -> AppResult<Transaction<'static, Postgres>> {
    let mut tx = state.db.begin().await?;
    // Два нажатия разом не должны пересечься
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('demo_stats'))")
        .execute(&mut *tx)
        .await?;
    mark(&mut tx).await?;
    Ok(tx)
}

#[utoipa::path(
    get, path = "/api/demo-stats", tag = "Демо-статистика",
    responses(
        (status = 200, description = "Сколько статистики сейчас и есть ли образец", body = DemoStats),
        (status = 403, description = "Только преподаватель и администратор"),
    ),
)]
pub async fn view(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<DemoStats>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    // Образец снимается при первом обращении, пока история на месте
    let mut tx = begin(&state).await?;
    capture(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(counts(&state.db).await?))
}

#[utoipa::path(
    post, path = "/api/demo-stats/clear", tag = "Демо-статистика",
    responses(
        (status = 200, description = "Статистика учеников удалена; учётные записи и материал на месте", body = DemoStats),
        (status = 403, description = "Только преподаватель и администратор"),
    ),
)]
pub async fn clear(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<DemoStats>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let mut tx = begin(&state).await?;
    capture(&mut tx).await?;
    wipe(&mut tx).await?;
    tx.commit().await?;
    audit::log(&state.db, audit::Event::new("demo_stats.clear").from_user(&user)).await;
    Ok(Json(counts(&state.db).await?))
}

#[utoipa::path(
    post, path = "/api/demo-stats/fill", tag = "Демо-статистика",
    responses(
        (status = 200, description = "Программа заполнена историей обучения", body = DemoStats),
        (status = 403, description = "Только преподаватель и администратор"),
        (status = 409, description = "Образца статистики на этом стенде нет"),
    ),
)]
pub async fn fill(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<DemoStats>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let mut tx = begin(&state).await?;
    capture(&mut tx).await?;
    let sample: bool = sqlx::query_scalar("SELECT to_regclass('demo_snapshot.lessons') IS NOT NULL")
        .fetch_one(&mut *tx)
        .await?;
    if !sample {
        return Err(AppError::Conflict(
            "образца статистики на этом стенде нет — сначала нужна пройденная группа".into(),
        ));
    }
    wipe(&mut tx).await?;
    restore(&mut tx).await?;
    tx.commit().await?;
    let now = counts(&state.db).await?;
    audit::log(
        &state.db,
        audit::Event::new("demo_stats.fill")
            .from_user(&user)
            .with(json!({ "lessons": now.lessons, "attempts": now.attempts })),
    )
    .await;
    Ok(Json(now))
}
