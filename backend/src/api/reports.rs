//! Отчёты, группы и очередь задач.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    error::{AppError, AppResult},
    jobs::worker,
    reports::session as session_report,
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/groups", get(list_groups))
        .route("/api/reports/session/{id}", get(session))
        .route("/api/reports/group/{id}", post(group_report))
        .route("/api/insights/group/{id}", post(group_insights))
        .route("/api/jobs/{id}", get(job))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupBrief {
    pub id: Uuid,
    pub title: String,
    pub teacher_fio: String,
    pub members: i64,
    /// Из них диспетчеров ДДС: пока приём вызовов 112 свёрнут, группы
    /// одних операторов интерфейс не показывает
    pub dispatchers: i64,
}

/// Курсант видит только свои группы, преподаватель — свои,
/// администратор — все.
#[utoipa::path(
    get,
    path = "/api/groups",
    tag = "Отчёты",
    responses(
        (status = 200, description = "Учебные группы", body = Vec<GroupBrief>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn list_groups(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<GroupBrief>>> {
    let rows: Vec<GroupBrief> = sqlx::query_as(
        "SELECT g.id, g.title, t.fio AS teacher_fio,
                (SELECT count(*) FROM group_members m WHERE m.group_id = g.id) AS members,
                (SELECT count(*) FROM group_members m JOIN users u ON u.id = m.user_id
                  WHERE m.group_id = g.id AND u.duty = 'dds') AS dispatchers
           FROM training_groups g
           JOIN users t ON t.id = g.teacher_id
          WHERE $1
             OR g.teacher_id = $2
             OR EXISTS (SELECT 1 FROM group_members m
                         WHERE m.group_id = g.id AND m.user_id = $2)
          ORDER BY g.title",
    )
    .bind(user.role == Role::Admin)
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct Format {
    /// `json` (по умолчанию) или `csv`
    #[serde(default)]
    pub format: Option<String>,
}

/// Отчёт по занятию. Синхронно: он маленький и нужен сразу после
/// сохранения карточки.
#[utoipa::path(
    get,
    path = "/api/reports/session/{id}",
    tag = "Отчёты",
    params(
        ("id" = String, Path, description = "Идентификатор занятия"),
    ),
    responses(
        (status = 200, description = "Отчёт по занятию (json или csv)", body = crate::reports::session::SessionReport),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn session(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<Format>,
) -> AppResult<Response> {
    // Чужое занятие курсанту не показываем — и не подтверждаем,
    // что оно вообще есть
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let Some(owner) = owner else {
        return Err(AppError::NotFound);
    };
    if user.role == Role::Student && owner != user.id {
        return Err(AppError::NotFound);
    }

    let report = session_report::build(&state, id).await.map_err(AppError::Other)?;

    if q.format.as_deref() == Some("csv") {
        let body = session_report::to_csv(&report);
        let filename = format!("session-{}.csv", report.card_number);
        return Ok((
            [
                (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                ),
            ],
            body,
        )
            .into_response());
    }

    Ok(Json(report).into_response())
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Period {
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub job_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/api/reports/group/{id}",
    tag = "Отчёты",
    params(
        ("id" = String, Path, description = "Идентификатор группы"),
    ),
    responses(
        (status = 200, description = "Заказать отчёт по группе", body = Accepted),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn group_report(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(period): Query<Period>,
) -> AppResult<(StatusCode, Json<Accepted>)> {
    enqueue_group(&state, &user, id, period, "report.group").await
}

#[utoipa::path(
    post,
    path = "/api/insights/group/{id}",
    tag = "Отчёты",
    params(
        ("id" = String, Path, description = "Идентификатор группы"),
    ),
    responses(
        (status = 200, description = "Заказать сводку по группе", body = Accepted),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn group_insights(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(period): Query<Period>,
) -> AppResult<(StatusCode, Json<Accepted>)> {
    enqueue_group(&state, &user, id, period, "insights.group").await
}

/// Отчёт по группе — дело преподавателя. Норматив ТЗ на подготовку
/// 30 секунд, поэтому отвечаем `202` и номером задачи, а не держим
/// соединение.
async fn enqueue_group(
    state: &AppState,
    user: &AuthUser,
    group_id: Uuid,
    period: Period,
    kind: &str,
) -> AppResult<(StatusCode, Json<Accepted>)> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM training_groups
             WHERE id = $1 AND ($2 OR teacher_id = $3))",
    )
    .bind(group_id)
    .bind(user.role == Role::Admin)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    let job_id = worker::enqueue(
        &state.db,
        kind,
        json!({ "groupId": group_id, "from": period.from, "to": period.to }),
        user.id,
    )
    .await
    .map_err(AppError::Other)?;

    // Отчёт по группе содержит результаты людей — кто его заказывал,
    // должно быть видно
    audit::log(
        &state.db,
        audit::Event::new(if kind == "report.group" { "report.group" } else { "insights.group" })
            .from_user(&user)
            .about("group", group_id)
            .with(json!({ "jobId": job_id, "from": period.from, "to": period.to })),
    )
    .await;

    Ok((StatusCode::ACCEPTED, Json(Accepted { job_id })))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Состояние задачи. Свои задачи видит автор, чужие — администратор.
#[utoipa::path(
    get,
    path = "/api/jobs/{id}",
    tag = "Отчёты",
    params(
        ("id" = String, Path, description = "Идентификатор задачи"),
    ),
    responses(
        (status = 200, description = "Состояние фоновой задачи", body = JobView),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn job(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<JobView>> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        Uuid, String, String,
        chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>,
        Option<Value>, Option<String>, Option<Uuid>,
    )> = sqlx::query_as(
        "SELECT id, kind, status::text, created_at, finished_at, output, error, created_by
           FROM ml_jobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;

    let Some((id, kind, status, created_at, finished_at, output, error, created_by)) = row else {
        return Err(AppError::NotFound);
    };
    if user.role != Role::Admin && created_by != Some(user.id) {
        return Err(AppError::NotFound);
    }

    Ok(Json(JobView { id, kind, status, created_at, finished_at, output, error }))
}
