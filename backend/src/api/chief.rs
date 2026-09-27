//! Доклад диспетчера ДДС вышестоящему начальнику: звонок, реплики,
//! разбор. Логика и допущения — в `domain::chief`.
//!
//! Звонит и докладывает только курсант своей службы: он диспетчер этой
//! ДДС. Преподаватель звонок видит, но не ведёт.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    domain::chief::{self, ChiefView},
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards/{id}/chief", get(view))
        .route("/api/cards/{id}/chief/start", post(start))
        .route("/api/cards/{id}/chief/say", post(say))
        .route("/api/cards/{id}/chief/finish", post(finish))
}

/// Длиннее этого — уже не реплика, а вставленный текст.
const MAX_LINE_CHARS: usize = 1000;

#[derive(Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ServiceQuery {
    /// Служба. Курсанту можно не указывать — берётся его
    pub service_id: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SayRequest {
    pub service_id: Option<String>,
    /// Что сказал диспетчер: набранное или распознанное из речи
    pub text: String,
}

/// Служба, в которой курсант диспетчер. Звонит только он.
async fn dispatcher_service(pool: &PgPool, card_id: Uuid, user: &AuthUser, requested: Option<&str>) -> AppResult<String> {
    if user.role != Role::Student {
        return Err(AppError::Forbidden);
    }
    let own = user
        .service_id
        .clone()
        .ok_or_else(|| AppError::Validation("у пользователя не задана служба".into()))?;
    if requested.is_some_and(|r| !r.is_empty() && r != own) {
        return Err(AppError::Forbidden);
    }
    let in_card: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM card_services WHERE card_id = $1 AND service_id = $2)",
    )
    .bind(card_id)
    .bind(&own)
    .fetch_one(pool)
    .await?;
    if !in_card {
        return Err(AppError::Validation("ваша служба не в списке оповещения по карточке".into()));
    }
    Ok(own)
}

async fn current(pool: &PgPool, card_id: Uuid, service_id: &str, user: &AuthUser) -> AppResult<ChiefView> {
    chief::latest(pool, card_id, service_id, Some(user.id))
        .await
        .map_err(AppError::Other)?
        .ok_or(AppError::NotFound)
}

