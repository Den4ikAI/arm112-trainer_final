//! Телефонный справочник ДДС, который формирует преподаватель.
//!
//! Рекомендации финалистам, п. 8: у диспетчера — «виртуальный IP-телефон
//! с возможностью набора номера и виртуального общения с абонентами
//! (руководитель организации, начальник отдела реагирования, начальник
//! аварийной бригады, специалист аварийной бригады), перечень
//! специалистов и номера условных телефонов должен формироваться
//! преподавателем, не более 10 номеров для одной ДДС». Заявитель —
//! отдельная роль: диспетчер звонит ему, только если преподаватель
//! включил его в перечень, и набирает номер из карточки.
//!
//! Справочник один на службу: все её диспетчеры видят одни и те же
//! номера. Правит преподаватель или администратор, курсант читает свой.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/phonebook", get(view).put(save).delete(reset))
}

/// «Не более 10 номеров для одной ДДС» — из рекомендаций финалистам.
pub const MAX_ENTRIES: usize = 10;

/// Роли абонентов из рекомендаций, в порядке показа.
pub const ROLES: [(&str, &str); 5] = [
    ("org_head", "Руководитель организации"),
    ("response_head", "Начальник отдела реагирования"),
    ("brigade_head", "Начальник аварийной бригады"),
    ("brigade_specialist", "Специалист аварийной бригады"),
    ("applicant", "Заявитель (по карточке)"),
];

pub fn role_title(role: &str) -> &'static str {
    ROLES.iter().find(|(r, _)| *r == role).map(|(_, t)| *t).unwrap_or("Абонент")
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// `org_head`, `response_head`, `brigade_head`, `brigade_specialist`, `applicant`
    pub role: String,
    /// Внутренний номер, 3–6 цифр. У заявителя пустой — берётся из карточки
    pub number: String,
    /// Как абонент подписан в телефоне: должность или ФИО
    pub title: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleInfo {
    pub role: String,
    pub title: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneBookView {
    pub service_id: String,
    pub entries: Vec<Entry>,
    /// Преподаватель записи не правил — показан справочник по умолчанию
    pub is_default: bool,
    pub max: usize,
    pub roles: Vec<RoleInfo>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ServiceQuery {
    /// Служба. Курсанту не нужна — берётся своя
    pub service_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveBook {
    pub entries: Vec<Entry>,
}

/// Справочник по умолчанию: та же раздача, что в миграции, для служб,
/// заведённых позже. Начальник аварийной бригады — прежний «старший
/// бригады», начальник отдела реагирования — прежний «вышестоящий
/// начальник» (последний номер сотни).
pub fn defaults(base: i32) -> Vec<Entry> {
    [("brigade_head", 1), ("brigade_specialist", 2), ("response_head", 19), ("org_head", 18)]
        .into_iter()
        .map(|(role, off)| Entry {
            role: role.into(),
            number: (base + off).to_string(),
            title: role_title(role).into(),
        })
        .collect()
}

/// Записи службы; нет своих — по умолчанию.
pub async fn entries(pool: &PgPool, service_id: &str) -> AppResult<(Vec<Entry>, bool)> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT role, number, title FROM phone_book WHERE service_id = $1 ORDER BY position, id",
    )
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        let base = crate::api::phone::base_number(pool, service_id).await?;
        return Ok((defaults(base), true));
    }
    Ok((rows.into_iter().map(|(role, number, title)| Entry { role, number, title }).collect(), false))
}

/// Чей справочник показывать: курсанту — свой, преподавателю — любой.
fn service_for(user: &AuthUser, requested: Option<String>) -> AppResult<String> {
    let requested = requested.filter(|s| !s.is_empty());
    if user.role == Role::Student {
        let own = user
            .service_id
            .clone()
            .ok_or_else(|| AppError::Validation("у пользователя не задана служба".into()))?;
        if requested.is_some_and(|r| r != own) {
            return Err(AppError::Forbidden);
        }
        return Ok(own);
    }
    requested.ok_or_else(|| AppError::Validation("укажите службу: serviceId".into()))
}

async fn service_exists(pool: &PgPool, service_id: &str) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM services WHERE id = $1)")
        .bind(service_id)
        .fetch_one(pool)
        .await?;
    if exists { Ok(()) } else { Err(AppError::NotFound) }
}

async fn view_of(pool: &PgPool, service_id: String) -> AppResult<PhoneBookView> {
    let (entries, is_default) = entries(pool, &service_id).await?;
    Ok(PhoneBookView {
        service_id,
        entries,
        is_default,
        max: MAX_ENTRIES,
        roles: ROLES.iter().map(|(r, t)| RoleInfo { role: (*r).into(), title: (*t).into() }).collect(),
    })
}

