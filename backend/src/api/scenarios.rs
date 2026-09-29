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
        .route("/api/teacher/scenarios", get(library))
        .route("/api/scenarios/manual", post(create_manual))
        .route("/api/scenarios/from-prompt", post(create_from_prompt))
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
/// Что сказать преподавателю, когда ход правки сорвался на модели.
///
/// «Нет JSON» — модель ответила, но не по схеме (оборвала ответ);
/// остальное — модель не ответила вовсе.
fn model_failure_note(error: &str) -> &'static str {
    if error.contains("JSON") || error.contains("json") {
        "Модель ответила не по форме — повторите замечание. Вызов остался прежним."
    } else {
        "Модель не ответила — повторите замечание позже. Вызов остался прежним."
    }
}

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
        (status = 503, description = "Модель не ответила или ответила не по форме; вызов не изменён"),
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

    // Сбой модели — не «internal». Qwen иногда обрывает JSON на
    // середине правки, и преподаватель видел пятисотый без объяснений.
    // Замечание уже в переписке: пишем рядом, что случилось, и отдаём
    // 503 с тем же текстом — как для недоступного сервера голоса
    let (reply, patch, _raw) =
        match scenario_edit::turn(state.ml.llm.as_ref(), &state.db, id, req.comment.trim()).await {
            Ok(t) => t,
            Err(e) if e.downcast_ref::<sqlx::Error>().is_some() => return Err(AppError::Other(e)),
            Err(e) => {
                tracing::warn!(scenario = %id, error = %e, "ход правки сорвался на модели");
                let text = model_failure_note(&e.to_string());
                chat_note(&state.db, id, text).await?;
                return Err(AppError::Unavailable(text.into()));
            }
        };

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

pub(crate) async fn active_version(pool: &sqlx::PgPool) -> AppResult<Uuid> {
    sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Conflict("в базе нет ни одной версии классификатора".into()))
}

// ─── Учебный материал преподавателя ────────────────────────────────────
//
// Пункт 6 замечаний команды: преподаватель создаёт вызовы сам — по
// описанию для модели (потом правит перепиской и вручную и утверждает)
// или вручную целиком. Все утверждённые попадают в составление занятия.

