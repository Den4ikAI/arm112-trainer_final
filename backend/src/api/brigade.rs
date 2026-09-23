//! Связь диспетчера ДДС с бригадой: назначение, доклады, звонок
//! старшему и разбор.
//!
//! Раньше диспетчер ДДС просто нажимал статусы — источника сведений,
//! на основании которого он их ставит, в тренажёре не было, и
//! проверить «статусы по факту», как требует памятка ДДС, было нечем.
//!
//! Устройство — из ответов заказчика в чате задачи (21.09.2026,
//! `refs/чат-ответы-заказчика-20-22.09.txt`, сообщения 684 и 691):
//! бригады диспетчер выбирает вручную; старший звонит с докладами
//! сам или диспетчер звонит ему; связь — обычный IP-телефон; алгоритм
//! один для всех служб. Звука здесь нет: доклад приходит текстом,
//! как входящий звонок в карточке ДДС. Голос делается отдельно.

use crate::{
    api::{
        auth::{AuthUser, Role},
        cards::visible_card,
    },
    audit,
    domain::{brigade, response},
    error::{AppError, AppResult},
    reports::forecast::setting,
    state::AppState,
    ws,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards/{id}/brigade", get(view).post(assign))
        .route("/api/cards/{id}/brigade/call", post(call))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentView {
    /// Номер учебной бригады. Настоящих составов служб у нас нет
    pub brigade_no: i32,
    pub assigned_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    pub stage: String,
    pub text: String,
    /// Когда этап наступает на самом деле. Это скрытая истина, как факты
    /// заявителя у оператора 112: курсанту не отдаётся, только
    /// преподавателю и администратору
    pub due_at: Option<DateTime<Utc>>,
    /// Когда диспетчер узнал об этапе
    pub delivered_at: Option<DateTime<Utc>>,
    /// `incoming` — старший позвонил сам, `outgoing` — диспетчер позвонил ему
    pub channel: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    pub at: DateTime<Utc>,
    /// Что узнал: наступивший этап или ничего нового
    pub learned: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrigadeView {
    pub service_id: String,
    /// Из скольких учебных бригад диспетчер выбирает
    pub brigade_count: i32,
    /// Можно назначать: карточка принята, бригады ещё нет, работы
    /// не закрыты
    pub can_assign: bool,
    pub assignment: Option<AssignmentView>,
    pub reports: Vec<ReportView>,
    pub calls: Vec<CallView>,
    /// Разбор работы диспетчера по этой карточке
    pub debrief: brigade::DdsGrade,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ServiceQuery {
    /// Служба. Курсанту можно не указывать — берётся своя
    pub service_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewAssignment {
    pub service_id: String,
    pub brigade_no: i32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CallRequest {
    pub service_id: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CallResult {
    /// Что ответил старший
    pub text: String,
    /// Этап, о котором диспетчер узнал этим звонком
    pub learned: Option<String>,
    pub brigade: BrigadeView,
}

/// Служба, от имени которой действует пользователь.
///
/// Курсант — диспетчер своей службы и ни чьей больше: так же проверяет
/// проставление статусов. Преподавателю службу надо назвать явно.
fn service_of(user: &AuthUser, requested: Option<&str>) -> AppResult<String> {
    if user.role == Role::Student {
        let own = user.service_id.clone().ok_or_else(|| {
            AppError::Validation("у пользователя не задана служба".into())
        })?;
        if requested.is_some_and(|r| r != own) {
            return Err(AppError::Forbidden);
        }
        return Ok(own);
    }
    requested
        .map(str::to_string)
        .ok_or_else(|| AppError::Validation("укажите службу".into()))
}

async fn in_card(pool: &PgPool, card_id: Uuid, service_id: &str) -> AppResult<()> {
    let listed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM card_services WHERE card_id = $1 AND service_id = $2)",
    )
    .bind(card_id)
    .bind(service_id)
    .fetch_one(pool)
    .await?;
    if listed {
        Ok(())
    } else {
        Err(AppError::Validation(
            "служба не входит в список оповещения по этой карточке".into(),
        ))
    }
}

/// Цепочка статусов службы по карточке, с временем.
async fn chain(pool: &PgPool, card_id: Uuid, service_id: &str) -> AppResult<Vec<brigade::Status>> {
    let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT status::text, at FROM response_entries
          WHERE card_id = $1 AND service_id = $2 ORDER BY at, id",
    )
    .bind(card_id)
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(status, at)| brigade::Status { status, at }).collect())
}

/// Этап из базы: stage, text, due_at, delivered_at, channel.
type ReportRow = (String, String, DateTime<Utc>, Option<DateTime<Utc>>, Option<String>);

/// Собрать представление бригады для карточки и службы.
///
/// `staff` — показывать ли шкалу целиком. Курсант видит только то,
/// о чём уже узнал: будущий этап — скрытая истина.
pub async fn load(
    pool: &PgPool,
    card_id: Uuid,
    service_id: &str,
    staff: bool,
) -> AppResult<BrigadeView> {
    let statuses = chain(pool, card_id, service_id).await?;
    let accepted_at = statuses.iter().find(|s| s.status == "Принята").map(|s| s.at);
    let closed = statuses
        .iter()
        .rev()
        .find(|s| !matches!(s.status.as_str(), "Добавлена" | "Получена службой"))
        .is_some_and(|s| response::is_terminal(&s.status));

    let assignment: Option<(Uuid, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, brigade_no, assigned_at FROM brigade_assignments
          WHERE card_id = $1 AND service_id = $2",
    )
    .bind(card_id)
    .bind(service_id)
    .fetch_optional(pool)
    .await?;

    let (reports, calls) = match &assignment {
        Some((id, _, _)) => {
            let reports: Vec<ReportRow> =
                sqlx::query_as(
                    "SELECT stage::text, text, due_at, delivered_at, channel
                       FROM brigade_reports WHERE assignment_id = $1 ORDER BY due_at",
                )
                .bind(id)
                .fetch_all(pool)
                .await?;
            let calls: Vec<CallView> = sqlx::query_as(
                "SELECT at, learned::text AS learned FROM brigade_calls
                  WHERE assignment_id = $1 ORDER BY at",
            )
            .bind(id)
            .fetch_all(pool)
            .await?;
            (reports, calls)
        }
        None => (Vec::new(), Vec::new()),
    };

    let norm = setting(pool, "brigade.update_norm_sec", 30.0).await as i64;
    let debrief = brigade::grade(&brigade::Input {
        accepted_at,
        assigned_at: assignment.as_ref().map(|a| a.2),
        reports: reports
            .iter()
            .map(|(stage, _, due_at, delivered_at, _)| brigade::Report {
                stage: stage.clone(),
                due_at: *due_at,
                delivered_at: *delivered_at,
            })
            .collect(),
        statuses,
        update_norm_sec: norm,
        now: Utc::now(),
    });

    let reports = reports
        .into_iter()
        .filter(|r| staff || r.3.is_some())
        .map(|(stage, text, due_at, delivered_at, channel)| ReportView {
            stage,
            text,
            due_at: staff.then_some(due_at),
            delivered_at,
            channel,
        })
        .collect();

    Ok(BrigadeView {
        service_id: service_id.to_string(),
        brigade_count: setting(pool, "brigade.count", 3.0).await as i32,
        can_assign: accepted_at.is_some() && assignment.is_none() && !closed,
        assignment: assignment.map(|(_, brigade_no, assigned_at)| AssignmentView {
            brigade_no,
            assigned_at,
        }),
        reports,
        calls,
        debrief,
    })
}

/// Балл диспетчера по карточке. Для хода занятия ДДС.
pub async fn score_for(pool: &PgPool, card_id: Uuid, service_id: &str) -> AppResult<Option<i32>> {
    Ok(load(pool, card_id, service_id, true).await?.debrief.score)
}

/// Разослать событие по карточке тем, кто её ведёт.
async fn publish(state: &AppState, card_id: Uuid, kind: &str, payload: serde_json::Value) {
    let topics: Option<(Option<Uuid>, Vec<String>)> = sqlx::query_as(
        "SELECT c.session_id, array(SELECT service_id FROM card_services WHERE card_id = c.id)
           FROM cards c WHERE c.id = $1",
    )
    .bind(card_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    if let Some((session_id, services)) = topics {
        state.ws.publish(ws::routes::card_event(
            kind,
            payload,
            ws::routes::card_topics(&services, session_id),
        ));
    }
}

#[utoipa::path(
    get,
    path = "/api/cards/{id}/brigade",
    tag = "Журнал",
    params(
        ("id" = String, Path, description = "Идентификатор карточки"),
        ServiceQuery,
    ),
    responses(
        (status = 200, description = "Бригада службы, доклады и разбор диспетчера", body = BrigadeView),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Карточка не найдена"),
    ),
)]
pub async fn view(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<ServiceQuery>,
) -> AppResult<Json<BrigadeView>> {
    visible_card(&state.db, id, &user).await?;
    let service_id = service_of(&user, q.service_id.as_deref())?;
    in_card(&state.db, id, &service_id).await?;
    Ok(Json(load(&state.db, id, &service_id, user.role != Role::Student).await?))
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/brigade",
    tag = "Журнал",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = NewAssignment,
    responses(
        (status = 201, description = "Бригада назначена, шкала работ запущена", body = BrigadeView),
        (status = 400, description = "Карточка не принята, бригада уже назначена или такой бригады нет"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn assign(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<NewAssignment>,
) -> AppResult<(StatusCode, Json<BrigadeView>)> {
    visible_card(&state.db, id, &user).await?;
    let service_id = service_of(&user, Some(&req.service_id))?;
    in_card(&state.db, id, &service_id).await?;

    // Направлять бригаду можно только по принятой карточке: заказчик
    // спрашивал именно про «после статуса „Принята“» (сообщение 682)
    let before = load(&state.db, id, &service_id, true).await?;
    if before.assignment.is_some() {
        return Err(AppError::Validation("бригада по этой карточке уже назначена".into()));
    }
    if !before.can_assign {
        return Err(AppError::Validation(
            "бригаду направляют после статуса «Принята», пока работы не закрыты".into(),
        ));
    }
    if !(1..=before.brigade_count).contains(&req.brigade_no) {
        return Err(AppError::Validation(format!(
            "у службы учебные бригады с 1 по {}",
            before.brigade_count
        )));
    }

    let шкала = brigade::Timeline {
        start_sec: setting(&state.db, "brigade.start_sec", 20.0).await as i64,
        arrive_sec: setting(&state.db, "brigade.arrive_sec", 40.0).await as i64,
        work_start_sec: setting(&state.db, "brigade.work_start_sec", 20.0).await as i64,
        work_sec: setting(&state.db, "brigade.work_sec", 60.0).await as i64,
    };

    let mut tx = state.db.begin().await?;
    // Условная вставка: два нажатия подряд не должны дать двух бригад
    let assignment_id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO brigade_assignments (card_id, service_id, brigade_no, user_id)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (card_id, service_id) DO NOTHING
         RETURNING id",
    )
    .bind(id)
    .bind(&service_id)
    .bind(req.brigade_no)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(assignment_id) = assignment_id else {
        return Err(AppError::Validation("бригада по этой карточке уже назначена".into()));
    };

    // Шкала работ целиком создаётся сразу: этапы наступают сами,
    // знает о них диспетчер или нет
    for (stage, offset) in brigade::STAGES.iter().zip(шкала.offsets()) {
        sqlx::query(
            "INSERT INTO brigade_reports (assignment_id, stage, text, due_at)
             VALUES ($1, $2::response_status, $3, now() + make_interval(secs => $4))",
        )
        .bind(assignment_id)
        .bind(stage)
        .bind(brigade::report_text(stage, req.brigade_no))
        .bind(offset as f64)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    audit::log(
        &state.db,
        audit::Event::new("brigade.assign")
            .from_user(&user)
            .about("card", id)
            .with(json!({ "serviceId": service_id, "brigadeNo": req.brigade_no })),
    )
    .await;

    publish(
        &state,
        id,
        "brigade.assigned",
        json!({ "cardId": id, "serviceId": service_id, "brigadeNo": req.brigade_no }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(load(&state.db, id, &service_id, user.role != Role::Student).await?)))
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/brigade/call",
    tag = "Журнал",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = CallRequest,
    responses(
        (status = 200, description = "Ответ старшего бригады", body = CallResult),
        (status = 400, description = "Бригада не назначена"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn call(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CallRequest>,
) -> AppResult<Json<CallResult>> {
    visible_card(&state.db, id, &user).await?;
    let service_id = service_of(&user, Some(&req.service_id))?;
    in_card(&state.db, id, &service_id).await?;

    let assignment: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, brigade_no FROM brigade_assignments WHERE card_id = $1 AND service_id = $2",
    )
    .bind(id)
    .bind(&service_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((assignment_id, brigade_no)) = assignment else {
        return Err(AppError::Validation("звонить некому: бригада не назначена".into()));
    };

    // Второй вариант из ответа заказчика: «диспетчер сам через
    // определённое время набирает по телефону старшего и уточняет ход
    // работ». Всё, что к этому моменту наступило, диспетчер узнаёт сейчас,
    // не дожидаясь доклада
    sqlx::query(
        "UPDATE brigade_reports SET delivered_at = now(), channel = 'outgoing'
          WHERE assignment_id = $1 AND delivered_at IS NULL AND due_at <= now()",
    )
    .bind(assignment_id)
    .execute(&state.db)
    .await?;

    // Старший называет последний наступивший этап
    let occurred: Vec<String> = sqlx::query_scalar(
        "SELECT stage::text FROM brigade_reports
          WHERE assignment_id = $1 AND due_at <= now()",
    )
    .bind(assignment_id)
    .fetch_all(&state.db)
    .await?;
    let learned = brigade::STAGES
        .iter()
        .rev()
        .find(|s| occurred.iter().any(|o| o == *s))
        .map(|s| s.to_string());

    let text = match &learned {
        Some(stage) => brigade::report_text(stage, brigade_no),
        None => brigade::no_change_text(brigade_no),
    };

    sqlx::query(
        "INSERT INTO brigade_calls (assignment_id, user_id, learned)
         VALUES ($1, $2, $3::response_status)",
    )
    .bind(assignment_id)
    .bind(user.id)
    .bind(&learned)
    .execute(&state.db)
    .await?;

    audit::log(
        &state.db,
        audit::Event::new("brigade.call")
            .from_user(&user)
            .about("card", id)
            .with(json!({ "serviceId": service_id, "learned": learned })),
    )
    .await;

    let brigade = load(&state.db, id, &service_id, user.role != Role::Student).await?;
    Ok(Json(CallResult { text, learned, brigade }))
}