#[utoipa::path(
    get,
    path = "/api/phonebook",
    tag = "IP-телефон",
    params(ServiceQuery),
    responses(
        (status = 200, description = "Справочник службы", body = PhoneBookView),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Курсант видит только свою службу"),
        (status = 404, description = "Службы нет"),
    ),
)]
pub async fn view(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ServiceQuery>,
) -> AppResult<Json<PhoneBookView>> {
    let service_id = service_for(&user, q.service_id)?;
    service_exists(&state.db, &service_id).await?;
    Ok(Json(view_of(&state.db, service_id).await?))
}

/// Записи, какими их можно хранить: роли известны, номера — цифры и
/// без повторов, подписи есть, всего не больше десяти.
fn validate(entries: &[Entry]) -> AppResult<Vec<Entry>> {
    if entries.len() > MAX_ENTRIES {
        return Err(AppError::Validation(format!("не более {MAX_ENTRIES} номеров для одной ДДС")));
    }
    let mut clean = Vec::with_capacity(entries.len());
    let mut numbers = std::collections::HashSet::new();
    for e in entries {
        if !ROLES.iter().any(|(r, _)| *r == e.role) {
            return Err(AppError::Validation(format!("неизвестная роль абонента: {}", e.role)));
        }
        let title = e.title.trim();
        if title.is_empty() || title.chars().count() > 80 {
            return Err(AppError::Validation("подпись абонента — от 1 до 80 знаков".into()));
        }
        let number = e.number.trim().to_string();
        if e.role == "applicant" {
            // Номер заявителя — в карточке; в справочнике он один
            if !numbers.insert(String::new()) {
                return Err(AppError::Validation("заявитель в справочнике один".into()));
            }
            clean.push(Entry { role: e.role.clone(), number: String::new(), title: title.into() });
            continue;
        }
        if !(3..=6).contains(&number.len()) || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AppError::Validation(format!("номер «{number}» — это 3–6 цифр")));
        }
        if !numbers.insert(number.clone()) {
            return Err(AppError::Validation(format!("номер {number} повторяется")));
        }
        clean.push(Entry { role: e.role.clone(), number, title: title.into() });
    }
    Ok(clean)
}

#[utoipa::path(
    put,
    path = "/api/phonebook",
    tag = "IP-телефон",
    params(ServiceQuery),
    request_body = SaveBook,
    responses(
        (status = 200, description = "Справочник сохранён", body = PhoneBookView),
        (status = 400, description = "Больше 10 номеров, повтор номера, неизвестная роль"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Правит преподаватель или администратор"),
        (status = 404, description = "Службы нет"),
    ),
)]
pub async fn save(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ServiceQuery>,
    Json(req): Json<SaveBook>,
) -> AppResult<Json<PhoneBookView>> {
    if user.role == Role::Student {
        return Err(AppError::Forbidden);
    }
    let service_id = service_for(&user, q.service_id)?;
    service_exists(&state.db, &service_id).await?;
    let entries = validate(&req.entries)?;

    // Справочник заменяется целиком: преподаватель правит таблицу и
    // сохраняет её, а не отдельные строки
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM phone_book WHERE service_id = $1")
        .bind(&service_id)
        .execute(&mut *tx)
        .await?;
    for (i, e) in entries.iter().enumerate() {
        sqlx::query(
            "INSERT INTO phone_book (service_id, role, number, title, position) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&service_id)
        .bind(&e.role)
        .bind(&e.number)
        .bind(&e.title)
        .bind(i as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    audit::log(
        &state.db,
        audit::Event::new("phonebook.save")
            .from_user(&user)
            .about("service", &service_id)
            .with(json!({ "entries": entries })),
    )
    .await;
    Ok(Json(view_of(&state.db, service_id).await?))
}

#[utoipa::path(
    delete,
    path = "/api/phonebook",
    tag = "IP-телефон",
    params(ServiceQuery),
    responses(
        (status = 200, description = "Свои записи удалены, служба снова со справочником по умолчанию", body = PhoneBookView),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Правит преподаватель или администратор"),
        (status = 404, description = "Службы нет"),
    ),
)]
pub async fn reset(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ServiceQuery>,
) -> AppResult<Json<PhoneBookView>> {
    if user.role == Role::Student {
        return Err(AppError::Forbidden);
    }
    let service_id = service_for(&user, q.service_id)?;
    service_exists(&state.db, &service_id).await?;
    sqlx::query("DELETE FROM phone_book WHERE service_id = $1")
        .bind(&service_id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state.db,
        audit::Event::new("phonebook.reset").from_user(&user).about("service", &service_id),
    )
    .await;
    Ok(Json(view_of(&state.db, service_id).await?))
}
