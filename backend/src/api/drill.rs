//! Марафон и блиц: правила прохождения и итоговый протокол.
//!
//! Виды занятия заводятся в `api::lessons`. Здесь — то, что меняется
//! для курсанта во время прохождения:
//!
//! * марафон — вызовы строго по порядку, одна попытка на вызов, разбор
//!   и рекомендации скрыты до последнего вызова (или до завершения
//!   занятия преподавателем);
//! * блиц — разбор сразу после каждого вызова, но только по сегменту.
//!
//! Оценка своя не пишется: берётся разбор `domain::grade` и
//! ограничивается `domain::drill`.

use crate::{
    api::auth::{AuthUser, Role},
    domain::{drill, grade::Grade},
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/lessons/{id}/protocol", get(protocol))
}

/// Правила занятия, к которому относится вызов.
#[derive(Debug, Clone)]
pub struct Rules {
    pub lesson_id: Uuid,
    pub kind: String,
    pub segment: Option<String>,
    pub seq: Option<i32>,
    pub lesson_finished: bool,
}

/// Занятие вызова — через задание. Вызов вне занятия правил не имеет.
pub async fn rules_for_session(pool: &PgPool, session_id: Uuid) -> AppResult<Option<Rules>> {
    let row: Option<(Uuid, String, Option<String>, Option<i32>, bool)> = sqlx::query_as(
        "SELECT l.id, l.kind::text, l.segment, a.seq, l.finished_at IS NOT NULL
           FROM sessions s
           JOIN assignments a ON a.id = s.assignment_id
           JOIN lessons l ON l.id = a.lesson_id
          WHERE s.id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(lesson_id, kind, segment, seq, lesson_finished)| Rules {
        lesson_id, kind, segment, seq, lesson_finished,
    }))
}

