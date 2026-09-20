//! Администратор: журнал действий, пользователи, версии классификатора.
//!
//! Раздел «Аудит» инструкции требует, чтобы было видно, кто, что и
//! когда сделал. Отсюда и фильтры: разбирают обычно не всё подряд,
//! а «что делал этот человек в этот день» или «кто трогал эту
//! карточку».

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    error::{AppError, AppResult},
    state::AppState,
};
use argon2::Argon2;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use password_hash::PasswordHasher;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/audit", get(audit_log))
        .route("/api/admin/users", get(list_users).post(create_user))
        .route("/api/admin/users/{id}/active", post(set_active))
        .route("/api/admin/classifier-versions", get(versions))
        .route("/api/admin/classifier-versions/{id}/activate", post(activate_version))
        .route("/api/classifier/versions/{a}/diff/{b}", get(version_diff))
        .route("/api/classifier/versions/{id}/impact", get(version_impact))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    pub user_id: Option<Uuid>,
    pub action: Option<String>,
    /// Всё по одной сущности: `card`, `session`, `group`
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    #[serde(default)]
    pub page: i64,
    #[serde(default = "default_size")]
    pub size: i64,
}

fn default_size() -> i64 {
    100
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub at: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<Uuid>,
    pub fio: Option<String>,
    pub action: String,
    pub entity: Option<String>,
    pub entity_id: Option<String>,
    pub payload: Option<Value>,
    pub ip: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    pub items: Vec<AuditEntry>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
    /// Какие виды действий вообще встречаются — для выпадающего списка
    pub actions: Vec<String>,
}