async fn open_report(pool: &PgPool, card_id: Uuid, service_id: &str, user_id: Uuid) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM chief_reports
          WHERE card_id = $1 AND service_id = $2 AND user_id = $3 AND finished_at IS NULL",
    )
    .bind(card_id)
    .bind(service_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

/// Реплика начальника по всему, что диспетчер сказал. `force_accept` —
/// диспетчер закончил доклад сам: начальник больше не спрашивает.
async fn chief_turn(state: &AppState, report_id: Uuid, card_id: Uuid, service_id: &str, force_accept: bool) -> AppResult<Option<chief::ChiefReview>> {
    let lines = chief::lines(&state.db, report_id).await.map_err(AppError::Other)?;
    let said: Vec<String> = lines.iter().filter(|l| l.who == "dispatcher").map(|l| l.text.clone()).collect();
    let asked: Vec<String> = lines.iter().filter_map(|l| l.topic.clone()).collect();
    let questions = if force_accept { chief::MAX_QUESTIONS } else { asked.len() };

    let facts = chief::facts(&state.db, card_id, service_id).await.map_err(AppError::Other)?;
    // На заглушке модели нет: доложенное ищут правила
    let llm = (state.config.ml_provider != crate::config::MlProvider::Mock).then(|| state.ml.llm.as_ref());
    // Модель в самом разговоре — только если она быстрая: на процессоре
    // её реплика — десятки секунд, а начальник по телефону так не молчит
    let live = crate::reports::forecast::setting(&state.db, "chief.live_model", 0.0).await >= 0.5;
    let (mut review, reply, topic, ms) =
        chief::evaluate(llm.filter(|_| live), &facts, &said, &asked, questions).await;

    // «Принял» звучит сразу, а модель разбирает доклад целиком в фоне,
    // если не разбирала его по ходу разговора: на процессоре это
    // десятки секунд, и держать курсанта в трубке ради разбора незачем.
    // Пока модель думает, в разборе — ключевые слова с пометкой
    let background = topic.is_none() && !live && llm.is_some();
    if background {
        review.pending = true;
        review.note = Some("нейросеть уточняет разбор, это до минуты".into());
    }

    chief::add_line(&state.db, report_id, "chief", &reply, topic.as_deref(), ms)
        .await
        .map_err(AppError::Other)?;

    if topic.is_none() {
        chief::close(&state.db, report_id, &review).await.map_err(AppError::Other)?;
        if background {
            let state = state.clone();
            tokio::spawn(async move {
                let (r, _) = chief::review_with_model(state.ml.llm.as_ref(), &facts, &said).await;
                if let Err(e) = chief::store_review(&state.db, report_id, &r).await {
                    tracing::warn!(error = %e, "разбор доклада начальнику не записан");
                }
            });
        }
        return Ok(Some(review));
    }
    Ok(None)
}

#[utoipa::path(
    get,
    path = "/api/cards/{id}/chief",
    tag = "Бригада ДДС",
    params(("id" = String, Path, description = "Идентификатор карточки"), ServiceQuery),
    responses(
        (status = 200, description = "Последний доклад начальнику; null — не звонили", body = Option<ChiefView>),
        (status = 401, description = "Требуется вход"),
        (status = 404, description = "Карточка не найдена или недоступна"),
    ),
)]
pub async fn view(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<ServiceQuery>,
) -> AppResult<Json<Option<ChiefView>>> {
    crate::api::cards::visible_card(&state.db, id, &user).await?;

    // Курсант видит свои звонки своей службы, преподаватель — любой
    let (service_id, only) = if user.role == Role::Student {
        let own = user.service_id.clone().ok_or(AppError::Forbidden)?;
        if q.service_id.as_deref().is_some_and(|s| !s.is_empty() && s != own) {
            return Err(AppError::Forbidden);
        }
        (own, Some(user.id))
    } else {
        let sv = q
            .service_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::Validation("укажите службу".into()))?;
        (sv, None)
    };

    Ok(Json(chief::latest(&state.db, id, &service_id, only).await.map_err(AppError::Other)?))
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/chief/start",
    tag = "Бригада ДДС",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = ServiceQuery,
    responses(
        (status = 200, description = "Звонок начат: начальник слушает", body = ChiefView),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Звонит только диспетчер своей службы"),
    ),
)]
pub async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ServiceQuery>,
) -> AppResult<Json<ChiefView>> {
    crate::api::cards::visible_card(&state.db, id, &user).await?;
    let service_id = dispatcher_service(&state.db, id, &user, req.service_id.as_deref()).await?;

    let fresh = open_report(&state.db, id, &service_id, user.id).await?.is_none();
    chief::open(&state.db, id, &service_id, user.id).await.map_err(AppError::Other)?;
    if fresh {
        audit::log(
            &state.db,
            audit::Event::new("chief.start").from_user(&user).about("card", id)
                .with(json!({ "serviceId": service_id })),
        )
        .await;
    }
    Ok(Json(current(&state.db, id, &service_id, &user).await?))
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/chief/say",
    tag = "Бригада ДДС",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = SayRequest,
    responses(
        (status = 200, description = "Реплика принята, начальник ответил", body = ChiefView),
        (status = 400, description = "Пустая или слишком длинная реплика"),
        (status = 409, description = "Звонок не начат или уже окончен"),
    ),
)]
pub async fn say(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<SayRequest>,
) -> AppResult<Json<ChiefView>> {
    crate::api::cards::visible_card(&state.db, id, &user).await?;
    let service_id = dispatcher_service(&state.db, id, &user, req.service_id.as_deref()).await?;

    let text = req.text.trim();
    if text.is_empty() {
        return Err(AppError::Validation("реплика пустая".into()));
    }
    if text.chars().count() > MAX_LINE_CHARS {
        return Err(AppError::Validation(format!("реплика длиннее {MAX_LINE_CHARS} знаков")));
    }

    let report_id = open_report(&state.db, id, &service_id, user.id)
        .await?
        .ok_or_else(|| AppError::Conflict("звонок начальнику не начат".into()))?;

    chief::add_line(&state.db, report_id, "dispatcher", text, None, None)
        .await
        .map_err(AppError::Other)?;

    if let Some(review) = chief_turn(&state, report_id, id, &service_id, false).await? {
        finished(&state, &user, id, &service_id, &review).await;
    }
    Ok(Json(current(&state.db, id, &service_id, &user).await?))
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/chief/finish",
    tag = "Бригада ДДС",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = ServiceQuery,
    responses(
        (status = 200, description = "Доклад окончен, есть разбор", body = ChiefView),
        (status = 409, description = "Звонок не начат или уже окончен"),
    ),
)]
pub async fn finish(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ServiceQuery>,
) -> AppResult<Json<ChiefView>> {
    crate::api::cards::visible_card(&state.db, id, &user).await?;
    let service_id = dispatcher_service(&state.db, id, &user, req.service_id.as_deref()).await?;

    let report_id = open_report(&state.db, id, &service_id, user.id)
        .await?
        .ok_or_else(|| AppError::Conflict("звонок начальнику не начат".into()))?;

    if let Some(review) = chief_turn(&state, report_id, id, &service_id, true).await? {
        finished(&state, &user, id, &service_id, &review).await;
    }
    Ok(Json(current(&state.db, id, &service_id, &user).await?))
}

async fn finished(state: &AppState, user: &AuthUser, card_id: Uuid, service_id: &str, review: &chief::ChiefReview) {
    audit::log(
        &state.db,
        audit::Event::new("chief.finish").from_user(user).about("card", card_id)
            .with(json!({
                "serviceId": service_id,
                "reported": review.reported,
                "total": review.total,
                "mode": review.mode,
            })),
    )
    .await;
}
