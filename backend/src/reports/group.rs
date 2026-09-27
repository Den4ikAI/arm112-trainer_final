//! Отчёт по группе.
//!
//! Считается в очереди: норматив ТЗ на подготовку — 30 секунд, и
//! держать всё это время HTTP-соединение незачем.
//!
//! Числа считает SQL, а не модель. Модель в сводке (`insights.group`)
//! только формулирует текст по готовой статистике — выдумать процент
//! она физически не может.

use crate::state::AppState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Что приходит в задачу из HTTP-эндпоинта.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub group_id: Uuid,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupStats {
    pub group_id: Uuid,
    pub group_title: String,
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    pub members: i64,
    /// Занятий с выставленной оценкой
    pub sessions: i64,
    pub avg_score: Option<f64>,
    pub avg_elapsed_sec: Option<f64>,
    pub avg_norm_sec: Option<f64>,
    pub over_norm: i64,
    /// Доля занятий с превышением норматива, 0..1
    pub over_norm_share: f64,
    /// Сколько раз значение совпало с эталоном, но не было выяснено
    pub guessed: i64,
    pub ok: i64,
    /// Доля угаданного среди совпавшего — главная метрика тренажёра
    pub guessed_share: f64,
    pub top_problems: Vec<Problem>,
    pub students: Vec<StudentRow>,
    /// Ошибки по полям и видам — исходник тепловой карты.
    pub heatmap: Vec<HeatCell>,
    /// Занятия по времени: сколько заняло против норматива.
    pub timeline: Vec<TimelinePoint>,
    /// Сколько полей выяснено, угадано и не заполнено.
    pub funnel: Funnel,
    /// Сколько каких замечаний.
    pub by_kind: Vec<KindCount>,
}

/// Клетка тепловой карты: поле × вид замечания.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HeatCell {
    pub label: String,
    pub kind: String,
    pub count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePoint {
    pub at: chrono::DateTime<chrono::Utc>,
    pub fio: String,
    pub elapsed_sec: i32,
    pub norm_sec: i32,
    pub score: i32,
    pub over_norm: bool,
}