/// Журнал действий.
///
/// Только администратору. ТЗ относит «просматривать системные
/// журналы» к его функциям мониторинга, а преподавателю даёт другой
/// инструмент — отчёты по занятию и по группе. Журнал действий
/// показывает, кто когда входил и с какого адреса; преподавателю для
/// разбора занятия это не нужно.
#[utoipa::path(
    get,
    path = "/api/admin/audit",
    tag = "Администрирование",
    responses(
        (status = 200, description = "Журнал действий", body = AuditPage),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn audit_log(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<AuditPage>> {
    user.require(&[Role::Admin])?;

    let size = q.size.clamp(1, 500);
    let offset = q.page.max(0) * size;

    let items: Vec<AuditEntry> = sqlx::query_as(
        "SELECT a.id, a.at, a.user_id, u.fio, a.action, a.entity, a.entity_id,
                a.payload, host(a.ip) AS ip
           FROM audit_log a
           LEFT JOIN users u ON u.id = a.user_id
          WHERE ($1::timestamptz IS NULL OR a.at >= $1)
            AND ($2::timestamptz IS NULL OR a.at < $2)
            AND ($3::uuid IS NULL OR a.user_id = $3)
            AND ($4::text IS NULL OR a.action = $4)
            AND ($5::text IS NULL OR a.entity = $5)
            AND ($6::text IS NULL OR a.entity_id = $6)
          ORDER BY a.at DESC, a.id DESC
          LIMIT $7 OFFSET $8",
    )
    .bind(q.from)
    .bind(q.to)
    .bind(q.user_id)
    .bind(&q.action)
    .bind(&q.entity)
    .bind(&q.entity_id)
    .bind(size)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log a
          WHERE ($1::timestamptz IS NULL OR a.at >= $1)
            AND ($2::timestamptz IS NULL OR a.at < $2)
            AND ($3::uuid IS NULL OR a.user_id = $3)
            AND ($4::text IS NULL OR a.action = $4)
            AND ($5::text IS NULL OR a.entity = $5)
            AND ($6::text IS NULL OR a.entity_id = $6)",
    )
    .bind(q.from)
    .bind(q.to)
    .bind(q.user_id)
    .bind(&q.action)
    .bind(&q.entity)
    .bind(&q.entity_id)
    .fetch_one(&state.db)
    .await?;

    let actions: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT action FROM audit_log ORDER BY 1")
            .fetch_all(&state.db)
            .await?;

    Ok(Json(AuditPage { items, total, page: q.page.max(0), size, actions }))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    pub id: Uuid,
    pub login: String,
    pub fio: String,
    pub role: String,
    pub service_id: Option<String>,
    pub service_title: Option<String>,
    pub workstation: Option<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub sessions: i64,
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    tag = "Администрирование",
    responses(
        (status = 200, description = "Учётные записи", body = Vec<UserRow>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
/// Учётные записи — целиком администраторское: ТЗ отдаёт ему
/// и создание, и назначение ролей, и блокировку.
async fn list_users(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<UserRow>>> {
    user.require(&[Role::Admin])?;

    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT u.id, u.login, u.fio, u.role::text AS role,
                u.service_id, s.title AS service_title,
                u.workstation, u.is_active, u.created_at,
                (SELECT count(*) FROM sessions se WHERE se.user_id = u.id) AS sessions
           FROM users u
           LEFT JOIN services s ON s.id = u.service_id
          ORDER BY u.role, u.login",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewUser {
    pub login: String,
    pub password: String,
    pub fio: String,
    pub role: String,
    pub service_id: Option<String>,
    pub workstation: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/users",
    tag = "Администрирование",
    responses(
        (status = 200, description = "Завести учётную запись", body = UserRow),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn create_user(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<NewUser>,
) -> AppResult<(StatusCode, Json<UserRow>)> {
    user.require(&[Role::Admin])?;

    if req.login.trim().is_empty() || req.fio.trim().is_empty() {
        return Err(AppError::Validation("логин и ФИО обязательны".into()));
    }
    // Короткий пароль на изолированном стенде не катастрофа, но
    // восемь символов — минимум, ниже которого нет смысла
    if req.password.chars().count() < 8 {
        return Err(AppError::Validation("пароль короче восьми символов".into()));
    }
    if !["student", "teacher", "admin"].contains(&req.role.as_str()) {
        return Err(AppError::Validation("неизвестная роль".into()));
    }
    // Служба у курсанта определяет, что он видит; у преподавателя
    // и администратора её быть не должно
    if req.role != "student" && req.service_id.is_some() {
        return Err(AppError::Validation(
            "служба задаётся только обучающемуся".into(),
        ));
    }
    // …и наоборот: курсанту она обязательна. Без неё лента по службе
    // отвечает отказом, и главный экран у такого курсанта пуст —
    // он не может даже начать учебный вызов
    if req.role == "student" && req.service_id.as_deref().unwrap_or("").is_empty() {
        return Err(AppError::Validation(
            "обучающемуся нужно указать службу: по ней отбирается его лента".into(),
        ));
    }

    let hash = Argon2::default()
        .hash_password(req.password.as_bytes())
        .map_err(|e| AppError::Other(anyhow::anyhow!("не удалось захешировать пароль: {e}")))?
        .to_string();

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id, workstation)
         VALUES ($1, $2, $3, $4::user_role, $5, $6)
         ON CONFLICT (login) DO NOTHING
         RETURNING id",
    )
    .bind(req.login.trim())
    .bind(&hash)
    .bind(req.fio.trim())
    .bind(&req.role)
    .bind(&req.service_id)
    .bind(&req.workstation)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict("такой логин уже занят".into()))?;

    // Пароль в аудит не попадает — только факт заведения
    audit::log(
        &state.db,
        audit::Event::new("user.create")
            .from_user(&user)
            .about("user", id)
            .with(json!({ "login": req.login, "role": req.role, "serviceId": req.service_id })),
    )
    .await;

    let row: UserRow = sqlx::query_as(
        "SELECT u.id, u.login, u.fio, u.role::text AS role,
                u.service_id, s.title AS service_title,
                u.workstation, u.is_active, u.created_at, 0::bigint AS sessions
           FROM users u LEFT JOIN services s ON s.id = u.service_id
          WHERE u.id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(row)))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct Active {
    pub active: bool,
}

/// Учётные записи не удаляем, а отключаем: на них ссылаются занятия,
/// карточки и записи аудита, и их история должна остаться читаемой.
#[utoipa::path(
    post,
    path = "/api/admin/users/{id}/active",
    tag = "Администрирование",
    params(
        ("id" = String, Path, description = "Идентификатор пользователя"),
    ),
    responses(
        (status = 204, description = "Включить или отключить запись"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn set_active(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<Active>,
) -> AppResult<StatusCode> {
    user.require(&[Role::Admin])?;
    if id == user.id && !req.active {
        return Err(AppError::Validation(
            "нельзя отключить собственную учётную запись".into(),
        ));
    }

    let updated = sqlx::query("UPDATE users SET is_active = $2 WHERE id = $1")
        .bind(id)
        .bind(req.active)
        .execute(&state.db)
        .await?
        .rows_affected();

    if updated == 0 {
        return Err(AppError::NotFound);
    }

    audit::log(
        &state.db,
        audit::Event::new(if req.active { "user.enable" } else { "user.disable" })
            .from_user(&user)
            .about("user", id),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionRow {
    pub id: Uuid,
    pub name: String,
    pub source_file: String,
    pub positions_count: i32,
    pub is_active: bool,
    pub imported_at: chrono::DateTime<chrono::Utc>,
}

/// Чем две версии классификатора отличаются.
///
/// Здесь преподаватель нужен: смена справочника означает, что часть
/// сценариев перестала соответствовать эталону, а курсантам нужна
/// переаттестация по новым позициям. Это его учебный процесс, а не
/// конфигурация системы — в отличие от самого переключения версии,
/// которое остаётся за администратором.
#[utoipa::path(
    get,
    path = "/api/classifier/versions/{a}/diff/{b}",
    tag = "Классификатор",
    params(
        ("a" = String, Path, description = "Версия, с которой сравниваем"),
        ("b" = String, Path, description = "Версия, с которой сравниваем"),
    ),
    responses(
        (status = 200, description = "Чем версии отличаются", body = crate::domain::version_diff::Diff),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn version_diff(
    State(state): State<AppState>,
    user: AuthUser,
    Path((a, b)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<crate::domain::version_diff::Diff>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    crate::domain::version_diff::diff(&state.db, a, b)
        .await
        .map(Json)
        .map_err(AppError::Other)
}

/// Кого задевает переход на эту версию.
///
/// Считается ДО переключения: преподаватель должен увидеть цену
/// решения заранее, а не обнаружить потом, что часть сценариев
/// оценивает не по тому справочнику.
#[utoipa::path(
    get,
    path = "/api/classifier/versions/{id}/impact",
    tag = "Классификатор",
    params(
        ("id" = String, Path, description = "Идентификатор версии"),
    ),
    responses(
        (status = 200, description = "Кого задевает переход на версию", body = crate::domain::version_diff::Impact),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn version_impact(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<crate::domain::version_diff::Impact>> {
    user.require(&[Role::Teacher, Role::Admin])?;

    let active: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
            .fetch_optional(&state.db)
            .await?;
    let Some(active) = active else {
        return Err(AppError::Conflict("нет активной версии классификатора".into()));
    };

    crate::domain::version_diff::impact(&state.db, active, id)
        .await
        .map(Json)
        .map_err(AppError::Other)
}

/// Сделать версию активной.
///
/// По ней считается и список оповещения, и эталон при разборе.
/// Уже выставленные оценки не пересчитываются: они зафиксировали тот
/// справочник, что действовал в момент сохранения.
#[utoipa::path(
    post,
    path = "/api/admin/classifier-versions/{id}/activate",
    tag = "Классификатор",
    params(
        ("id" = String, Path, description = "Идентификатор версии"),
    ),
    responses(
        (status = 200, description = "Сделать версию активной", body = ActivateResult),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn activate_version(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ActivateResult>> {
    user.require(&[Role::Admin])?;

    let name: Option<String> =
        sqlx::query_scalar("SELECT name FROM classifier_versions WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let Some(name) = name else {
        return Err(AppError::NotFound);
    };

    // Цену считаем до переключения: после него «что изменилось»
    // уже не восстановить
    let previous: Option<String> =
        sqlx::query_scalar("SELECT name FROM classifier_versions WHERE is_active")
            .fetch_optional(&state.db)
            .await?;
    let active: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
            .fetch_optional(&state.db)
            .await?;
    let impact = match active {
        Some(active) if active != id => {
            Some(crate::domain::version_diff::impact(&state.db, active, id)
                .await
                .map_err(AppError::Other)?)
        }
        _ => None,
    };

    let mut tx = state.db.begin().await?;

    // Активная ровно одна: частичный индекс на is_active этого не
    // гарантирует, гарантируем порядком операций в транзакции
    sqlx::query("UPDATE classifier_versions SET is_active = false WHERE is_active")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE classifier_versions SET is_active = true WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    audit::log(
        &state.db,
        audit::Event::new("classifier.activate")
            .from_user(&user)
            .about("classifier_version", id)
            .with(json!({
                "name": name,
                "previous": previous,
                // Сколько сценариев перестало соответствовать эталону
                // и сколько новых позиций осталось без тренировки —
                // это и есть цена переключения
                "scenariosDrifted": impact.as_ref().map(|i| i.scenarios_drifted.len()),
                "positionsUntrained": impact.as_ref().map(|i| i.positions_untrained.len()),
                "studentsUncovered": impact.as_ref().map(|i| i.students_uncovered.len()),
            })),
    )
    .await;

    tracing::info!(%id, %name, "активная версия классификатора изменена");
    Ok(Json(ActivateResult {
        activated: name,
        impact: impact.unwrap_or(crate::domain::version_diff::Impact {
            scenarios_drifted: Vec::new(),
            students_uncovered: Vec::new(),
            positions_untrained: Vec::new(),
        }),
    }))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivateResult {
    pub activated: String,
    /// Кого это задело. Считается до переключения, по прежней активной
    pub impact: crate::domain::version_diff::Impact,
}

/// Какие версии классификатора загружены. По ТЗ система должна
/// переживать их смену, и видеть, что сейчас активно, обязательно.
#[utoipa::path(
    get,
    path = "/api/admin/classifier-versions",
    tag = "Классификатор",
    responses(
        (status = 200, description = "Загруженные версии", body = Vec<VersionRow>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn versions(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<VersionRow>>> {
    user.require(&[Role::Admin])?;

    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT id, name, source_file, positions_count, is_active, imported_at
           FROM classifier_versions ORDER BY imported_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows))
}
