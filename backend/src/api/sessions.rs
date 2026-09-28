//! Занятие обучающегося: карточка, разговор с заявителем, сохранение.
//!
//! Одно занятие — один вызов из билета. Курсант получает только легенду
//! (первую фразу заявителя); остальное он должен ВЫЯСНИТЬ вопросами.
//! Эталон и ответы на невыясненные вопросы наружу не отдаются: иначе
//! их видно в DevTools и метрика «узнал или угадал» теряет смысл.

use crate::{
    audit,
    domain::classify::{classify, ClassifyResult},
    domain::free_question,
    domain::live_applicant,
    domain::grade::{self, Grade},
    ml::LabelScorer,
    ws,
    error::{AppError, AppResult},
    state::AppState,
    api::auth::AuthUser,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/scenarios", get(list_scenarios))
        .route("/api/sessions", post(create))
        .route("/api/sessions/{id}/ask", post(ask))
        .route("/api/sessions/{id}/ask-free", post(ask_free))
        .route("/api/sessions/{id}/card", patch(patch_card))
        .route("/api/sessions/{id}/save", post(save))
        .route("/api/sessions/{id}/transcript", get(transcript))
        .route("/api/sessions/{id}/grade", get(grade_view))
        .route("/api/sessions/active", get(active))
        .route("/api/sessions/{id}", get(state_view))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "session_mode", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    Card112,
    Dds,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: Uuid,
    pub mode: SessionMode,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub norm_sec: i32,
}

