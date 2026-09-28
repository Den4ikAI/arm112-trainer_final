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
    routing::{delete, get, post},
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
        .route("/api/admin/users/{id}", delete(delete_user))
        .route("/api/users/{id}/duty", axum::routing::patch(set_duty))
        .route("/api/groups/{id}/members", get(group_members))
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
    /// Профиль курсанта: `op112` или `dds`; у преподавателя и
    /// администратора — `null`
    pub duty: Option<String>,
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
                u.service_id, s.title AS service_title, u.duty::text AS duty,
                u.workstation, u.is_active, u.created_at,
                (SELECT count(*) FROM sessions se WHERE se.user_id = u.id) AS sessions
           FROM users u
           LEFT JOIN services s ON s.id = u.service_id
          -- Удалённые обезличены и в списке не нужны: их история видна
          -- в отчётах и журнале действий
          WHERE u.deleted_at IS NULL
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
    /// Профиль курсанта: `op112` или `dds`. Не задан — выводится из
    /// службы: со службой — ДДС, без неё — оператор 112
    #[serde(default)]
    pub duty: Option<String>,
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
    // Профиль рабочего места: диспетчеру ДДС служба обязательна — по
    // ней отбирается его лента, оператору 112 она не нужна
    let service = req.service_id.clone().filter(|s| !s.trim().is_empty());
    let duty = if req.role == "student" {
        Some(duty_for(req.duty.as_deref(), service.as_deref())?)
    } else {
        if req.duty.is_some() {
            return Err(AppError::Validation("профиль задаётся только обучающемуся".into()));
        }
        None
    };

    let hash = Argon2::default()
        .hash_password(req.password.as_bytes())
        .map_err(|e| AppError::Other(anyhow::anyhow!("не удалось захешировать пароль: {e}")))?
        .to_string();

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id, workstation, duty)
         VALUES ($1, $2, $3, $4::user_role, $5, $6, $7::user_duty)
         ON CONFLICT (login) DO NOTHING
         RETURNING id",
    )
    .bind(req.login.trim())
    .bind(&hash)
    .bind(req.fio.trim())
    .bind(&req.role)
    .bind(&service)
    .bind(&req.workstation)
    .bind(duty)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Conflict("такой логин уже занят".into()))?;

    // Пароль в аудит не попадает — только факт заведения
    audit::log(
        &state.db,
        audit::Event::new("user.create")
            .from_user(&user)
            .about("user", id)
            .with(json!({ "login": req.login, "role": req.role, "serviceId": service,
                          "duty": duty })),
    )
    .await;

    let row: UserRow = sqlx::query_as(
        "SELECT u.id, u.login, u.fio, u.role::text AS role,
                u.service_id, s.title AS service_title, u.duty::text AS duty,
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

    // Удалённую запись обратно не включаем: она обезличена
    let updated = sqlx::query(
        "UPDATE users SET is_active = $2 WHERE id = $1 AND deleted_at IS NULL",
    )
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

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    /// `deleted` — записи не было истории, она стёрта; `anonymized` —
    /// история есть, запись обезличена после резервной копии
    pub mode: String,
    /// Резервная копия данных записи — таблица `user_backups`
    pub backup_id: Option<Uuid>,
}

/// Есть ли у записи история: учебная работа, действия, материалы.
/// Такую нельзя стирать — на неё ссылаются оценки и журнал действий.
async fn has_history(pool: &sqlx::PgPool, id: Uuid) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sessions WHERE user_id = $1)
             OR EXISTS (SELECT 1 FROM cards WHERE created_by = $1)
             OR EXISTS (SELECT 1 FROM response_entries WHERE user_id = $1)
             OR EXISTS (SELECT 1 FROM lessons WHERE teacher_id = $1)
             OR EXISTS (SELECT 1 FROM training_groups WHERE teacher_id = $1)
             OR EXISTS (SELECT 1 FROM assignments WHERE created_by = $1 OR user_id = $1)
             OR EXISTS (SELECT 1 FROM scenarios WHERE created_by = $1 OR approved_by = $1)
             OR EXISTS (SELECT 1 FROM expert_grades WHERE teacher_id = $1)
             OR EXISTS (SELECT 1 FROM audit_log WHERE user_id = $1)",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}

