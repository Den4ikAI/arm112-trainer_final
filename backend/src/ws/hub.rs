//! Шина событий.
//!
//! Один широковещательный канал на весь процесс, фильтрация — на
//! стороне подписчика. Карта «топик → канал» здесь была бы лишней:
//! аудитория одного стенда это учебный класс, десятки соединений,
//! а карту пришлось бы чистить от пустых топиков и сторожить гонки
//! между подпиской и отправкой.
//!
//! Если стендов станет много, это место меняется на Redis pub/sub
//! без изменения остального кода: наружу торчат только `publish`
//! и `subscribe`.

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// Сколько событий держим для отставшего подписчика. Больше —
/// и медленный клиент начнёт есть память; меньше — начнёт терять
/// события при любом всплеске.
const CAPACITY: usize = 256;

/// Событие вместе с адресами, по которым его ждут.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// `service:mchs`, `session:<uuid>`, `group:<uuid>`, `user:<uuid>`
    pub topics: Vec<String>,
    /// `card.dispatched`, `response.recorded`, `ack.timeout`, …
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub payload: Value,
}

impl Event {
    pub fn new(kind: &str, payload: Value) -> Self {
        Self { topics: Vec::new(), kind: kind.to_string(), payload }
    }

    pub fn to(mut self, topic: impl Into<String>) -> Self {
        self.topics.push(topic.into());
        self
    }

    pub fn to_all(mut self, topics: impl IntoIterator<Item = String>) -> Self {
        self.topics.extend(topics);
        self
    }
}

#[derive(Clone)]
pub struct Hub {
    tx: broadcast::Sender<Event>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CAPACITY);
        Self { tx }
    }

    /// Отправить событие. Отсутствие подписчиков — не ошибка:
    /// занятие идёт и без открытого журнала у преподавателя.
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Сколько соединений слушает шину. Нужно для `/api/admin/health`.
    pub fn listeners(&self) -> usize {
        self.tx.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn событие_доходит_до_подписчика() {
        let hub = Hub::new();
        let mut rx = hub.subscribe();

        hub.publish(Event::new("card.dispatched", json!({ "cardId": "1" })).to("service:mchs"));

        let got = rx.recv().await.unwrap();
        assert_eq!(got.kind, "card.dispatched");
        assert_eq!(got.topics, vec!["service:mchs"]);
    }

    /// Занятие идёт и без открытого журнала у преподавателя.
    #[tokio::test]
    async fn отправка_без_подписчиков_не_ошибка() {
        let hub = Hub::new();
        hub.publish(Event::new("session.started", json!({})));
        assert_eq!(hub.listeners(), 0);
    }

    /// Событие получают все подписчики, а не первый попавшийся:
    /// одну карточку ждут и диспетчер, и преподаватель.
    #[tokio::test]
    async fn событие_получают_все_подписчики() {
        let hub = Hub::new();
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();

        hub.publish(Event::new("ack.timeout", json!({ "cardId": "7" })).to("service:mchs"));

        assert_eq!(a.recv().await.unwrap().kind, "ack.timeout");
        assert_eq!(b.recv().await.unwrap().kind, "ack.timeout");
    }

    /// В сериализации поля события лежат рядом с типом, а не вложены:
    /// клиенту удобнее читать `{type, cardId}`, чем `{type, payload}`.
    #[test]
    fn полезная_нагрузка_разворачивается_в_корень() {
        let e = Event::new("response.recorded", json!({ "cardId": "1", "status": "Принята" }));
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "response.recorded");
        assert_eq!(v["cardId"], "1");
        assert_eq!(v["status"], "Принята");
    }
}
