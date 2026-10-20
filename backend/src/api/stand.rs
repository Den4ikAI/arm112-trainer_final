//! Что экран входа знает о стенде до входа.
//!
//! Обратная связь экспертов 18.10: логины и пароли учебного стенда —
//! на экран входа, «чтобы мы не искали их в документации»; приём вызовов
//! 112 свернуть, чтобы он не отвлекал от работы диспетчера ДДС.
//!
//! Учётные записи показываются, только если на сервере задан
//! `DEMO_PASSWORD` (демонстрационный стенд), и только те, к которым этот
//! пароль действительно подходит: сменённый пароль в списке не появится.

use crate::{error::AppResult, state::AppState};
use argon2::Argon2;
use axum::{extract::State, routing::get, Json, Router};
use password_hash::{phc::PasswordHash, PasswordVerifier};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/stand", get(stand))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Stand {
    /// Показывать ли рабочее место оператора 112 (параметр `ui.op112`)
    pub op112: bool,
    /// Учебные учётные записи; `None` — стенд не демонстрационный
    pub demo: Option<Demo>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Demo {
    /// Общий пароль учебных учётных записей
    pub password: String,
    pub accounts: Vec<DemoAccount>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DemoAccount {
    pub login: String,
    pub fio: String,
    /// `teacher`, `admin`, `student`
    pub role: String,
    /// У курсанта: `dds` или `op112`
    pub duty: Option<String>,
    /// Краткое название службы диспетчера ДДС
    pub service: Option<String>,
}

/// Подходит ли пароль к хешу. Проверка argon2 стоит десятки
/// миллисекунд, а экран входа открывают часто — помним ответ по паре
/// «пароль, хеш»: сменят пароль учётной записи — сменится и хеш.
static ПРОВЕРЕНО: LazyLock<Mutex<HashMap<(String, String), bool>>> =
    LazyLock::new(Default::default);

fn подходит(password: &str, hash: &str) -> bool {
    let key = (password.to_string(), hash.to_string());
    if let Some(&v) = ПРОВЕРЕНО.lock().unwrap().get(&key) {
        return v;
    }
    let ok = PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        .unwrap_or(false);
    ПРОВЕРЕНО.lock().unwrap().insert(key, ok);
    ok
}

/// Включено ли рабочее место оператора 112. Нет параметра — включено:
/// так ведёт себя база до миграции, где он появился.
pub async fn op112_enabled(pool: &sqlx::PgPool) -> AppResult<bool> {
    let v: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'ui.op112'")
            .fetch_optional(pool)
            .await?;
    Ok(v.and_then(|v| v.as_f64()).is_none_or(|x| x >= 0.5))
}

#[utoipa::path(
    get,
    path = "/api/stand",
    tag = "Состояние",
    responses(
        (status = 200, description = "Что показать на экране входа; без входа", body = Stand),
    ),
)]
pub async fn stand(State(state): State<AppState>) -> AppResult<Json<Stand>> {
    let op112 = op112_enabled(&state.db).await?;

    let Some(password) = state.config.demo_password.clone() else {
        return Ok(Json(Stand { op112, demo: None }));
    };

    // По одной записи на роль: администратор, преподаватель, диспетчер
    // каждой службы — в порядке окна «Добавьте службы»; оператор 112 —
    // если его рабочее место не свёрнуто
    let rows: Vec<(String, String, String, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT DISTINCT ON (u.role, u.duty, u.service_id)
                    u.login, u.fio, u.role::text, u.duty::text, sc.short_title, u.password_hash
               FROM users u
               LEFT JOIN service_catalog sc ON sc.service_id = u.service_id
              WHERE u.is_active
                -- Диспетчер — службы из окна «Добавьте службы»: у неё
                -- есть короткое имя для подписи; оператор — если не свёрнут
                AND (u.role <> 'student'
                     OR (u.duty = 'dds' AND sc.service_id IS NOT NULL)
                     OR (u.duty = 'op112' AND $1))
              ORDER BY u.role, u.duty, u.service_id, u.login",
        )
        .bind(op112)
        .fetch_all(&state.db)
        .await?;

    let порядок = |role: &str, duty: Option<&str>| match (role, duty) {
        ("teacher", _) => 0,
        ("student", Some("dds")) => 1,
        ("student", _) => 2,
        _ => 3,
    };
    let позиция: HashMap<String, i32> = sqlx::query_as::<_, (String, i32)>(
        "SELECT short_title, position FROM service_catalog",
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .collect();

    let mut accounts: Vec<DemoAccount> = Vec::new();
    for (login, fio, role, duty, service, hash) in rows {
        // Проверка argon2 занимает поток — уводим её с исполнителя
        let p = password.clone();
        let ok = tokio::task::spawn_blocking(move || подходит(&p, &hash))
            .await
            .unwrap_or(false);
        if ok {
            accounts.push(DemoAccount { login, fio, role, duty, service });
        }
    }
    accounts.sort_by_key(|a| {
        (
            порядок(&a.role, a.duty.as_deref()),
            a.service.as_ref().and_then(|s| позиция.get(s)).copied().unwrap_or(i32::MAX),
            a.login.clone(),
        )
    });

    Ok(Json(Stand { op112, demo: Some(Demo { password, accounts }) }))
}