/// Удалить учётную запись.
///
/// ТЗ запрещает удалять критически важные данные без резервного
/// копирования. Поэтому:
///
/// * записи без истории (завели и ни разу не вошли) удаляются сразу —
///   терять нечего;
/// * запись с историей сначала копируется в `user_backups`, затем
///   обезличивается: ФИО и логин заменяются, вход закрыт, из групп
///   человек убран. Оценки, карточки и журнал действий остаются и
///   читаются — ссылки на запись не рвутся.
///
/// Свою запись и последнего администратора удалить нельзя: система
/// осталась бы без управления.
#[utoipa::path(
    delete,
    path = "/api/admin/users/{id}",
    tag = "Администрирование",
    params(("id" = String, Path, description = "Идентификатор пользователя")),
    responses(
        (status = 200, description = "Запись удалена или обезличена", body = Deleted),
        (status = 400, description = "Свою запись или последнего администратора удалить нельзя"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Нет такой записи"),
    ),
)]
pub async fn delete_user(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Deleted>> {
    user.require(&[Role::Admin])?;
    if id == user.id {
        return Err(AppError::Validation("нельзя удалить собственную учётную запись".into()));
    }

    let row: Option<(String, String, String, Option<String>, Option<String>, bool, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT login, fio, role::text, service_id, workstation, is_active, created_at
               FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let Some((login, fio, role, service_id, workstation, is_active, created_at)) = row else {
        return Err(AppError::NotFound);
    };

    if role == "admin" {
        let других: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users
              WHERE role = 'admin' AND is_active AND deleted_at IS NULL AND id <> $1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await?;
        if других == 0 {
            return Err(AppError::Validation(
                "это последний администратор: без него системой некому управлять".into(),
            ));
        }
    }

    if !has_history(&state.db, id).await? {
        // Ссылки, которых нет в проверке истории (членство в группе,
        // прогнозы), уходят каскадом. Нашлась ссылка без каскада —
        // значит, история всё-таки есть: идём по пути обезличивания
        let mut tx = state.db.begin().await?;
        match sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(&mut *tx).await {
            Ok(_) => {
                tx.commit().await?;
                audit::log(
                    &state.db,
                    audit::Event::new("user.delete")
                        .from_user(&user)
                        .about("user", id)
                        .with(json!({ "mode": "deleted", "login": login, "role": role })),
                )
                .await;
                return Ok(Json(Deleted { mode: "deleted".into(), backup_id: None }));
            }
            Err(sqlx::Error::Database(e)) if e.is_foreign_key_violation() => {
                tx.rollback().await?;
            }
            Err(e) => return Err(e.into()),
        }
    }

    let mut tx = state.db.begin().await?;

    // Резервная копия — до обезличивания и в той же транзакции:
    // не записалась копия — не трогаем и запись
    let groups: Vec<String> = sqlx::query_scalar(
        "SELECT g.title FROM group_members m JOIN training_groups g ON g.id = m.group_id
          WHERE m.user_id = $1 ORDER BY g.title",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let sessions: Vec<(Uuid, Option<Uuid>, chrono::DateTime<chrono::Utc>, Option<i32>)> = sqlx::query_as(
        "SELECT s.id, s.scenario_id, s.started_at, g.score
           FROM sessions s LEFT JOIN grades g ON g.session_id = s.id
          WHERE s.user_id = $1 ORDER BY s.started_at",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;

    let data = json!({
        "user": {
            "id": id, "login": login, "fio": fio, "role": role,
            "serviceId": service_id, "workstation": workstation,
            "isActive": is_active, "createdAt": created_at,
        },
        "groups": groups,
        "sessions": sessions.iter().map(|(sid, scenario, at, score)| json!({
            "id": sid, "scenarioId": scenario, "startedAt": at, "score": score,
        })).collect::<Vec<_>>(),
    });

    let backup_id: Uuid = sqlx::query_scalar(
        "INSERT INTO user_backups (user_id, data, created_by) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(id)
    .bind(&data)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;

    // Пароль — хеш случайной строки: вход закрыт наверняка, а код,
    // который разбирает хеш, не спотыкается о пустое значение
    let hash = Argon2::default()
        .hash_password(Uuid::new_v4().to_string().as_bytes())
        .map_err(|e| AppError::Other(anyhow::anyhow!("не удалось захешировать пароль: {e}")))?
        .to_string();

    sqlx::query(
        "UPDATE users
            SET login = $2, fio = 'Удалённая учётная запись', password_hash = $3,
                workstation = NULL, is_active = false, deleted_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(format!("удалён-{}", id.simple()))
    .bind(&hash)
    .execute(&mut *tx)
    .await?;

    // Из групп убираем: иначе обезличенная запись висела бы в ходе
    // занятий и в списках преподавателя
    sqlx::query("DELETE FROM group_members WHERE user_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // ФИО в журнал не пишем — оно и так в резервной копии, а журнал
    // действий читают шире
    audit::log(
        &state.db,
        audit::Event::new("user.delete")
            .from_user(&user)
            .about("user", id)
            .with(json!({ "mode": "anonymized", "role": role, "backupId": backup_id })),
    )
    .await;

    Ok(Json(Deleted { mode: "anonymized".into(), backup_id: Some(backup_id) }))
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

    // Опросные карты, выведенные из классификатора, следуют за версией:
    // дерево признаков в новой версии могло поменяться
    let cards = crate::ingest::survey::materialise(&state.db, id)
        .await
        .map_err(crate::error::AppError::Other)?;
    tracing::info!(cards, "опросные карты пересобраны по новой версии");

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


// ─── Профиль рабочего места курсанта ─────────────────────────────────

/// Проверить пару «профиль — служба» и вернуть профиль.
///
/// Диспетчеру ДДС служба обязательна: по ней отбирается его лента.
/// Оператору 112 служба не нужна, и с ней он путался бы в двух ролях
/// сразу. Профиль не задан — выводится из службы.
fn duty_for(duty: Option<&str>, service: Option<&str>) -> AppResult<&'static str> {
    match (duty, service) {
        (Some("dds"), Some(_)) | (None, Some(_)) => Ok("dds"),
        (Some("dds"), None) => Err(AppError::Validation(
            "диспетчеру ДДС нужно указать службу: по ней отбирается его лента".into(),
        )),
        (Some("op112"), None) | (None, None) => Ok("op112"),
        (Some("op112"), Some(_)) => Err(AppError::Validation(
            "у оператора 112 службы нет: он принимает вызовы, а не работает за ДДС".into(),
        )),
        _ => Err(AppError::Validation("профиль — «op112» или «dds»".into())),
    }
}

/// Преподаватель меняет профиль только своим курсантам, администратор —
/// любому курсанту.
async fn may_manage(pool: &sqlx::PgPool, actor: &AuthUser, student: Uuid) -> AppResult<()> {
    match actor.role {
        Role::Admin => Ok(()),
        Role::Teacher => {
            let mine: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT 1 FROM group_members m
                      JOIN training_groups g ON g.id = m.group_id
                     WHERE m.user_id = $1 AND g.teacher_id = $2)",
            )
            .bind(student)
            .bind(actor.id)
            .fetch_one(pool)
            .await?;
            if mine { Ok(()) } else { Err(AppError::Forbidden) }
        }
        Role::Student => Err(AppError::Forbidden),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DutyChange {
    /// `op112` или `dds`
    pub duty: String,
    /// Служба диспетчера ДДС; для оператора 112 не указывается
    pub service_id: Option<String>,
}

/// Направить курсанта на другое рабочее место.
///
/// Организатор, чат 18.09: обучающегося «могут направить на практику
/// в одно или другое место в зависимости от результатов». Действует
/// сразу, без перевхода: сервер берёт профиль из базы на каждом запросе.
#[utoipa::path(
    patch,
    path = "/api/users/{id}/duty",
    tag = "Администрирование",
    params(
        ("id" = String, Path, description = "Идентификатор курсанта"),
    ),
    responses(
        (status = 200, description = "Профиль изменён", body = UserRow),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn set_duty(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<DutyChange>,
) -> AppResult<Json<UserRow>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    may_manage(&state.db, &user, id).await?;

    let role: Option<String> =
        sqlx::query_scalar("SELECT role::text FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    match role.as_deref() {
        None => return Err(AppError::NotFound),
        Some("student") => {}
        Some(_) => {
            return Err(AppError::Validation("профиль задаётся только обучающемуся".into()))
        }
    }

    let service = req.service_id.clone().filter(|s| !s.trim().is_empty());
    let duty = duty_for(Some(req.duty.as_str()), service.as_deref())?;
    if let Some(sid) = &service {
        let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM services WHERE id = $1)")
            .bind(sid)
            .fetch_one(&state.db)
            .await?;
        if !known {
            return Err(AppError::Validation("такой службы нет в справочнике".into()));
        }
    }

    let before: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT duty::text, service_id FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;

    sqlx::query("UPDATE users SET duty = $2::user_duty, service_id = $3 WHERE id = $1")
        .bind(id)
        .bind(duty)
        .bind(&service)
        .execute(&state.db)
        .await?;

    audit::log(
        &state.db,
        audit::Event::new("user.duty")
            .from_user(&user)
            .about("user", id)
            .with(json!({
                "было": { "duty": before.0, "serviceId": before.1 },
                "стало": { "duty": duty, "serviceId": service },
            })),
    )
    .await;

    let row: UserRow = sqlx::query_as(
        "SELECT u.id, u.login, u.fio, u.role::text AS role,
                u.service_id, s.title AS service_title, u.duty::text AS duty,
                u.workstation, u.is_active, u.created_at,
                (SELECT count(*) FROM sessions se WHERE se.user_id = u.id) AS sessions
           FROM users u LEFT JOIN services s ON s.id = u.service_id
          WHERE u.id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: Uuid,
    pub login: String,
    pub fio: String,
    /// `op112` или `dds`
    pub duty: Option<String>,
    pub service_id: Option<String>,
    pub service_title: Option<String>,
}

/// Курсанты группы с их профилями — чтобы преподаватель видел, кто
/// где работает, и мог направить на другое рабочее место.
#[utoipa::path(
    get,
    path = "/api/groups/{id}/members",
    tag = "Администрирование",
    params(
        ("id" = String, Path, description = "Идентификатор группы"),
    ),
    responses(
        (status = 200, description = "Курсанты группы", body = Vec<Member>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn group_members(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<Member>>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let teacher: Option<Uuid> =
        sqlx::query_scalar("SELECT teacher_id FROM training_groups WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let Some(teacher) = teacher else { return Err(AppError::NotFound) };
    if user.role == Role::Teacher && teacher != user.id {
        return Err(AppError::Forbidden);
    }

    let rows: Vec<Member> = sqlx::query_as(
        "SELECT u.id, u.login, u.fio, u.duty::text AS duty,
                u.service_id, s.title AS service_title
           FROM group_members m
           JOIN users u ON u.id = m.user_id
           LEFT JOIN services s ON s.id = u.service_id
          WHERE m.group_id = $1 AND u.deleted_at IS NULL
          ORDER BY u.fio",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}