/// Сколько вызовов марафона курсант ещё не довёл до сохранения.
pub async fn remaining(pool: &PgPool, lesson_id: Uuid, user_id: Uuid) -> AppResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM assignments a
          WHERE a.lesson_id = $1 AND a.user_id = $2
            AND NOT EXISTS (SELECT 1 FROM sessions s
                             WHERE s.assignment_id = a.id AND s.finished_at IS NOT NULL
                               AND EXISTS (SELECT 1 FROM grades g WHERE g.session_id = s.id))",
    )
    .bind(lesson_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

/// Разбор марафона закрыт: занятие идёт, а у курсанта остались вызовы.
///
/// Экзамен: подсказка после третьего вызова — это подсказка к
/// четвёртому. Преподаватель видит всё и сразу — он и принимает
/// экзамен.
async fn closed(pool: &PgPool, rules: &Rules, user_id: Uuid) -> AppResult<bool> {
    if rules.kind != "marathon" || rules.lesson_finished {
        return Ok(false);
    }
    Ok(remaining(pool, rules.lesson_id, user_id).await? > 0)
}

/// Разбор вызова для показа курсанту: `None` — пока закрыт марафоном.
///
/// Преподавателю и администратору марафон разбор не закрывает; блиц
/// ограничивает сегментом для всех — это и есть результат упражнения.
pub async fn present(
    pool: &PgPool,
    session_id: Uuid,
    viewer: &AuthUser,
    g: Grade,
) -> AppResult<Option<Grade>> {
    let Some(rules) = rules_for_session(pool, session_id).await? else {
        return Ok(Some(g));
    };
    if viewer.role == Role::Student {
        let owner: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE id = $1")
            .bind(session_id)
            .fetch_one(pool)
            .await?;
        if closed(pool, &rules, owner).await? {
            return Ok(None);
        }
    }
    Ok(Some(match &rules.segment {
        Some(segment) if segment != drill::SEGMENT_DDS => drill::restrict(g, segment),
        _ => g,
    }))
}

/// Скрыт ли разбор вызова по карточке от курсанта — для экранов, где
/// курсант видит балл по карточке (экспертная оценка, анализ текста).
pub async fn closed_for_card(pool: &PgPool, card_id: Uuid, viewer: &AuthUser) -> AppResult<bool> {
    if viewer.role != Role::Student {
        return Ok(false);
    }
    let session: Option<Uuid> = sqlx::query_scalar("SELECT session_id FROM cards WHERE id = $1")
        .bind(card_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    // Карточка без сессии — учебное происшествие ДДС: у него свой экзамен
    let Some(session) = session else {
        return crate::api::brigade::exam_running(pool, card_id, viewer.id).await;
    };
    match rules_for_session(pool, session).await? {
        Some(rules) => closed(pool, &rules, viewer.id).await,
        None => Ok(false),
    }
}

/// Балл правил в попытках блица — по сегменту, как в разборе:
/// иначе рядом с экспертной оценкой стоял бы балл за всю карточку,
/// а в разборе того же вызова — за один сегмент.
pub async fn segment_scores(
    pool: &PgPool,
    card_id: Uuid,
    attempts: &mut [crate::api::review::Attempt],
) -> AppResult<()> {
    let session: Option<Uuid> = sqlx::query_scalar("SELECT session_id FROM cards WHERE id = $1")
        .bind(card_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    let Some(session) = session else { return Ok(()) };
    let Some(Rules { segment: Some(segment), .. }) = rules_for_session(pool, session).await? else {
        return Ok(());
    };
    if segment == drill::SEGMENT_DDS {
        return Ok(());
    }
    let Some(g) = crate::domain::grade::load(pool, session).await.map_err(AppError::Other)? else {
        return Ok(());
    };
    let score = drill::restrict(g, &segment).score;
    for a in attempts.iter_mut().filter(|a| a.kind == "card112") {
        a.auto_score = Some(score);
    }
    Ok(())
}

/// Можно ли начинать этот вызов марафона.
///
/// Порядок строгий, попытка одна: иначе экзамен превращается в
/// тренировку — первый вызов можно было бы пройти пять раз и оставить
/// лучший результат.
pub async fn check_start(pool: &PgPool, assignment_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let row: Option<(Uuid, String, Option<i32>)> = sqlx::query_as(
        "SELECT l.id, l.kind::text, a.seq
           FROM assignments a JOIN lessons l ON l.id = a.lesson_id
          WHERE a.id = $1",
    )
    .bind(assignment_id)
    .fetch_optional(pool)
    .await?;
    let Some((lesson_id, kind, seq)) = row else { return Ok(()) };

    // Выданное преподавателем задание проходится один раз: закончил —
    // дальше только просмотр разбора (Денис, 29.09: «при открытии уже
    // выполненной карточки обучающийся не должен иметь возможность
    // повторно её пройти»)
    let закончен: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sessions
                         WHERE assignment_id = $1 AND finished_at IS NOT NULL)",
    )
    .bind(assignment_id)
    .fetch_one(pool)
    .await?;
    if закончен {
        return Err(AppError::Conflict(
            "задание уже выполнено: повторно пройти нельзя, открыть можно разбор".into(),
        ));
    }

    if kind != "marathon" {
        return Ok(());
    }

    let начат: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE assignment_id = $1)",
    )
    .bind(assignment_id)
    .fetch_one(pool)
    .await?;
    if начат {
        return Err(AppError::Conflict(
            "в марафоне у каждого вызова одна попытка: незаконченный вызов \
             продолжается из журнала"
                .into(),
        ));
    }

    let раньше: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM assignments a
          WHERE a.lesson_id = $1 AND a.user_id = $2 AND a.seq < $3
            AND NOT EXISTS (SELECT 1 FROM sessions s
                             WHERE s.assignment_id = a.id AND s.finished_at IS NOT NULL)",
    )
    .bind(lesson_id)
    .bind(user_id)
    .bind(seq.unwrap_or(0))
    .fetch_one(pool)
    .await?;
    if раньше > 0 {
        return Err(AppError::Conflict(
            "в марафоне вызовы идут по порядку: сначала закончите предыдущий".into(),
        ));
    }
    Ok(())
}