/// Строка списка вызовов у преподавателя: с темой, статусом и тем,
/// откуда вызов взялся. Список курсанта (`/api/scenarios`) черновиков
/// не показывает и темы не знает, поэтому у преподавателя свой.
#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: Uuid,
    pub title: Option<String>,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    pub legend: String,
    pub status: String,
    /// `generated` — из билета моделью, `prompt` — по описанию
    /// преподавателя, `manual` — собран вручную
    pub source: String,
    pub norm_sec: i32,
    /// Итоговый тип эталона: по нему преподаватель узнаёт тему
    pub outcome: Option<String>,
    pub type_ids: Vec<String>,
    pub questions_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/teacher/scenarios",
    tag = "Вызовы",
    responses(
        (status = 200, description = "Все вызовы, кроме архивных, с темой и статусом", body = Vec<LibraryItem>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn library(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<LibraryItem>>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let rows: Vec<LibraryItem> = sqlx::query_as(
        "SELECT s.id, s.title, s.ticket, s.call_no, s.legend, s.status::text AS status,
                s.source, s.norm_sec,
                s.expected->>'outcome' AS outcome,
                COALESCE(ARRAY(SELECT jsonb_array_elements_text(
                                  COALESCE(s.expected->'typeIds', '[]'::jsonb))),
                         '{}') AS type_ids,
                (SELECT count(*) FROM scenario_hidden_facts f
                  WHERE f.scenario_id = s.id AND NOT f.retired) AS questions_count,
                s.created_at
           FROM scenarios s
          WHERE s.status <> 'archived'
          -- Тот же порядок, что у списка курсанта: билеты впереди.
          -- Только что созданный вызов интерфейс открывает сам
          ORDER BY s.ticket NULLS LAST, s.call_no NULLS LAST, s.created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Вызов, собранный преподавателем вручную.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewScenario {
    /// Тема — как вызов будет называться в списке и в составлении занятия
    pub title: String,
    /// Первая фраза заявителя
    pub legend: String,
    #[serde(default)]
    pub applicant_name: String,
    #[serde(default)]
    pub applicant_phone: String,
    #[serde(default)]
    pub applicant_status: String,
    /// Норматив заполнения, секунды
    #[serde(default)]
    pub norm_sec: Option<i32>,
    /// Эталон: типы происшествия и ответы опросной карты. Службы и
    /// итоговый тип посчитает классификатор
    #[serde(default)]
    pub type_ids: Vec<String>,
    #[serde(default)]
    pub answers: std::collections::HashMap<String, Vec<String>>,
    /// Адрес, который оператор должен выяснить
    #[serde(default)]
    pub address: Option<Value>,
    /// Что заявитель скажет только в ответ на вопрос
    #[serde(default)]
    pub hidden: Vec<crate::domain::scenario_edit::HiddenRow>,
}

/// Завести черновик вызова и досчитать эталон — общая часть ручного
/// создания и создания по описанию.
async fn save_draft(
    state: &AppState,
    user: &AuthUser,
    source: &str,
    source_ref: Option<&str>,
    req: &NewScenario,
) -> AppResult<Uuid> {
    let version_id = active_version(&state.db).await?;
    let status = if req.applicant_status.trim().is_empty() {
        "очевидец"
    } else {
        req.applicant_status.trim()
    };
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO scenarios
             (title, legend, applicant_name, applicant_phone, applicant_status,
              norm_sec, expected, version_id, status, source, source_ref, created_by)
         VALUES (NULLIF(btrim($1), ''), $2, $3, $4, $5, $6, $7, $8, 'draft', $9, $10, $11)
         RETURNING id",
    )
    .bind(&req.title)
    .bind(req.legend.trim())
    .bind(req.applicant_name.trim())
    .bind(req.applicant_phone.trim())
    .bind(status)
    .bind(req.norm_sec.unwrap_or(60).clamp(20, 1800))
    .bind(json!({ "typeIds": req.type_ids, "answers": req.answers,
                  "address": req.address.clone().unwrap_or(json!({})) }))
    .bind(version_id)
    .bind(source)
    .bind(source_ref)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    // Факты и эталон — через ту же правку, что у переписки: там факты
    // пишутся, утверждения ставятся в очередь, эталон считает
    // классификатор
    let hidden: Vec<_> = req
        .hidden
        .iter()
        .filter(|h| !h.question.trim().is_empty() && !h.answer.trim().is_empty())
        .enumerate()
        .map(|(i, h)| crate::domain::scenario_edit::HiddenRow {
            key: if h.key.trim().is_empty() { format!("f{}", i + 1) } else { h.key.trim().to_string() },
            question: h.question.trim().to_string(),
            answer: h.answer.trim().to_string(),
            unlocks: h.unlocks.clone(),
            statement: None,
        })
        .collect();
    let patch = Patch {
        hidden: Some(hidden),
        answers: Some(req.answers.clone()),
        type_ids: Some(req.type_ids.clone()),
        ..Default::default()
    };
    scenario_edit::apply(&state.db, version_id, id, &patch)
        .await
        .map_err(AppError::Other)?;
    Ok(id)
}

#[utoipa::path(
    post,
    path = "/api/scenarios/manual",
    tag = "Вызовы",
    request_body = NewScenario,
    responses(
        (status = 201, description = "Черновик вызова, эталон посчитан классификатором", body = crate::domain::scenario_edit::ScenarioFull),
        (status = 400, description = "Нет темы или первой фразы"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn create_manual(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<NewScenario>,
) -> AppResult<(StatusCode, Json<ScenarioFull>)> {
    user.require(&[Role::Teacher, Role::Admin])?;
    if req.title.trim().is_empty() || req.legend.trim().is_empty() {
        return Err(AppError::Validation("нужны тема и первая фраза заявителя".into()));
    }
    let id = save_draft(&state, &user, "manual", None, &req).await?;
    chat_note(&state.db, id, "Вызов собран преподавателем вручную. Эталон посчитан классификатором.").await?;
    audit::log(
        &state.db,
        audit::Event::new("scenario.create").from_user(&user).about("scenario", id)
            .with(json!({ "source": "manual" })),
    )
    .await;
    let full = scenario_edit::load(&state.db, id).await.map_err(AppError::Other)?;
    Ok((StatusCode::CREATED, Json(full)))
}

/// Описание вызова для модели.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    /// Тема — как вызов будет называться в списке
    #[serde(default)]
    pub title: String,
    /// Что произошло, словами преподавателя: «пожилая женщина сообщает
    /// о запахе газа в подъезде, адрес — …»
    pub prompt: String,
}

#[utoipa::path(
    post,
    path = "/api/scenarios/from-prompt",
    tag = "Вызовы",
    request_body = PromptRequest,
    responses(
        (status = 201, description = "Черновик по описанию: реплики пишет модель, эталон — классификатор", body = crate::domain::scenario_edit::ScenarioFull),
        (status = 400, description = "Пустое описание"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn create_from_prompt(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<PromptRequest>,
) -> AppResult<(StatusCode, Json<ScenarioFull>)> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let prompt = req.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::Validation("опишите вызов словами".into()));
    }
    let title = if req.title.trim().is_empty() {
        prompt.chars().take(60).collect::<String>()
    } else {
        req.title.trim().to_string()
    };

    // Тот же конвейер, что у вызовов из билетов: модель выбирает тип и
    // признаки из закрытых словарей и пишет реплики, эталон считает
    // классификатор. Описание преподавателя — вместо текста билета
    let call = crate::ingest::tickets::TicketCall {
        ticket: 0,
        call_no: 0,
        situation: prompt.clone(),
        address: String::new(),
        address_hidden: None,
        applicant_name: None,
        applicant_phone: None,
    };
    let version_id = active_version(&state.db).await?;
    let generated =
        crate::domain::scenario_gen::generate(state.ml.llm.as_ref(), &state.db, version_id, &call).await;

    let (draft, note) = match generated {
        Ok(g) => {
            let answers = g
                .expected
                .get("answers")
                .map(crate::api::sessions::answers_to_map)
                .unwrap_or_default();
            let type_ids: Vec<String> = g
                .expected
                .get("typeIds")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            // Пустая легенда от заглушки или модели — не вызов
            if g.legend.trim().is_empty() {
                (fallback_draft(&title, &prompt), "Модель не написала первую фразу заявителя. \
                    Сохранён черновик с описанием — допишите вручную.".to_string())
            } else {
                (
                    NewScenario {
                        title: title.clone(),
                        legend: g.legend,
                        applicant_name: g.applicant_name,
                        applicant_phone: g.applicant_phone,
                        applicant_status: String::new(),
                        norm_sec: None,
                        type_ids,
                        answers,
                        address: g.expected.get("address").cloned(),
                        hidden: g
                            .hidden
                            .into_iter()
                            .map(|h| crate::domain::scenario_edit::HiddenRow {
                                key: h.key,
                                question: h.question,
                                answer: h.answer,
                                unlocks: h.unlocks,
                                statement: None,
                            })
                            .collect(),
                    },
                    "Вызов подготовлен моделью по описанию преподавателя. Признаки выбраны \
                     из опросной карты, эталон посчитан классификатором. Проверьте, \
                     поправьте перепиской или вручную и утвердите."
                        .to_string(),
                )
            }
        }
        // Модель недоступна или ответила не по форме — не отказываем:
        // черновик с описанием вместо первой фразы, остальное
        // преподаватель допишет вручную. Причину говорим прямо
        Err(e) => {
            tracing::warn!(error = %e, "вызов по описанию не подготовлен моделью");
            (
                fallback_draft(&title, &prompt),
                format!(
                    "Модель не подготовила вызов ({e}). Сохранён черновик с описанием \
                     вместо первой фразы — допишите тип, признаки и вопросы вручную."
                ),
            )
        }
    };

    let id = save_draft(&state, &user, "prompt", Some(&prompt), &draft).await?;
    sqlx::query(
        "INSERT INTO scenario_chat (scenario_id, role, content, user_id)
         VALUES ($1, 'teacher', $2, $3)",
    )
    .bind(id)
    .bind(format!("Описание вызова: {prompt}"))
    .bind(user.id)
    .execute(&state.db)
    .await?;
    chat_note(&state.db, id, &note).await?;
    audit::log(
        &state.db,
        audit::Event::new("scenario.create").from_user(&user).about("scenario", id)
            .with(json!({ "source": "prompt" })),
    )
    .await;
    let full = scenario_edit::load(&state.db, id).await.map_err(AppError::Other)?;
    Ok((StatusCode::CREATED, Json(full)))
}

fn fallback_draft(title: &str, prompt: &str) -> NewScenario {
    NewScenario {
        title: title.to_string(),
        legend: prompt.to_string(),
        applicant_name: String::new(),
        applicant_phone: String::new(),
        applicant_status: String::new(),
        norm_sec: None,
        type_ids: Vec::new(),
        answers: Default::default(),
        address: None,
        hidden: Vec::new(),
    }
}
