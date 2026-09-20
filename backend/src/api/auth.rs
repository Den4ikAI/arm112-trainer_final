//! Аутентификация и разграничение доступа.
//!
//! Роль и служба — независимые оси. Роль говорит, что человек умеет
//! делать, служба — какие карточки он видит. ДДС это организация,
//! а не человек: требование ТЗ «чтобы в их ленту попадали только
//! профильные события» реализуется через `users.service_id`.

use crate::{
    audit,
    error::{AppError, AppResult},
    state::AppState,
};
use argon2::Argon2;
use axum::{
    extract::{FromRequestParts, State},
    http::request::Parts,
    routing::{get, post},
    Json, Router,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use password_hash::{phc::PasswordHash, PasswordVerifier};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Student,
    Teacher,
    Admin,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Идентификатор пользователя
    pub sub: Uuid,
    pub role: Role,
    /// Служба, к которой привязан пользователь. У преподавателя
    /// и администратора её нет.
    pub service_id: Option<String>,
    pub workstation: Option<String>,
    /// Истекает через JWT_TTL_HOURS
    pub exp: i64,
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    pub id: Uuid,
    pub login: String,
    pub fio: String,
    pub role: Role,
    pub service_id: Option<String>,
    pub workstation: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
    /// Номер АРМ: в боевой системе он вводится при входе
    #[serde(default)]
    pub workstation: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub token: String,
    pub user: UserView,
}

async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let row: Option<(Uuid, String, String, String, Role, Option<String>, Option<String>, bool)> =
        sqlx::query_as(
            "SELECT id, login, password_hash, fio, role, service_id, workstation, is_active
               FROM users WHERE login = $1",
        )
        .bind(&req.login)
        .fetch_optional(&state.db)
        .await?;

    // Одинаковый ответ на неизвестный логин и неверный пароль:
    // иначе по времени ответа можно перебирать логины
    let Some((id, login, hash, fio, role, service_id, ws, is_active)) = row else {
        // Неудачную попытку пишем без идентификатора: пользователя
        // с таким логином нет, но сама попытка — событие безопасности
        audit::log(
            &state.db,
            audit::Event::new("auth.login.failed")
                .from_ip(ip)
                .with(serde_json::json!({ "login": req.login, "reason": "нет такого логина" })),
        )
        .await;
        return Err(AppError::Unauthorized);
    };
    if !is_active {
        audit::log(
            &state.db,
            audit::Event::new("auth.login.failed")
                .by(id)
                .from_ip(ip)
                .with(serde_json::json!({ "login": login, "reason": "учётная запись отключена" })),
        )
        .await;
        return Err(AppError::Forbidden);
    }

    let parsed = PasswordHash::new(&hash).map_err(|_| AppError::Unauthorized)?;
    if Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed)
        .is_err()
    {
        audit::log(
            &state.db,
            audit::Event::new("auth.login.failed")
                .by(id)
                .from_ip(ip)
                .with(serde_json::json!({ "login": login, "reason": "неверный пароль" })),
        )
        .await;
        return Err(AppError::Unauthorized);
    }

    // Номер АРМ при входе перекрывает сохранённый в профиле
    let workstation = req.workstation.or(ws);

    let exp = (chrono::Utc::now() + chrono::Duration::hours(state.config.jwt_ttl_hours)).timestamp();
    let claims = Claims {
        sub: id,
        role,
        service_id: service_id.clone(),
        workstation: workstation.clone(),
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Other(e.into()))?;

    audit::log(
        &state.db,
        audit::Event::new("auth.login")
            .by(id)
            .from_ip(ip)
            .with(serde_json::json!({ "login": login, "workstation": workstation })),
    )
    .await;

    Ok(Json(LoginResponse {
        token,
        user: UserView { id, login, fio, role, service_id, workstation },
    }))
}

/// Токен живёт до истечения срока, серверного списка отзыва нет.
/// Для учебного стенда этого достаточно; фронтенд просто забывает токен.
async fn logout() -> axum::http::StatusCode {
    axum::http::StatusCode::NO_CONTENT
}

async fn me(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<UserView>> {
    let row: Option<UserView> = sqlx::query_as(
        "SELECT id, login, fio, role, service_id, workstation FROM users WHERE id = $1",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    let mut view = row.ok_or(AppError::NotFound)?;
    // Номер АРМ принадлежит сеансу, а не профилю: человек мог сесть
    // за другое рабочее место, и в карточке должен стоять тот номер,
    // который он ввёл при входе
    view.workstation = user.workstation;
    Ok(Json(view))
}

/// Адрес клиента.
///
/// В проде сервер стоит за обратным прокси, и адрес соединения — это
/// адрес прокси. Настоящий приходит в заголовке; берём первый элемент
/// `X-Forwarded-For` — это исходный клиент, остальные добавляются
/// промежуточными прокси.
///
/// Заголовкам можно верить только потому, что снаружи напрямую до
/// сервера не достучаться: по ТЗ система работает в изолированном
/// контуре. В открытой сети их пришлось бы перезаписывать на прокси.
fn client_ip(parts: &Parts) -> Option<std::net::IpAddr> {
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
    };

    header("x-forwarded-for")
        .or_else(|| header("x-real-ip"))
        .or_else(|| {
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|info| info.0.ip())
        })
}

/// Адрес клиента отдельно от пользователя: при входе его ещё нет,
/// а записать неудачную попытку надо именно с адресом — иначе
/// перебор паролей в журнале не отличить от забывчивости.
pub struct ClientIp(pub Option<std::net::IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(client_ip(parts)))
    }
}

// ─── Извлечение пользователя из запроса ──────────────────────────────

/// Аутентифицированный пользователь. Появление этого типа в аргументах
/// хендлера само по себе требует валидного токена.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: Role,
    pub service_id: Option<String>,
    pub workstation: Option<String>,
    /// Адрес клиента для аудита. За обратным прокси берётся из
    /// заголовков, напрямую — из самого соединения.
    pub ip: Option<std::net::IpAddr>,
}

impl AuthUser {
    /// Проверка роли на уровне хендлера, а не интерфейса.
    /// Фронтенд прячет кнопки, но запрос может прийти и мимо него.
    pub fn require(&self, allowed: &[Role]) -> AppResult<()> {
        if allowed.contains(&self.role) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;

        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| AppError::Unauthorized)?;

        Ok(AuthUser {
            id: data.claims.sub,
            role: data.claims.role,
            service_id: data.claims.service_id,
            workstation: data.claims.workstation,
            ip: client_ip(parts),
        })
    }
}
