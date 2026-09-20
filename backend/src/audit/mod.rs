//! Журнал действий.
//!
//! Требование ТЗ — хранить журналы безопасности не менее 6 месяцев,
//! поэтому таблица партиционирована по месяцам. Запись идёт мимо
//! бизнес-транзакции: упавший аудит не должен отменять действие
//! пользователя, но обязан оставить след в логе.

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Событие аудита. Поля совпадают с колонками `audit_log`.
pub struct Event<'a> {
    pub user_id: Option<Uuid>,
    pub ip: Option<std::net::IpAddr>,
    /// Точечное имя: `card.status.auto`, `response.add`, `auth.login`
    pub action: &'a str,
    pub entity: Option<&'a str>,
    pub entity_id: Option<String>,
    pub payload: Option<Value>,
}

impl<'a> Event<'a> {
    pub fn new(action: &'a str) -> Self {
        Self { user_id: None, ip: None, action, entity: None, entity_id: None, payload: None }
    }

    pub fn by(mut self, user_id: Uuid) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Пользователь вместе с адресом: так не забудешь второе.
    pub fn from_user(mut self, user: &crate::api::auth::AuthUser) -> Self {
        self.user_id = Some(user.id);
        self.ip = user.ip;
        self
    }

    pub fn from_ip(mut self, ip: Option<std::net::IpAddr>) -> Self {
        self.ip = ip;
        self
    }

    pub fn about(mut self, entity: &'a str, id: impl ToString) -> Self {
        self.entity = Some(entity);
        self.entity_id = Some(id.to_string());
        self
    }

    pub fn with(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }
}

/// Записать событие. Ошибка не всплывает наверх: аудит не повод
/// отменять уже выполненное действие — но в лог она попадёт.
pub async fn log(pool: &PgPool, event: Event<'_>) {
    let result = sqlx::query(
        "INSERT INTO audit_log (user_id, action, entity, entity_id, payload, ip)
         VALUES ($1, $2, $3, $4, $5, $6::inet)",
    )
    .bind(event.user_id)
    .bind(event.action)
    .bind(event.entity)
    .bind(event.entity_id.as_deref())
    .bind(event.payload.as_ref())
    // Приводим в SQL, а не включаем ради одного поля поддержку inet
    // в sqlx: та тянет за собой ipnetwork со своими типами
    .bind(event.ip.map(|ip| ip.to_string()))
    .execute(pool)
    .await;

    if let Err(e) = result {
        tracing::error!(error = %e, action = event.action, "не удалось записать аудит");
    }
}