/// Воронка: из всего, что требовалось заполнить, сколько выяснено
/// у заявителя, сколько угадано и сколько не заполнено вовсе.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Funnel {
    pub learned: i64,
    pub guessed: i64,
    pub wrong: i64,
    pub missing: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KindCount {
    pub kind: String,
    pub count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub label: String,
    pub kind: String,
    pub count: i64,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudentRow {
    pub user_id: Uuid,
    pub fio: String,
    pub sessions: i64,
    pub avg_score: Option<f64>,
    pub over_norm: i64,
    pub guessed: i64,
    /// Средний экспертный балл по вызовам, которые оценил преподаватель
    pub avg_expert_score: Option<f64>,
    /// Сколько вызовов оценено экспертно
    pub expert_graded: i64,
}

/// Полный отчёт. Пока это та же статистика — отдельного блока сверх
/// неё у отчёта по группе нет, но вид задачи отделён от сводки,
/// чтобы их можно было заказывать порознь.
pub async fn build(state: &AppState, input: &Value) -> Result<Value> {
    Ok(serde_json::to_value(stats(&state.db, input).await?)?)
}

pub async fn stats(pool: &PgPool, input: &Value) -> Result<GroupStats> {
    let input: Input = serde_json::from_value(input.clone())?;

    let (group_title, members): (String, i64) = sqlx::query_as(
        "SELECT g.title, (SELECT count(*) FROM group_members m WHERE m.group_id = g.id)
           FROM training_groups g WHERE g.id = $1",
    )
    .bind(input.group_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("группа не найдена"))?;

    // Занятия участников группы за период. Границы необязательны:
    // без них берём всё
    let totals: (i64, Option<f64>, Option<f64>, Option<f64>, i64) = sqlx::query_as(
        "SELECT count(*),
                avg(gr.score)::float8,
                avg(gr.elapsed_sec)::float8,
                avg(gr.norm_sec)::float8,
                count(*) FILTER (WHERE gr.over_norm)
           FROM grades gr
           JOIN sessions s ON s.id = gr.session_id
           JOIN group_members m ON m.user_id = s.user_id
          WHERE m.group_id = $1
            AND ($2::timestamptz IS NULL OR gr.created_at >= $2)
            AND ($3::timestamptz IS NULL OR gr.created_at < $3)",
    )
    .bind(input.group_id)
    .bind(input.from)
    .bind(input.to)
    .fetch_one(pool)
    .await?;

    let (sessions, avg_score, avg_elapsed_sec, avg_norm_sec, over_norm) = totals;

    // Разбор лежит в jsonb: разворачиваем массив находок и считаем
    // по видам. Так не нужна отдельная таблица под каждое замечание
    let kinds: Vec<(String, i64)> = sqlx::query_as(
        "SELECT f ->> 'kind' AS kind, count(*)
           FROM grades gr
           JOIN sessions s ON s.id = gr.session_id
           JOIN group_members m ON m.user_id = s.user_id
           CROSS JOIN LATERAL jsonb_array_elements(gr.findings) AS f
          WHERE m.group_id = $1
            AND ($2::timestamptz IS NULL OR gr.created_at >= $2)
            AND ($3::timestamptz IS NULL OR gr.created_at < $3)
          GROUP BY 1",
    )
    .bind(input.group_id)
    .bind(input.from)
    .bind(input.to)
    .fetch_all(pool)
    .await?;

    let count_of = |k: &str| kinds.iter().find(|(kind, _)| kind == k).map_or(0, |(_, n)| *n);
    let guessed = count_of("guessed");
    let ok = count_of("ok");

    let top_problems: Vec<Problem> = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT f ->> 'label', f ->> 'kind', count(*)
           FROM grades gr
           JOIN sessions s ON s.id = gr.session_id
           JOIN group_members m ON m.user_id = s.user_id
           CROSS JOIN LATERAL jsonb_array_elements(gr.findings) AS f
          WHERE m.group_id = $1
            AND ($2::timestamptz IS NULL OR gr.created_at >= $2)
            AND ($3::timestamptz IS NULL OR gr.created_at < $3)
            AND f ->> 'kind' <> 'ok'
          GROUP BY 1, 2
          ORDER BY 3 DESC, 1
          LIMIT 10",
    )
    .bind(input.group_id)
    .bind(input.from)
    .bind(input.to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(label, kind, count)| Problem { label, kind, count })
    .collect();

    // Каждый участник в списке, даже если ещё ни разу не занимался:
    // преподавателю нужно видеть, кто не приступал
    #[allow(clippy::type_complexity)]
    let students: Vec<StudentRow> = sqlx::query_as::<_, (Uuid, String, i64, Option<f64>, i64, i64, Option<f64>, i64)>(
        "SELECT u.id, u.fio,
                count(gr.id),
                avg(gr.score)::float8,
                count(*) FILTER (WHERE gr.over_norm),
                -- sum() от bigint даёт numeric, приводим обратно
                COALESCE(sum((
                  SELECT count(*) FROM jsonb_array_elements(gr.findings) AS f
                   WHERE f ->> 'kind' = 'guessed'
                )), 0)::bigint,
                (avg(eg.score) FILTER (WHERE gr.id IS NOT NULL))::float8,
                count(eg.id) FILTER (WHERE gr.id IS NOT NULL)
           FROM group_members m
           JOIN users u ON u.id = m.user_id
           LEFT JOIN sessions s ON s.user_id = u.id
           LEFT JOIN grades gr ON gr.session_id = s.id
                AND ($2::timestamptz IS NULL OR gr.created_at >= $2)
                AND ($3::timestamptz IS NULL OR gr.created_at < $3)
           -- У вызова одна карточка, у карточки не больше одной
           -- экспертной оценки вызова: строки не множатся
           LEFT JOIN cards c ON c.session_id = s.id
           LEFT JOIN expert_grades eg ON eg.kind = 'card112' AND eg.card_id = c.id
          WHERE m.group_id = $1
          GROUP BY u.id, u.fio
          ORDER BY u.fio",
    )
    .bind(input.group_id)
    .bind(input.from)
    .bind(input.to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(user_id, fio, sessions, avg_score, over_norm, guessed, avg_expert_score, expert_graded)| {
        StudentRow {
            user_id, fio, sessions, avg_score, over_norm, guessed, avg_expert_score, expert_graded,
        }
    })
    .collect();

    // Тепловая карта: поле × вид замечания. Берём только поля, где
    // хоть что-то было не так, — «верно» в карте ошибок не нужно
    let heatmap: Vec<HeatCell> = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT f ->> 'label', f ->> 'kind', count(*)
           FROM grades gr
           JOIN sessions s ON s.id = gr.session_id
           JOIN group_members m ON m.user_id = s.user_id
           CROSS JOIN LATERAL jsonb_array_elements(gr.findings) AS f
          WHERE m.group_id = $1
            AND ($2::timestamptz IS NULL OR gr.created_at >= $2)
            AND ($3::timestamptz IS NULL OR gr.created_at < $3)
            AND f ->> 'kind' <> 'ok'
          GROUP BY 1, 2
          ORDER BY 1, 2",
    )
    .bind(input.group_id)
    .bind(input.from)
    .bind(input.to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(label, kind, count)| HeatCell { label, kind, count })
    .collect();

    // Динамика: последние занятия по времени. Больше двухсот точек
    // на графике всё равно не различить
    let timeline: Vec<TimelinePoint> = sqlx::query_as::<_, (
        chrono::DateTime<chrono::Utc>, String, i32, i32, i32, bool,
    )>(
        "SELECT gr.created_at, u.fio, gr.elapsed_sec, gr.norm_sec, gr.score, gr.over_norm
           FROM grades gr
           JOIN sessions s ON s.id = gr.session_id
           JOIN users u ON u.id = s.user_id
           JOIN group_members m ON m.user_id = s.user_id
          WHERE m.group_id = $1
            AND ($2::timestamptz IS NULL OR gr.created_at >= $2)
            AND ($3::timestamptz IS NULL OR gr.created_at < $3)
          ORDER BY gr.created_at DESC
          LIMIT 200",
    )
    .bind(input.group_id)
    .bind(input.from)
    .bind(input.to)
    .fetch_all(pool)
    .await?
    .into_iter()
    .rev()
    .map(|(at, fio, elapsed_sec, norm_sec, score, over_norm)| TimelinePoint {
        at, fio, elapsed_sec, norm_sec, score, over_norm,
    })
    .collect();

    let by_kind: Vec<KindCount> = kinds
        .iter()
        .filter(|(k, _)| k != "ok")
        .map(|(kind, count)| KindCount { kind: kind.clone(), count: *count })
        .collect();

    // «Выяснил» — это «ok» за вычетом угаданного: угаданное тоже
    // совпало с эталоном, но заявитель этого не говорил
    let funnel = Funnel {
        learned: ok,
        guessed,
        wrong: count_of("wrong"),
        missing: count_of("missing"),
    };

    Ok(GroupStats {
        group_id: input.group_id,
        group_title,
        from: input.from,
        to: input.to,
        members,
        sessions,
        avg_score,
        avg_elapsed_sec,
        avg_norm_sec,
        over_norm,
        over_norm_share: share(over_norm, sessions),
        guessed,
        ok,
        guessed_share: share(guessed, guessed + ok),
        top_problems,
        students,
        heatmap,
        timeline,
        funnel,
        by_kind,
    })
}

/// Доля без деления на ноль: пустая группа даёт 0, а не NaN,
/// который потом не сериализуется в JSON.
fn share(part: i64, whole: i64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

#[cfg(test)]
mod tests {
    use super::share;

    #[test]
    fn доля_от_нуля_это_ноль_а_не_nan() {
        assert_eq!(share(0, 0), 0.0);
        assert_eq!(share(1, 4), 0.25);
    }
}
