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
        .route("/api/scenarios/{id}/statements", axum::routing::put(edit_statements))
        .route("/api/scenarios/{id}/statements/prepare", post(prepare_statements))
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
/// Служебная реплика в переписке: то, что сказала система, а не модель.
async fn chat_note(pool: &sqlx::PgPool, id: Uuid, text: &str) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content) VALUES ($1, 'system', $2)",
    )
    .bind(id)
    .bind(text)
    .execute(pool)
    .await?;
    Ok(())
}

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

    let before = scenario_edit::load(&state.db, id).await.map_err(AppError::Other)?;

    let (reply, patch, _raw) = scenario_edit::turn(state.ml.llm.as_ref(), &state.db, id, req.comment.trim())
        .await
        .map_err(AppError::Other)?;

    // Правку применяем до записи реплики: иначе в переписке остаётся
    // «изменено: hidden» от хода, который сорвался на полпути.
    //
    // Сорвавшаяся правка не роняет весь ход — реплику модели
    // преподаватель увидит в любом случае, а рядом прочитает, что
    // именно не получилось. Пятисотый показывал ему «internal»
    let mut applied = false;
    let mut trouble: Option<String> = None;
    if let (Some(patch), true) = (&patch, req.apply) {
        let version_id = active_version(&state.db).await?;
        match scenario_edit::apply(&state.db, version_id, id, patch).await {
            Ok(_) => applied = true,
            Err(e) => {
                tracing::error!(scenario = %id, error = %e, "правка не применилась");
                trouble = Some(e.to_string());
            }
        }
    }

    let scenario = scenario_edit::load(&state.db, id).await.map_err(AppError::Other)?;

    // В переписку пишем разницу между «до» и «после», а не то, что
    // прислала модель: она отчитывается о правках, которых не сделала
    let changed = scenario_edit::changed_fields(&before, &scenario);
    let ничего_не_изменилось = changed.as_object().is_some_and(|m| m.is_empty());
    // «Применено» означает, что вызов изменился, а не что запрос прошёл
    let applied = applied && !ничего_не_изменилось;

    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content, patch)
         VALUES ($1, 'model', $2, $3)",
    )
    .bind(id)
    .bind(&reply)
    .bind(&changed)
    .execute(&state.db)
    .await?;

    if let Some(e) = trouble {
        chat_note(&state.db, id, &format!("Правку не удалось применить: {e}. Вызов остался прежним.")).await?;
    } else if !applied && patch.is_some() && req.apply {
        chat_note(&state.db, id,
            "Модель отчиталась о правке, но вызов не изменился. \
             Попробуйте сказать конкретнее — например, назвать вопрос дословно.").await?;
    }

    audit::log(
        &state.db,
        audit::Event::new("scenario.refine")
            .from_user(&user)
            .about("scenario", id)
            .with(json!({ "comment": req.comment, "applied": applied })),
    )
    .await;

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

/// Правка утверждений фактов преподавателем.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementsEdit {
    /// Утверждения первой фразы заявителя. Пустой список — подготовить
    /// заново моделью
    pub legend_statements: Option<Vec<String>>,
    /// Ключ факта → утверждение. Пустая строка — подготовить заново
    #[serde(default)]
    pub facts: std::collections::HashMap<String, String>,
}

/// Утверждения фактов вызова: по ним описание курсанта сверяется
/// кросс-энкодером. Готовит модель, преподаватель правит здесь —
/// вместе с ответами заявителя, до утверждения вызова.
#[utoipa::path(
    put,
    path = "/api/scenarios/{id}/statements",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    request_body = StatementsEdit,
    responses(
        (status = 200, description = "Вызов с новыми утверждениями", body = crate::domain::scenario_edit::ScenarioFull),
        (status = 400, description = "У вызова нет такого факта"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn edit_statements(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<StatementsEdit>,
) -> AppResult<Json<ScenarioFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    scenario_edit::load(&state.db, id).await.map_err(|_| AppError::NotFound)?;

    crate::domain::fact_statements::save_manual(
        &state.db,
        id,
        req.legend_statements.as_deref(),
        &req.facts,
    )
    .await
    .map_err(|e| AppError::Validation(e.to_string()))?;

    // Сброшенное пустой строкой готовит модель заново
    crate::domain::fact_statements::enqueue(&state.db, id, Some(user.id))
        .await
        .map_err(AppError::Other)?;

    audit::log(
        &state.db,
        audit::Event::new("scenario.statements")
            .from_user(&user)
            .about("scenario", id)
            .with(json!({ "facts": req.facts.len(), "legend": req.legend_statements.is_some() })),
    )
    .await;

    scenario_edit::load(&state.db, id).await.map(Json).map_err(AppError::Other)
}

/// Подготовить утверждения моделью заново, целиком.
#[utoipa::path(
    post,
    path = "/api/scenarios/{id}/statements/prepare",
    tag = "Вызовы",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
    ),
    responses(
        (status = 202, description = "Поставлено в очередь"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Нет такого вызова"),
    ),
)]
async fn prepare_statements(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    user.require(&[Role::Teacher, Role::Admin])?;
    scenario_edit::load(&state.db, id).await.map_err(|_| AppError::NotFound)?;
    crate::domain::fact_statements::enqueue(&state.db, id, Some(user.id))
        .await
        .map_err(AppError::Other)?;
    Ok(StatusCode::ACCEPTED)
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

    // Считаем не только отыгранные занятия, но и выданные задания:
    // вызов, назначенный группе и ещё никем не начатый, занятий не
    // имеет, а ссылку на себя — имеет, и удаление упиралось бы в неё
    let used: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM sessions    WHERE scenario_id = $1)
              + (SELECT count(*) FROM assignments WHERE scenario_id = $1)",
    )
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
                .with(json!({ "занятий_и_заданий": used })),
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
