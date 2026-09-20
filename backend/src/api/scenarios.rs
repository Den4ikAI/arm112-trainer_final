//! Подготовка учебных вызовов преподавателем.
//!
//! Порядок работы взят из ТЗ, раздел «Настройка учебной среды»:
//! система формирует вызовы, преподаватель их просматривает, правит
//! комментарием и утверждает — частично или полностью.
//!
//! Курсанту доступны только утверждённые: черновик мог быть собран
//! с неполным эталоном, и оценивать по нему нельзя.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    domain::scenario_edit::{self, Patch, ScenarioFull},
    error::{AppError, AppResult},
    jobs::worker,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/scenarios/generate", post(generate))
        .route("/api/scenarios/{id}", get(one).patch(edit).delete(remove))
        .route("/api/scenarios/{id}/chat", get(chat_history).post(chat_turn))
        .route("/api/scenarios/{id}/approve", post(approve))
}

/// Полный сценарий, вместе с эталоном. Только преподавателю:
/// курсант получает `ScenarioPublic` без ответов и без эталона.
#[utoipa::path(
    get,
    path = "/api/scenarios/{id}",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 200, description = "Вызов целиком, вместе с эталоном", body = crate::domain::scenario_edit::ScenarioFull),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ScenarioFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    scenario_edit::load(&state.db, id)
        .await
        .map(Json)
        .map_err(|_| AppError::NotFound)
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub patch: Option<Value>,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/scenarios/{id}/chat",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 200, description = "Переписка о вызове", body = Vec<ChatMessage>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn chat_history(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<ChatMessage>>> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let rows: Vec<ChatMessage> = sqlx::query_as(
        "SELECT id, role::text AS role, content, patch, at
           FROM scenario_chat WHERE scenario_id = $1 ORDER BY at, id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct Comment {
    pub comment: String,
    /// Применить предложенную правку сразу. По умолчанию да:
    /// преподаватель для того и пишет замечание
    #[serde(default = "yes")]
    pub apply: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub reply: String,
    /// Что изменилось. Пусто — модель только ответила словами
    pub applied: bool,
    pub scenario: ScenarioFull,
}

/// Ход переписки: замечание преподавателя — ответ модели — правка.
///
/// Идёт мимо очереди `ml_jobs` намеренно. Очередь без состояния
/// заставила бы каждый ход заново разбирать весь контекст, а это
/// полминуты на процессоре. Здесь же сервер моделей держит KV-кэш,
/// и второй ход стоит секунды.
#[utoipa::path(
    post,
    path = "/api/scenarios/{id}/chat",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 200, description = "Замечание преподавателя и правка", body = TurnResult),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn chat_turn(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<Comment>,
) -> AppResult<Json<TurnResult>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    if req.comment.trim().is_empty() {
        return Err(AppError::Validation("пустое замечание".into()));
    }

    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content, user_id)
         VALUES ($1, 'teacher', $2, $3)",
    )
    .bind(id)
    .bind(req.comment.trim())
    .bind(user.id)
    .execute(&state.db)
    .await?;

    let (reply, patch, raw) = scenario_edit::turn(state.ml.llm.as_ref(), &state.db, id, req.comment.trim())
        .await
        .map_err(AppError::Other)?;

    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content, patch)
         VALUES ($1, 'model', $2, $3)",
    )
    .bind(id)
    .bind(&reply)
    .bind(raw.get("patch").cloned())
    .execute(&state.db)
    .await?;

    let mut applied = false;
    if let (Some(patch), true) = (&patch, req.apply) {
        let version_id = active_version(&state.db).await?;
        scenario_edit::apply(&state.db, version_id, id, patch)
            .await
            .map_err(AppError::Other)?;
        applied = true;
    }

    audit::log(
        &state.db,
        audit::Event::new("scenario.refine")
            .from_user(&user)
            .about("scenario", id)
            .with(json!({ "comment": req.comment, "applied": applied })),
    )
    .await;

    let scenario = scenario_edit::load(&state.db, id).await.map_err(AppError::Other)?;
    Ok(Json(TurnResult { reply, applied, scenario }))
}