/// Следующий вызов марафона курсанта — единственный, что он видит в
/// списке, пока марафон идёт. `None` — марафона нет; `Some(None)` —
/// марафон идёт, но все вызовы пройдены.
pub async fn marathon_next(pool: &PgPool, user_id: Uuid) -> AppResult<Option<Option<Uuid>>> {
    let lesson: Option<Uuid> = sqlx::query_scalar(
        "SELECT l.id FROM lessons l
           JOIN group_members m ON m.group_id = l.group_id
          WHERE m.user_id = $1 AND l.kind = 'marathon'
            AND l.started_at IS NOT NULL AND l.finished_at IS NULL
          ORDER BY l.started_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some(lesson) = lesson else { return Ok(None) };

    let next: Option<Uuid> = sqlx::query_scalar(
        "SELECT a.scenario_id FROM assignments a
          WHERE a.lesson_id = $1 AND a.user_id = $2
            AND NOT EXISTS (SELECT 1 FROM sessions s
                             WHERE s.assignment_id = a.id AND s.finished_at IS NOT NULL)
          ORDER BY a.seq LIMIT 1",
    )
    .bind(lesson)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(Some(next))
}

// ─── Итоговый протокол ───────────────────────────────────────────────

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCall {
    /// Номер вызова в занятии
    pub seq: i32,
    pub scenario_id: Uuid,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    /// Начало первой фразы заявителя — чтобы отличать вызовы без билета
    pub legend: String,
    pub session_id: Option<Uuid>,
    pub card_id: Option<Uuid>,
    /// Карточка сохранена и разобрана
    pub finished: bool,
    /// Балл: в блице — по сегменту. Пусто, пока разбор закрыт
    pub score: Option<i32>,
    pub elapsed_sec: Option<i32>,
    pub over_norm: Option<bool>,
    /// Грубые ошибки — подписи полей (`domain::drill::is_gross`)
    pub gross: Vec<String>,
    /// Экспертная оценка преподавателя, если поставлена
    pub expert_score: Option<i32>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolRow {
    pub user_id: Uuid,
    pub fio: String,
    pub calls: Vec<ProtocolCall>,
    /// Сколько вызовов доведено до разбора
    pub done: i64,
    pub total: i64,
    pub avg_score: Option<f64>,
    /// Время на все разобранные вызовы, секунды
    pub total_sec: i64,
    pub gross_count: i64,
    /// Баллы закрыты: марафон курсанта ещё идёт
    pub hidden: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Protocol {
    pub lesson_id: Uuid,
    pub title: String,
    pub kind: String,
    pub mode: String,
    pub segment: Option<String>,
    /// Подпись сегмента блица
    pub segment_label: Option<String>,
    pub finished: bool,
    pub rows: Vec<ProtocolRow>,
}

/// Итоговый протокол занятия: по каждому курсанту — каждый вызов,
/// балл, время, грубые ошибки; средний балл и общее время.
///
/// Преподаватель видит всех и сразу. Курсант — только себя, и в
/// марафоне баллы открываются после последнего вызова.
#[utoipa::path(
    get,
    path = "/api/lessons/{id}/protocol",
    tag = "Занятия",
    params(("id" = String, Path, description = "Идентификатор занятия")),
    responses(
        (status = 200, description = "Итоговый протокол", body = Protocol),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Занятие не найдено"),
    ),
)]
pub async fn protocol(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Protocol>> {
    let pool = &state.db;
    let row: Option<(String, String, String, Option<String>, Uuid, Uuid, bool)> = sqlx::query_as(
        "SELECT title, kind::text, mode::text, segment, group_id, teacher_id,
                finished_at IS NOT NULL
           FROM lessons WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((title, kind, mode, segment, group_id, teacher_id, finished)) = row else {
        return Err(AppError::NotFound);
    };

    // Кого показывать
    let люди: Vec<(Uuid, String)> = match user.role {
        Role::Admin => members(pool, group_id, None).await?,
        Role::Teacher if user.id == teacher_id => members(pool, group_id, None).await?,
        Role::Teacher => return Err(AppError::Forbidden),
        Role::Student => {
            let свои = members(pool, group_id, Some(user.id)).await?;
            if свои.is_empty() {
                return Err(AppError::Forbidden);
            }
            свои
        }
    };

    let mut rows = Vec::with_capacity(люди.len());
    for (user_id, fio) in люди {
        let hidden = user.role == Role::Student
            && kind == "marathon"
            && !finished
            && remaining(pool, id, user_id).await? > 0;
        rows.push(row_for(pool, id, user_id, fio, segment.as_deref(), hidden).await?);
    }

    Ok(Json(Protocol {
        lesson_id: id,
        title,
        kind,
        mode,
        segment_label: segment.as_deref().map(|s| drill::segment_label(s).to_string()),
        segment,
        finished,
        rows,
    }))
}

async fn members(pool: &PgPool, group_id: Uuid, only: Option<Uuid>) -> AppResult<Vec<(Uuid, String)>> {
    Ok(sqlx::query_as(
        "SELECT u.id, u.fio FROM group_members m JOIN users u ON u.id = m.user_id
          WHERE m.group_id = $1 AND ($2::uuid IS NULL OR u.id = $2)
          ORDER BY u.fio",
    )
    .bind(group_id)
    .bind(only)
    .fetch_all(pool)
    .await?)
}

async fn row_for(
    pool: &PgPool,
    lesson_id: Uuid,
    user_id: Uuid,
    fio: String,
    segment: Option<&str>,
    hidden: bool,
) -> AppResult<ProtocolRow> {
    #[allow(clippy::type_complexity)]
    let задания: Vec<(Uuid, Option<i32>, Uuid, Option<i32>, Option<i32>, String)> = sqlx::query_as(
        "SELECT a.id, a.seq, s.id, s.ticket, s.call_no, s.legend
           FROM assignments a JOIN scenarios s ON s.id = a.scenario_id
          WHERE a.lesson_id = $1 AND a.user_id = $2
          ORDER BY a.seq NULLS LAST, a.created_at",
    )
    .bind(lesson_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let mut calls = Vec::with_capacity(задания.len());
    let (mut done, mut sum, mut total_sec, mut gross_count) = (0i64, 0i64, 0i64, 0i64);

    for (n, (assignment_id, seq, scenario_id, ticket, call_no, legend)) in задания.into_iter().enumerate() {
        // Последняя разобранная попытка; нет такой — последняя начатая
        let попытка: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
            "SELECT s.id, c.id FROM sessions s LEFT JOIN cards c ON c.session_id = s.id
              WHERE s.assignment_id = $1
              ORDER BY (EXISTS (SELECT 1 FROM grades g WHERE g.session_id = s.id)) DESC,
                       s.started_at DESC
              LIMIT 1",
        )
        .bind(assignment_id)
        .fetch_optional(pool)
        .await?;

        let (session_id, card_id) = match попытка {
            Some((s, c)) => (Some(s), c),
            None => (None, None),
        };
        let разбор = match session_id {
            Some(s) => crate::domain::grade::load(pool, s).await.map_err(AppError::Other)?,
            None => None,
        };
        let разбор = match (разбор, segment) {
            (Some(g), Some(sg)) if sg != drill::SEGMENT_DDS => Some(drill::restrict(g, sg)),
            (g, _) => g,
        };

        let finished = разбор.is_some();
        let mut call = ProtocolCall {
            seq: seq.unwrap_or(n as i32 + 1),
            scenario_id,
            ticket,
            call_no,
            legend: legend.chars().take(80).collect(),
            session_id,
            card_id,
            finished,
            score: None,
            elapsed_sec: None,
            over_norm: None,
            gross: vec![],
            expert_score: None,
        };

        if let Some(g) = &разбор {
            done += 1;
            sum += g.score as i64;
            total_sec += g.elapsed_sec as i64;
            let gross: Vec<String> = g
                .findings
                .iter()
                .filter(|f| drill::is_gross(f))
                .map(|f| f.label.clone())
                .collect();
            gross_count += gross.len() as i64;
            if !hidden {
                call.score = Some(g.score);
                call.elapsed_sec = Some(g.elapsed_sec);
                call.over_norm = Some(g.over_norm);
                call.gross = gross;
                if let Some(s) = session_id {
                    call.expert_score = crate::api::review::expert_for_session(pool, s)
                        .await?
                        .map(|e| e.score);
                }
            }
        }
        calls.push(call);
    }

    let total = calls.len() as i64;
    Ok(ProtocolRow {
        user_id,
        fio,
        calls,
        done,
        total,
        avg_score: (!hidden && done > 0).then(|| sum as f64 / done as f64),
        total_sec: if hidden { 0 } else { total_sec },
        gross_count: if hidden { 0 } else { gross_count },
        hidden,
    })
}