/// Сценарий в том виде, в каком его можно показать курсанту:
/// без эталона и без ответов на ещё не заданные вопросы.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioPublic {
    pub id: Uuid,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    pub legend: String,
    pub applicant_name: String,
    pub applicant_phone: String,
    pub applicant_status: String,
    pub norm_sec: i32,
    /// Вопросы, которые можно задать. Ответы приходят по одному
    /// через POST /ask — и попадают в стенограмму.
    pub questions: Vec<QuestionPublic>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionPublic {
    pub fact_id: Uuid,
    pub key: String,
    pub question: String,
    /// Ответ уже получен в этом занятии
    pub revealed: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub id: Uuid,
    pub number: String,
    pub session_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub saved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub operator: String,
    pub workstation: Option<String>,
    pub phone_aon: String,
    pub phone_provided: String,
    pub phone_on_site: String,
    pub applicant_name: String,
    pub applicant_status: String,
    pub address: Value,
    pub description: String,
    pub victims: Option<i32>,
    pub no_contact: bool,
    pub call_dropped: bool,
    pub type_ids: Vec<String>,
    pub answers: Value,
    pub outcome_type: Option<String>,
    pub status: String,
    /// Список оповещения: службы, подобранные классификатором и добавленные руками
    pub service_ids: Vec<String>,
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioBrief {
    pub id: Uuid,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    /// Первая фраза заявителя — по ней курсант узнаёт вызов в списке
    pub legend: String,
    pub norm_sec: i32,
    pub questions_count: i64,
    /// `draft` или `approved`. Курсанту приходят только утверждённые
    pub status: String,
    /// По нему вызов без билета отличается от соседнего с той же
    /// легендой: у подготовленных моделью и заведённых вручную номера
    /// билета нет, и преподаватель видел ряд одинаковых подписей
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Список вызовов, которые можно взять в работу. Эталон не отдаётся
/// и здесь: в списке его тем более незачем показывать.
#[utoipa::path(
    get,
    path = "/api/scenarios",
    tag = "Занятие",
    responses(
        (status = 200, description = "Доступные учебные вызовы", body = Vec<ScenarioBrief>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn list_scenarios(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<ScenarioBrief>>> {
    // Черновики видит только тот, кто их выверяет
    let drafts = user.role != crate::api::auth::Role::Student;

    // Пока по группе курсанта идёт занятие, он видит только его
    // вызовы: тему выбирает преподаватель, а не курсант.
    // Вне занятия список открыт — иначе тренажёром нельзя
    // пользоваться для самостоятельной отработки
    let items: Vec<ScenarioBrief> = sqlx::query_as(
        "SELECT s.id, s.ticket, s.call_no, s.legend, s.norm_sec,
                (SELECT count(*) FROM scenario_hidden_facts f
                  WHERE f.scenario_id = s.id AND NOT f.retired) AS questions_count,
                s.status::text AS status, s.created_at
           FROM scenarios s
          WHERE (s.status = 'approved' OR $1)
            AND (NOT EXISTS (
                     SELECT 1 FROM lessons l
                      JOIN group_members m ON m.group_id = l.group_id
                      WHERE m.user_id = $2
                        AND l.mode = 'card112'
                        AND l.started_at IS NOT NULL AND l.finished_at IS NULL)
                 OR EXISTS (
                     SELECT 1 FROM assignments a
                      JOIN lessons l ON l.id = a.lesson_id
                      WHERE a.scenario_id = s.id AND a.user_id = $2
                        AND l.started_at IS NOT NULL AND l.finished_at IS NULL))
          ORDER BY s.ticket NULLS LAST, s.call_no NULLS LAST, s.created_at DESC",
    )
    .bind(drafts)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    // В марафоне курсант видит только следующий вызов: порядок
    // строгий, а список из десяти кнопок подсказывал бы, что можно
    // начать с любого
    if user.role == crate::api::auth::Role::Student {
        if let Some(next) = crate::api::drill::marathon_next(&state.db, user.id).await? {
            let items = items.into_iter().filter(|s| Some(s.id) == next).collect();
            return Ok(Json(items));
        }
    }

    Ok(Json(items))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    pub scenario_id: Uuid,
    pub mode: SessionMode,
    #[serde(default)]
    pub assignment_id: Option<Uuid>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateResponse {
    pub session: SessionView,
    pub card: CardView,
    pub scenario: ScenarioPublic,
}

#[utoipa::path(
    post,
    path = "/api/sessions",
    tag = "Занятие",
    responses(
        (status = 200, description = "Начать занятие", body = CreateResponse),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateRequest>,
) -> AppResult<(StatusCode, Json<CreateResponse>)> {
    let sc: Option<(Uuid, Option<i32>, Option<i32>, String, String, String, String, i32, String)> =
        sqlx::query_as(
            "SELECT id, ticket, call_no, legend, applicant_name, applicant_phone,
                    applicant_status, norm_sec, status::text
               FROM scenarios WHERE id = $1",
        )
        .bind(req.scenario_id)
        .fetch_optional(&state.db)
        .await?;

    let Some((sid, ticket, call_no, legend, name, phone, app_status, norm_sec, status)) = sc else {
        return Err(AppError::NotFound);
    };
    // Черновик и архив курсанту не дают: первый ещё не выверен
    // преподавателем, второй выведен из оборота
    if status != "approved" {
        user.require(&[crate::api::auth::Role::Teacher, crate::api::auth::Role::Admin])?;
    }

    let (assignment_id, norm_sec) = assignment_for(&state.db, &user, sid, req.assignment_id, norm_sec).await?;
    if let Some(a) = assignment_id {
        crate::api::drill::check_start(&state.db, a, user.id).await?;
    }

    let mut tx = state.db.begin().await?;

    let session_id: Uuid = sqlx::query_scalar(
        "INSERT INTO sessions (user_id, scenario_id, assignment_id, mode, norm_sec)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user.id)
    .bind(sid)
    .bind(assignment_id)
    .bind(req.mode)
    .bind(norm_sec)
    .fetch_one(&mut *tx)
    .await?;

    // АОН заполнен заранее: номер звонящего оператор видит до ответа
    let card_id: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (session_id, created_by, workstation, phone_aon)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(session_id)
    .bind(user.id)
    .bind(&user.workstation)
    .bind(&phone)
    .fetch_one(&mut *tx)
    .await?;

    // Первая фраза заявителя — уже часть разговора
    sqlx::query(
        "INSERT INTO transcript_lines (session_id, who, text, at_sec)
         VALUES ($1, 'заявитель', $2, 0)",
    )
    .bind(session_id)
    .bind(&legend)
    .execute(&mut *tx)
    .await?;

    // Снятые правкой факты в новое занятие не попадают
    let questions: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, key, question FROM scenario_hidden_facts
          WHERE scenario_id = $1 AND NOT retired ORDER BY sort_order",
    )
    .bind(sid)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    // Прогноз фиксируется здесь и больше не меняется. Записать его
    // после — значит подогнать под факт, и проверка потеряет смысл.
    // Если истории мало, прогноза просто нет: врать про точность
    // на трёх занятиях хуже, чем промолчать
    let group_no: Option<i32> = sqlx::query_scalar(
        "SELECT t.group_no FROM incident_types t
          WHERE t.id = (SELECT expected->'typeIds'->>0 FROM scenarios WHERE id = $1)",
    )
    .bind(sid)
    .fetch_optional(&state.db)
    .await?
    .flatten();

    if let Err(e) =
        crate::reports::forecast::record(&state.db, user.id, session_id, group_no).await
    {
        // Прогноз — надстройка: без него занятие проходит как обычно
        tracing::warn!(session = %session_id, error = %e, "прогноз не записан");
    }

    let session: SessionView = load_session_view(&state.db, session_id).await?;
    let card = load_card(&state.db, card_id).await?;

    audit::log(
        &state.db,
        audit::Event::new("session.start")
            .from_user(&user)
            .about("session", session_id)
            .with(serde_json::json!({
                "scenarioId": sid, "mode": req.mode, "cardId": card_id,
            })),
    )
    .await;

    // Преподаватель видит, что курсант взял вызов, сразу — не дожидаясь
    // сохранения карточки
    state.ws.publish(ws::Event::new(
        "session.started",
        serde_json::json!({
            "sessionId": session_id,
            "userId": user.id,
            "scenarioId": sid,
            "ticket": ticket,
            "callNo": call_no,
        }),
    )
    .to(format!("user:{}", user.id))
    .to_all(ws::routes::group_topics(&state.db, user.id).await));

    Ok((
        StatusCode::CREATED,
        Json(CreateResponse {
            session,
            card,
            scenario: ScenarioPublic {
                id: sid,
                ticket,
                call_no,
                legend,
                applicant_name: name,
                applicant_phone: phone,
                applicant_status: app_status,
                norm_sec,
                questions: questions
                    .into_iter()
                    .map(|(fact_id, key, question)| QuestionPublic {
                        fact_id, key, question, revealed: false,
                    })
                    .collect(),
            },
        }),
    ))
}

/// К какому заданию относится попытка, и какой у неё норматив.
///
/// Клиент задание не присылает: курсант берёт вызов из общего
/// списка, а тот при идущем занятии уже сужен до его заданий. Пока
/// связь ждали от клиента, её не было ни у одной попытки, и ход
/// занятия у преподавателя всегда показывал ноль начатых вызовов.
/// Поэтому задание находим сами — по идущему занятию курсанта и
/// сценарию.
///
/// Присланное явно проверяем: попытка, записанная в чужое задание
/// или задание по другому вызову, испортила бы отчёт занятия.
///
/// Норматив задания, если преподаватель его задал, главнее
/// сценарного — для того он в задании и хранится.
async fn assignment_for(
    pool: &sqlx::PgPool,
    user: &AuthUser,
    scenario_id: Uuid,
    requested: Option<Uuid>,
    scenario_norm: i32,
) -> AppResult<(Option<Uuid>, i32)> {
    let found: Option<(Uuid, Option<i32>)> = match requested {
        Some(id) => {
            let row: Option<(Uuid, Option<i32>)> = sqlx::query_as(
                "SELECT a.id, a.norm_sec FROM assignments a
                  WHERE a.id = $1 AND a.scenario_id = $2
                    AND (a.user_id = $3 OR a.group_id IN (
                         SELECT group_id FROM group_members WHERE user_id = $3))",
            )
            .bind(id)
            .bind(scenario_id)
            .bind(user.id)
            .fetch_optional(pool)
            .await?;
            if row.is_none() {
                return Err(AppError::Validation(
                    "задание не ваше или выдано по другому вызову".into(),
                ));
            }
            row
        }
        None => {
            sqlx::query_as(
                "SELECT a.id, a.norm_sec FROM assignments a
                   JOIN lessons l ON l.id = a.lesson_id
                  WHERE a.scenario_id = $1 AND a.user_id = $2
                    AND l.started_at IS NOT NULL AND l.finished_at IS NULL
                  ORDER BY l.started_at DESC
                  LIMIT 1",
            )
            .bind(scenario_id)
            .bind(user.id)
            .fetch_optional(pool)
            .await?
        }
    };

    Ok(match found {
        Some((id, norm)) => (Some(id), norm.unwrap_or(scenario_norm)),
        None => (None, scenario_norm),
    })
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AskRequest {
    pub fact_id: Uuid,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AskResponse {
    pub answer: String,
    pub at_sec: i32,
    /// Поля карточки, которые теперь считаются ВЫЯСНЕННЫМИ, а не угаданными
    pub unlocks: Vec<String>,
}

/// Курсант задаёт вопрос — заявитель отвечает. Факт помечается
/// выясненным: на этом держится разбор «узнал или угадал».
#[utoipa::path(
    post,
    path = "/api/sessions/{id}/ask",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Задать вопрос заявителю", body = AskResponse),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn ask(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AskRequest>,
) -> AppResult<Json<AskResponse>> {
    let session = own_session(&state.db, id, &user).await?;

    // Снятые с учёта факты здесь не отсеиваем: занятие могло начаться
    // до правки, вопрос у курсанта уже на экране, и отказ отвечать на
    // него выглядел бы поломкой
    let fact: Option<(String, String, String, Vec<String>)> = sqlx::query_as(
        "SELECT f.key, f.question, f.answer, f.unlocks
           FROM scenario_hidden_facts f
           JOIN sessions s ON s.scenario_id = f.scenario_id
          WHERE f.id = $1 AND s.id = $2",
    )
    .bind(req.fact_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    let Some((_key, question, answer, unlocks)) = fact else {
        return Err(AppError::NotFound);
    };

    let answer =
        applicant_says(&state, &user, id, &question, Some(req.fact_id), Some(&answer), answer.clone())
            .await?;
    let at_sec = elapsed_sec(session.started_at);

    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO transcript_lines (session_id, who, text, at_sec)
         VALUES ($1, 'оператор', $2, $3), ($1, 'заявитель', $4, $3)",
    )
    .bind(id)
    .bind(&question)
    .bind(at_sec)
    .bind(&answer)
    .execute(&mut *tx)
    .await?;

    // Повторный вопрос времени не переписывает: важна первая попытка
    sqlx::query(
        "INSERT INTO revealed_facts (session_id, fact_id, at_sec) VALUES ($1, $2, $3)
         ON CONFLICT (session_id, fact_id) DO NOTHING",
    )
    .bind(id)
    .bind(req.fact_id)
    .bind(at_sec)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(AskResponse { answer, at_sec, unlocks }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AskFreeRequest {
    /// Вопрос своими словами — напечатанный или распознанный с голоса
    pub text: String,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AskFreeResponse {
    pub answer: String,
    pub at_sec: i32,
    /// Про какой факт вопрос. `None` — ни про какой: заявитель переспросил
    pub fact_id: Option<Uuid>,
    pub unlocks: Vec<String>,
}

/// Свободный вопрос заявителю. Тренажёр понимает, про какой из скрытых
/// фактов спрашивают, и заявитель отвечает записанным в сценарии — или,
/// с `applicant.live_model`, тем же своими словами (`applicant_says`).
/// Не понял — переспрашивает, и факт остаётся невыясненным.
#[utoipa::path(
    post,
    path = "/api/sessions/{id}/ask-free",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Ответ заявителя на свободный вопрос", body = AskFreeResponse),
        (status = 400, description = "Пустой вопрос"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn ask_free(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<AskFreeRequest>,
) -> AppResult<Json<AskFreeResponse>> {
    let session = own_session(&state.db, id, &user).await?;
    let text = req.text.trim();
    if text.is_empty() {
        return Err(AppError::Validation("вопрос пуст".into()));
    }
    // Длинный текст — скорее вставленный откуда-то, чем вопрос
    let text: String = text.chars().take(300).collect();

    // Те же факты, что у курсанта кнопками на экране
    let facts: Vec<(Uuid, String, String, Vec<String>, String)> = sqlx::query_as(
        "SELECT f.id, f.question, f.answer, f.unlocks, f.key
           FROM scenario_hidden_facts f
           JOIN sessions s ON s.scenario_id = f.scenario_id
          WHERE s.id = $1 AND NOT f.retired
          ORDER BY f.sort_order",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    // Ключ факта идёт вместе с вопросом: модели ключ со смыслом помогает
    // отличить вопрос не по делу (см. `LabelScorer`)
    let candidates: Vec<String> = facts.iter().map(|f| format!("{}\t{}", f.4, f.1)).collect();
    // Модель не ответила — понимаем по словам, а не оставляем курсанта
    // без ответа: разговор не должен вставать из-за сервера модели
    let scored = match state.ml.scorer.score(&text, &candidates).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "свободный вопрос: модель не ответила, понимаю по словам");
            crate::ml::mock::MockLabelScorer.score(&text, &candidates).await?
        }
    };
    let hit = free_question::pick(&scored, &candidates).map(|i| &facts[i]);

    let (prepared, fact_id, unlocks) = match hit {
        Some((fid, _, answer, unlocks, _)) => (answer.clone(), Some(*fid), unlocks.clone()),
        None => {
            let name: Option<String> = sqlx::query_scalar(
                "SELECT sc.applicant_name FROM scenarios sc
                   JOIN sessions s ON s.scenario_id = sc.id WHERE s.id = $1",
            )
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
            let female = name.as_deref().is_some_and(free_question::applicant_female);
            (free_question::not_understood(female).to_string(), None, Vec::new())
        }
    };
    let current = hit.map(|(_, _, answer, _, _)| answer.as_str());
    let answer = applicant_says(&state, &user, id, &text, fact_id, current, prepared).await?;

    let at_sec = elapsed_sec(session.started_at);
    let mut tx = state.db.begin().await?;
    // В стенограмме — дословно сказанное курсантом: по ней разбирают,
    // как он спрашивал, а не какую кнопку тренажёр за него подобрал
    sqlx::query(
        "INSERT INTO transcript_lines (session_id, who, text, at_sec)
         VALUES ($1, 'оператор', $2, $3), ($1, 'заявитель', $4, $3)",
    )
    .bind(id)
    .bind(&text)
    .bind(at_sec)
    .bind(&answer)
    .execute(&mut *tx)
    .await?;
    if let Some(fid) = fact_id {
        sqlx::query(
            "INSERT INTO revealed_facts (session_id, fact_id, at_sec) VALUES ($1, $2, $3)
             ON CONFLICT (session_id, fact_id) DO NOTHING",
        )
        .bind(id)
        .bind(fid)
        .bind(at_sec)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Json(AskFreeResponse { answer, at_sec, fact_id, unlocks }))
}

/// Что скажет заявитель: заготовленная фраза сценария или живая реплика
/// модели (`applicant.live_model`, профиль с видеокартой).
///
/// Раскрытие факта решено до этого вызова — здесь только формулировка.
/// Модели уходят легенда, уже раскрытые ответы и ответ на текущий
/// вопрос; ответов на незаданные вопросы в запросе нет. Реплика, не
/// прошедшая проверку или не успевшая, заменяется заготовленной, и это
/// пишется в журнал действий.
async fn applicant_says(
    state: &AppState,
    user: &AuthUser,
    session_id: Uuid,
    question: &str,
    current_fact: Option<Uuid>,
    current_answer: Option<&str>,
    prepared: String,
) -> AppResult<String> {
    let live = crate::reports::forecast::setting(&state.db, "applicant.live_model", 0.0).await >= 0.5;
    if !live {
        return Ok(prepared);
    }

    let (name, status, legend): (String, String, String) = sqlx::query_as(
        "SELECT sc.applicant_name, sc.applicant_status, sc.legend
           FROM scenarios sc JOIN sessions s ON s.scenario_id = sc.id
          WHERE s.id = $1",
    )
    .bind(session_id)
    .fetch_one(&state.db)
    .await?;

    // Раскрытое раньше — без текущего факта: он идёт отдельно, как
    // ответ на этот вопрос
    let revealed: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.question, f.answer
           FROM revealed_facts r JOIN scenario_hidden_facts f ON f.id = r.fact_id
          WHERE r.session_id = $1 AND ($2::uuid IS NULL OR f.id <> $2)
          ORDER BY r.at_sec",
    )
    .bind(session_id)
    .bind(current_fact)
    .fetch_all(&state.db)
    .await?;

    let mut recent: Vec<(String, String)> = sqlx::query_as(
        "SELECT who, text FROM transcript_lines WHERE session_id = $1
          ORDER BY id DESC LIMIT $2",
    )
    .bind(session_id)
    .bind(live_applicant::RECENT_LINES as i64)
    .fetch_all(&state.db)
    .await?;
    recent.reverse();

    let ctx = live_applicant::Context {
        name,
        status,
        legend,
        revealed,
        current: current_answer.map(str::to_string),
        recent,
        question: question.to_string(),
    };
    let started = std::time::Instant::now();
    let outcome = live_applicant::reply(state.ml.llm.as_ref(), &ctx, LIVE_REPLY_BUDGET).await;
    let ms = started.elapsed().as_millis() as u64;
    match outcome {
        live_applicant::Outcome::Live(text) => {
            tracing::debug!(ms, "живая реплика заявителя");
            Ok(text)
        }
        other => {
            let (reason, rejected) = match other {
                live_applicant::Outcome::Rejected { reason, reply } => (reason, Some(reply)),
                live_applicant::Outcome::Fallback(reason) => (reason, None),
                live_applicant::Outcome::Live(_) => unreachable!(),
            };
            tracing::info!(ms, reason, "живая реплика заявителя отбракована — заготовленная");
            audit::log(
                &state.db,
                audit::Event::new("applicant.fallback")
                    .from_user(user)
                    .about("session", session_id)
                    .with(serde_json::json!({ "reason": reason, "rejected": rejected, "ms": ms })),
            )
            .await;
            Ok(prepared)
        }
    }
}

/// Сколько ждём живую реплику вместе с проверкой. На RTX 4090 обе
/// занимают около секунды; дальше разговор уже тормозит.
const LIVE_REPLY_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchCard {
    pub phone_provided: Option<String>,
    pub phone_on_site: Option<String>,
    pub applicant_name: Option<String>,
    pub applicant_status: Option<String>,
    /// Частичный адрес: присланные поля дописываются к уже введённым
    pub address: Option<Value>,
    pub description: Option<String>,
    /// Ключ отсутствует — поле не трогаем; `null` снимает отметку
    /// о пострадавших. Различить это обычным Option нельзя, отсюда
    /// двойной Option и свой десериализатор.
    #[serde(default, deserialize_with = "present_or_absent")]
    pub victims: Option<Option<i32>>,
    pub no_contact: Option<bool>,
    pub call_dropped: Option<bool>,
    pub type_ids: Option<Vec<String>>,
    pub answers: Option<Value>,
    /// Службы, добавленные вручную сверх подобранных классификатором
    pub manual_service_ids: Option<Vec<String>>,
}

/// Вызывается только когда ключ есть в присланном JSON, поэтому
/// `null` доходит сюда как `Some(None)`.
fn present_or_absent<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(de).map(Some)
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchResponse {
    pub card: CardView,
    pub classify: ClassifyResult,
}

/// Частичное обновление. Вызывается по дебаунсу с фронтенда —
/// это и есть автосохранение, переживающее обрыв сети.
#[utoipa::path(
    patch,
    path = "/api/sessions/{id}/card",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Частичное обновление карточки", body = PatchResponse),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn patch_card(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchCard>,
) -> AppResult<Json<PatchResponse>> {
    let session = own_session(&state.db, id, &user).await?;
    if session.finished_at.is_some() {
        return Err(AppError::Conflict("занятие уже завершено".into()));
    }

    let card_id: Uuid = sqlx::query_scalar("SELECT id FROM cards WHERE session_id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    // Адрес и ответы склеиваются на стороне базы: фронтенд шлёт
    // только изменившиеся поля, и два параллельных PATCH не затирают
    // друг друга целиком
    sqlx::query(
        "UPDATE cards SET
            phone_provided   = COALESCE($2, phone_provided),
            phone_on_site    = COALESCE($3, phone_on_site),
            applicant_name   = COALESCE($4, applicant_name),
            applicant_status = COALESCE($5, applicant_status),
            address          = address || COALESCE($6, '{}'::jsonb),
            description      = COALESCE($7, description),
            victims          = CASE WHEN $8 THEN $9 ELSE victims END,
            no_contact       = COALESCE($10, no_contact),
            call_dropped     = COALESCE($11, call_dropped),
            type_ids         = COALESCE($12, type_ids),
            answers          = answers || COALESCE($13, '{}'::jsonb)
         WHERE id = $1",
    )
    .bind(card_id)
    .bind(req.phone_provided.as_ref())
    .bind(req.phone_on_site.as_ref())
    .bind(req.applicant_name.as_ref())
    .bind(req.applicant_status.as_ref())
    .bind(req.address.as_ref())
    .bind(req.description.as_ref())
    .bind(req.victims.is_some())
    .bind(req.victims.flatten())
    .bind(req.no_contact)
    .bind(req.call_dropped)
    .bind(req.type_ids.as_ref())
    .bind(req.answers.as_ref())
    .execute(&state.db)
    .await?;

    let result = reclassify(&state.db, card_id).await?;

    if let Some(manual) = &req.manual_service_ids {
        // Автоподобранную службу удалить нельзя (инструкция, раздел 6),
        // поэтому руками управляем только строками source = 'manual'
        sqlx::query("DELETE FROM card_services WHERE card_id = $1 AND source = 'manual'")
            .bind(card_id)
            .execute(&state.db)
            .await?;
        for s in manual {
            sqlx::query(
                "INSERT INTO card_services (card_id, service_id, source)
                 VALUES ($1, $2, 'manual') ON CONFLICT DO NOTHING",
            )
            .bind(card_id)
            .bind(s)
            .execute(&state.db)
            .await?;
        }
    }

    Ok(Json(PatchResponse {
        card: load_card(&state.db, card_id).await?,
        classify: result,
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveResponse {
    pub card: CardView,
    /// Разбор. Пусто, пока его закрывает марафон: разбор и
    /// рекомендации откроются после последнего вызова
    pub grade: Option<Grade>,
}

/// Сохранение карточки завершает занятие и сразу считает разбор.
/// Считается один раз: пересчёт задним числом после смены версии
/// классификатора менял бы уже выставленные отметки.
#[utoipa::path(
    post,
    path = "/api/sessions/{id}/save",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Сохранить карточку и получить разбор", body = SaveResponse),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn save(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<SaveResponse>> {
    let session = own_session(&state.db, id, &user).await?;
    if session.finished_at.is_some() {
        return Err(AppError::Conflict("занятие уже завершено".into()));
    }

    let card_id: Uuid = sqlx::query_scalar("SELECT id FROM cards WHERE session_id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    let elapsed = elapsed_sec(session.started_at);

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE sessions SET finished_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE cards SET saved_at = now(), status = 'Зарегистрирована' WHERE id = $1")
        .bind(card_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let Some(scenario_id) = session.scenario_id else {
        // Занятие без сценария — свободная тренировка, сравнивать не с чем
        return Err(AppError::Conflict("у занятия нет сценария, разбор невозможен".into()));
    };

    let g = grade::grade(grade::GradeInput {
        session_id: id,
        card_id,
        scenario_id,
        elapsed_sec: elapsed,
        norm_sec: session.norm_sec,
        version_id: active_version(&state.db).await?,
        pool: &state.db,
    })
    .await?;
    grade::store(&state.db, id, &g).await?;

    // Факт проставляется один раз и только после разбора — это
    // вторая половина проверки прогноза
    if let Err(e) = crate::reports::forecast::check(&state.db, id, g.score).await {
        tracing::warn!(session = %id, error = %e, "прогноз не сверен с фактом");
    }

    // Рекомендации: сначала вердикт по открытым, потом новые по тому,
    // что в этом разборе повторилось. Надстройка — без неё разбор
    // проходит как обычно
    if let Err(e) = crate::reports::advice::after_grade(&state.db, id).await {
        tracing::warn!(session = %id, error = %e, "рекомендации не обновлены");
    }

    // Нейросетевой разбор описания — в очередь: на процессоре это
    // десятки секунд, а разбор правил курсант должен увидеть сразу
    if let Err(e) = crate::domain::text_analysis::enqueue(
        &state.db,
        crate::domain::text_analysis::Kind::Card112,
        card_id,
        None,
        Some(user.id),
    )
    .await
    {
        tracing::warn!(session = %id, error = %e, "анализ текста не поставлен в очередь");
    }

    let card = load_card(&state.db, card_id).await?;

    // Сохранение карточки — основное действие оператора, и именно его
    // разбирают, когда выясняют, кто что отправил в службы
    audit::log(
        &state.db,
        audit::Event::new("card.save")
            .from_user(&user)
            .about("card", card_id)
            .with(serde_json::json!({
                "number": card.number,
                "sessionId": id,
                "outcomeType": card.outcome_type,
                "services": card.service_ids,
                "elapsedSec": elapsed,
                "normSec": session.norm_sec,
                "score": g.score,
            })),
    )
    .await;

    // Вот ради чего канал и нужен: карточка уходит диспетчерам
    // профильных ДДС, и у них начинают тикать 30 секунд
    state.ws.publish(ws::routes::card_event(
        "card.dispatched",
        serde_json::json!({
            "cardId": card.id,
            "number": card.number,
            "outcomeType": card.outcome_type,
            "services": card.service_ids,
            "operator": card.operator,
        }),
        ws::routes::card_topics(&card.service_ids, Some(id)),
    ));

    // Балл в событии — тот же, что курсант увидит на экране: канал
    // слушают и сам курсант, и его группа, и в марафоне он не должен
    // открывать то, что закрыто в разборе
    let grade = crate::api::drill::present(&state.db, id, &user, g).await?;
    state.ws.publish(
        ws::Event::new(
            "session.finished",
            serde_json::json!({ "sessionId": id, "userId": user.id,
                                "score": grade.as_ref().map(|g| g.score) }),
        )
        .to(format!("user:{}", user.id))
        .to_all(ws::routes::group_topics(&state.db, user.id).await),
    );

    Ok(Json(SaveResponse { card, grade }))
}

/// Разбор доступен только после сохранения: пока курсант не закончил,
/// показывать ему эталон нельзя.
#[utoipa::path(
    get,
    path = "/api/sessions/{id}/grade",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Разбор занятия", body = Grade),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn grade_view(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Grade>> {
    own_session(&state.db, id, &user).await?;
    let g = grade::load(&state.db, id)
        .await
        .map_err(AppError::Other)?
        .ok_or(AppError::NotFound)?;
    crate::api::drill::present(&state.db, id, &user, g)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::Conflict("разбор марафона откроется после последнего вызова".into()))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    pub who: String,
    pub text: String,
    pub at_sec: i32,
}

#[utoipa::path(
    get,
    path = "/api/sessions/{id}/transcript",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Стенограмма разговора", body = Vec<TranscriptLine>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn transcript(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<TranscriptLine>>> {
    own_session(&state.db, id, &user).await?;

    let lines: Vec<TranscriptLine> = sqlx::query_as(
        "SELECT who, text, at_sec FROM transcript_lines
          WHERE session_id = $1 ORDER BY at_sec, id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(lines))
}

// ─── Продолжение после обновления страницы ──────────────────────────

/// Всё, что нужно экрану, чтобы продолжить вызов с того же места.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub session: SessionView,
    pub card: CardView,
    /// Вопросы — с отметкой, какие уже заданы в этом вызове
    pub scenario: ScenarioPublic,
    pub transcript: Vec<TranscriptLine>,
    /// Список оповещения по уже выбранным признакам. Пусто, пока
    /// тип происшествия не выбран
    pub classify: Option<ClassifyResult>,
    /// Разбор, если карточка уже сохранена
    pub grade: Option<Grade>,
}

/// Сколько живёт незаконченный вызов, который предлагается продолжить.
/// Норматив вызова — минуты; брошенное полдня назад — уже не
/// продолжение, а мусор, и таймер показал бы часы.
const ACTIVE_HOURS: i32 = 12;

/// Незаконченный вызов курсанта — чтобы после обновления страницы,
/// повторного входа или в новой вкладке продолжить его, а не начать
/// заново с пустой карточкой.
///
/// Замечание со стенда: после F5 экран писал «Занятие не запущено»,
/// а повторный выбор того же билета заводил новую карточку.
#[utoipa::path(
    get,
    path = "/api/sessions/active",
    tag = "Занятие",
    responses(
        (status = 200, description = "Незаконченный вызов", body = SessionState),
        (status = 204, description = "Незаконченного вызова нет"),
        (status = 401, description = "Требуется вход"),
    ),
)]
async fn active(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;

    let id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sessions
          WHERE user_id = $1 AND mode = 'card112' AND finished_at IS NULL
            AND scenario_id IS NOT NULL
            AND started_at > now() - make_interval(hours => $2)
          ORDER BY started_at DESC
          LIMIT 1",
    )
    .bind(user.id)
    .bind(ACTIVE_HOURS)
    .fetch_optional(&state.db)
    .await?;

    match id {
        Some(id) => Ok(Json(load_state(&state.db, id, &user).await?).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// Состояние вызова целиком: карточка, разговор, заданные вопросы
/// и — если карточка сохранена — разбор.
#[utoipa::path(
    get,
    path = "/api/sessions/{id}",
    tag = "Занятие",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Состояние вызова", body = SessionState),
        (status = 401, description = "Требуется вход"),
        (status = 404, description = "Нет такого вызова"),
    ),
)]
async fn state_view(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<SessionState>> {
    own_session(&state.db, id, &user).await?;
    Ok(Json(load_state(&state.db, id, &user).await?))
}

async fn load_state(pool: &sqlx::PgPool, id: Uuid, viewer: &AuthUser) -> AppResult<SessionState> {
    let session = load_session_view(pool, id).await?;

    let card_id: Uuid = sqlx::query_scalar("SELECT id FROM cards WHERE session_id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let card = load_card(pool, card_id).await?;

    #[allow(clippy::type_complexity)]
    let sc: (Uuid, Option<i32>, Option<i32>, String, String, String, String) = sqlx::query_as(
        "SELECT s.id, s.ticket, s.call_no, s.legend, s.applicant_name,
                s.applicant_phone, s.applicant_status
           FROM scenarios s JOIN sessions x ON x.scenario_id = s.id
          WHERE x.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let (sid, ticket, call_no, legend, name, phone, app_status) = sc;

    // Снятый правкой вопрос остаётся, если его уже задали: он есть
    // в разговоре, и пропажа кнопки выглядела бы поломкой
    let questions: Vec<(Uuid, String, String, bool)> = sqlx::query_as(
        "SELECT f.id, f.key, f.question,
                EXISTS (SELECT 1 FROM revealed_facts r
                         WHERE r.session_id = $2 AND r.fact_id = f.id) AS revealed
           FROM scenario_hidden_facts f
          WHERE f.scenario_id = $1
            AND (NOT f.retired OR EXISTS (SELECT 1 FROM revealed_facts r
                                           WHERE r.session_id = $2 AND r.fact_id = f.id))
          ORDER BY f.sort_order",
    )
    .bind(sid)
    .bind(id)
    .fetch_all(pool)
    .await?;

    let transcript: Vec<TranscriptLine> = sqlx::query_as(
        "SELECT who, text, at_sec FROM transcript_lines
          WHERE session_id = $1 ORDER BY at_sec, id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    // Список оповещения только считаем, не переписываем: он уже
    // записан при последней правке карточки
    let classify = if card.type_ids.is_empty() {
        None
    } else {
        let answers: Value = sqlx::query_scalar("SELECT answers FROM cards WHERE id = $1")
            .bind(card_id)
            .fetch_one(pool)
            .await?;
        let version_id = active_version(pool).await?;
        Some(classify(pool, version_id, &card.type_ids, &answers_to_map(&answers)).await?)
    };

    // Марафон разбор не показывает и после обновления страницы
    let grade = match grade::load(pool, id).await.map_err(AppError::Other)? {
        Some(g) => crate::api::drill::present(pool, id, viewer, g).await?,
        None => None,
    };

    Ok(SessionState {
        scenario: ScenarioPublic {
            id: sid,
            ticket,
            call_no,
            legend,
            applicant_name: name,
            applicant_phone: phone,
            applicant_status: app_status,
            norm_sec: session.norm_sec,
            questions: questions
                .into_iter()
                .map(|(fact_id, key, question, revealed)| QuestionPublic {
                    fact_id, key, question, revealed,
                })
                .collect(),
        },
        session,
        card,
        transcript,
        classify,
        grade,
    })
}

// ─── Общее ───────────────────────────────────────────────────────────

struct SessionRow {
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    norm_sec: i32,
    scenario_id: Option<Uuid>,
}

/// Чужое занятие курсанту недоступно; преподаватель видит любое.
async fn own_session(pool: &sqlx::PgPool, id: Uuid, user: &AuthUser) -> AppResult<SessionRow> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        Uuid,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
        Option<Uuid>,
    )> = sqlx::query_as(
        "SELECT user_id, started_at, finished_at, norm_sec, scenario_id
           FROM sessions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some((owner, started_at, finished_at, norm_sec, scenario_id)) = row else {
        return Err(AppError::NotFound);
    };
    if owner != user.id && user.role == crate::api::auth::Role::Student {
        // Не Forbidden: иначе по коду ответа можно перебирать чужие занятия
        return Err(AppError::NotFound);
    }
    Ok(SessionRow { started_at, finished_at, norm_sec, scenario_id })
}

fn elapsed_sec(started_at: chrono::DateTime<chrono::Utc>) -> i32 {
    (chrono::Utc::now() - started_at).num_seconds().max(0) as i32
}

async fn load_session_view(pool: &sqlx::PgPool, id: Uuid) -> AppResult<SessionView> {
    let (mode, started_at, finished_at, norm_sec): (
        SessionMode,
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
        i32,
    ) = sqlx::query_as("SELECT mode, started_at, finished_at, norm_sec FROM sessions WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;

    Ok(SessionView { id, mode, started_at, finished_at, norm_sec })
}

#[derive(sqlx::FromRow)]
struct CardRow {
    number: String,
    session_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    saved_at: Option<chrono::DateTime<chrono::Utc>>,
    workstation: Option<String>,
    phone_aon: String,
    phone_provided: String,
    phone_on_site: String,
    applicant_name: String,
    applicant_status: String,
    address: Value,
    description: String,
    victims: Option<i32>,
    no_contact: bool,
    call_dropped: bool,
    type_ids: Vec<String>,
    answers: Value,
    outcome_type: Option<String>,
    status: String,
    operator: String,
}

async fn load_card(pool: &sqlx::PgPool, card_id: Uuid) -> AppResult<CardView> {
    let r: CardRow = sqlx::query_as(
        "SELECT c.number, c.session_id, c.created_at, c.saved_at, c.workstation,
                c.phone_aon, c.phone_provided, c.phone_on_site,
                c.applicant_name, c.applicant_status, c.address, c.description,
                c.victims, c.no_contact, c.call_dropped, c.type_ids, c.answers,
                c.outcome_type, c.status::text AS status,
                -- в карточке стоит тот, кто её ведёт: это уходит в журнал
                COALESCE(u.fio, '') AS operator
           FROM cards c LEFT JOIN users u ON u.id = c.created_by
          WHERE c.id = $1",
    )
    .bind(card_id)
    .fetch_one(pool)
    .await?;

    let service_ids: Vec<String> = sqlx::query_scalar(
        "SELECT service_id FROM card_services WHERE card_id = $1 ORDER BY source, service_id",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await?;

    Ok(CardView {
        id: card_id,
        number: r.number,
        session_id: r.session_id,
        created_at: r.created_at,
        saved_at: r.saved_at,
        operator: r.operator,
        workstation: r.workstation,
        phone_aon: r.phone_aon,
        phone_provided: r.phone_provided,
        phone_on_site: r.phone_on_site,
        applicant_name: r.applicant_name,
        applicant_status: r.applicant_status,
        address: r.address,
        description: r.description,
        victims: r.victims,
        no_contact: r.no_contact,
        call_dropped: r.call_dropped,
        type_ids: r.type_ids,
        answers: r.answers,
        outcome_type: r.outcome_type,
        status: r.status,
        service_ids,
    })
}

/// Активная версия классификатора. Одна на всю установку: курсанты
/// и эталон должны считаться по одному и тому же справочнику.
async fn active_version(pool: &sqlx::PgPool) -> AppResult<Uuid> {
    sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Conflict("в базе нет ни одной версии классификатора".into()))
}

/// Пересчёт списка оповещения после изменения признаков.
/// Строки source = 'auto' полностью переписываются: классификатор —
/// единственный их источник. Ручные и пришедшие из ВИС не трогаем.
async fn reclassify(pool: &sqlx::PgPool, card_id: Uuid) -> AppResult<ClassifyResult> {
    let (type_ids, answers): (Vec<String>, Value) =
        sqlx::query_as("SELECT type_ids, answers FROM cards WHERE id = $1")
            .bind(card_id)
            .fetch_one(pool)
            .await?;

    let version_id = active_version(pool).await?;

    let result = classify(pool, version_id, &type_ids, &answers_to_map(&answers)).await?;

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM card_services WHERE card_id = $1 AND source = 'auto'")
        .bind(card_id)
        .execute(&mut *tx)
        .await?;
    for s in &result.services {
        sqlx::query(
            "INSERT INTO card_services (card_id, service_id, source)
             VALUES ($1, $2, 'auto') ON CONFLICT (card_id, service_id) DO NOTHING",
        )
        .bind(card_id)
        .bind(&s.service_id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE cards SET outcome_type = $2 WHERE id = $1")
        .bind(card_id)
        .bind(result.outcome.as_ref())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(result)
}

/// Ответы в карточке лежат как есть: плашки массивом, этажность —
/// строкой. Движку классификатора нужен единый вид.
pub fn answers_to_map(answers: &Value) -> HashMap<String, Vec<String>> {
    let empty = Map::new();
    let obj = answers.as_object().unwrap_or(&empty);
    obj.iter()
        .map(|(k, v)| {
            let values = match v {
                Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                Value::String(s) => vec![s.clone()],
                Value::Null => vec![],
                other => vec![other.to_string()],
            };
            (k.clone(), values)
        })
        .collect()
}