/// Ручная правка полей. ТЗ требует и её: «создавать, редактировать
/// и валидировать учебные сценарии».
#[utoipa::path(
    patch,
    path = "/api/scenarios/{id}",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 200, description = "Ручная правка полей", body = crate::domain::scenario_edit::ScenarioFull),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn edit(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(patch): Json<Patch>,
) -> AppResult<Json<ScenarioFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let version_id = active_version(&state.db).await?;
    let scenario = scenario_edit::apply(&state.db, version_id, id, &patch)
        .await
        .map_err(AppError::Other)?;

    // Ручная правка — повод для принудительной проверки грамматики
    // (ТЗ, «Проверка грамматики после ручных манипуляций с данными»)
    let mut issues = Vec::new();
    for text in std::iter::once(scenario.legend.as_str())
        .chain(scenario.hidden.iter().map(|h| h.answer.as_str()))
    {
        issues.extend(state.ml.grammar.check(text).await.map_err(AppError::Other)?);
    }

    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content, patch)
         VALUES ($1, 'system', $2, $3)",
    )
    .bind(id)
    .bind(format!(
        "Правка вручную. Проверка грамматики: {}",
        if issues.is_empty() { "замечаний нет".to_string() } else { format!("{} замечаний", issues.len()) }
    ))
    .bind(serde_json::to_value(&issues).ok())
    .execute(&state.db)
    .await?;

    audit::log(
        &state.db,
        audit::Event::new("scenario.edit")
            .from_user(&user)
            .about("scenario", id)
            .with(json!({ "grammarIssues": issues.len() })),
    )
    .await;

    Ok(Json(scenario))
}

/// Утвердить сценарий. После этого он виден курсантам.
#[utoipa::path(
    post,
    path = "/api/scenarios/{id}/approve",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 200, description = "Утвердить вызов", body = crate::domain::scenario_edit::ScenarioFull),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn approve(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ScenarioFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let scenario = scenario_edit::load(&state.db, id).await.map_err(|_| AppError::NotFound)?;

    // Сценарий с неполным эталоном оценивать нечем: курсант заполнит
    // карточку, а сравнивать будет не с чем
    if let Some(reason) = &scenario.classify_reason {
        return Err(AppError::Validation(format!(
            "эталон неполон: {reason}. Допишите признаки, потом утверждайте"
        )));
    }
    if scenario.hidden.is_empty() {
        return Err(AppError::Validation(
            "у вызова нет скрытых фактов — курсанту нечего выяснять".into(),
        ));
    }

    sqlx::query("UPDATE scenarios SET status = 'approved', approved_by = $2 WHERE id = $1")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;

    audit::log(
        &state.db,
        audit::Event::new("scenario.approve").from_user(&user).about("scenario", id),
    )
    .await;

    scenario_edit::load(&state.db, id).await.map(Json).map_err(AppError::Other)
}

/// Удалить. Только черновик: по утверждённому могли уже заниматься,
/// и его удаление унесло бы занятия вместе с оценками.
#[utoipa::path(
    delete,
    path = "/api/scenarios/{id}",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 204, description = "Удалить черновик"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let used: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE scenario_id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if used > 0 {
        // Архивируем: курсанту не показываем, историю сохраняем
        sqlx::query("UPDATE scenarios SET status = 'archived' WHERE id = $1")
            .bind(id)
            .execute(&state.db)
            .await?;
        audit::log(
            &state.db,
            audit::Event::new("scenario.archive").from_user(&user).about("scenario", id)
                .with(json!({ "sessions": used })),
        )
        .await;
        return Ok(StatusCode::NO_CONTENT);
    }

    sqlx::query("DELETE FROM scenarios WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state.db,
        audit::Event::new("scenario.delete").from_user(&user).about("scenario", id),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    /// Из каких билетов брать. Пусто — из всех
    #[serde(default)]
    pub tickets: Vec<i32>,
    /// Сколько вызовов подготовить
    #[serde(default = "default_count")]
    pub count: usize,
}

fn default_count() -> usize {
    3
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub job_id: Uuid,
}

/// Подготовить вызовы из билетов.
///
/// В очередь, а не синхронно: один вызов это около минуты на
/// процессоре, и преподаватель заказывает их десятками.
#[utoipa::path(
    post,
    path = "/api/scenarios/generate",
    tag = "Вызовы",
    responses(
        (status = 200, description = "Подготовить вызовы из билетов", body = Accepted),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn generate(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<GenerateRequest>,
) -> AppResult<(StatusCode, Json<Accepted>)> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let job_id = worker::enqueue(
        &state.db,
        "scenario.generate",
        json!({ "tickets": req.tickets, "count": req.count.clamp(1, 96) }),
        user.id,
    )
    .await
    .map_err(AppError::Other)?;

    audit::log(
        &state.db,
        audit::Event::new("scenario.generate")
            .from_user(&user)
            .with(json!({ "jobId": job_id, "count": req.count })),
    )
    .await;

    Ok((StatusCode::ACCEPTED, Json(Accepted { job_id })))
}

async fn active_version(pool: &sqlx::PgPool) -> AppResult<Uuid> {
    sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Conflict("в базе нет ни одной версии классификатора".into()))
}
