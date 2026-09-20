//! Эндпоинт `/ws`.
//!
//! Аутентификация тем же JWT, что и REST, только в query: заголовок
//! Authorization браузерный WebSocket-клиент выставить не умеет.
//!
//! Подписка проверяется по тем же правилам, что и REST: курсант видит
//! свою службу и свои занятия, преподаватель — всё в своих группах.
//! Без этой проверки канал стал бы дырой в обход всего разграничения:
//! достаточно было бы подписаться на чужой топик.

use crate::{api::auth::Claims, api::auth::Role, state::AppState, ws::Event};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new().route("/ws", get(upgrade))
}

#[derive(Deserialize)]
pub struct Auth {
    pub token: String,
}

async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(auth): Query<Auth>,
) -> Response {
    let claims = decode::<Claims>(
        &auth.token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    );

    match claims {
        Ok(data) => ws.on_upgrade(move |socket| serve(socket, state, data.claims)),
        // До установки соединения отвечаем обычным HTTP: клиент
        // должен отличить протухший токен от разрыва сети
        Err(_) => IntoResponse::into_response(crate::error::AppError::Unauthorized),
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
    /// Клиент проверяет, жив ли канал
    Ping,
}

async fn serve(socket: WebSocket, state: AppState, claims: Claims) {
    let (mut sender, mut receiver) = socket.split();
    let mut events = state.ws.subscribe();
    let mut topics: HashSet<String> = HashSet::new();

    tracing::debug!(user = %claims.sub, "канал класса открыт");

    loop {
        tokio::select! {
            // Событие с шины: отдаём, если клиент на него подписан
            event = events.recv() => match event {
                Ok(event) => {
                    if !event.topics.iter().any(|t| topics.contains(t)) {
                        continue;
                    }
                    let Ok(text) = serde_json::to_string(&event) else { continue };
                    if sender.send(Message::text(text)).await.is_err() {
                        break;
                    }
                }
                // Клиент не успевал читать и отстал. Честно говорим
                // об этом: пусть перезапросит состояние через REST,
                // а не показывает устаревший журнал
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    let warn = json!({ "type": "lagged", "missed": n });
                    if sender.send(Message::text(warn.to_string())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            },

            // Сообщение от клиента
            incoming = receiver.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Subscribe { topics: want }) => {
                            for topic in want {
                                if allowed(&state, &claims, &topic).await {
                                    topics.insert(topic);
                                } else {
                                    let err = json!({
                                        "type": "error",
                                        "error": "forbidden_topic",
                                        "topic": topic,
                                    });
                                    let _ = sender.send(Message::text(err.to_string())).await;
                                }
                            }
                            let ack = json!({
                                "type": "subscribed",
                                "topics": topics.iter().collect::<Vec<_>>(),
                            });
                            if sender.send(Message::text(ack.to_string())).await.is_err() {
                                break;
                            }
                        }
                        Ok(ClientMessage::Unsubscribe { topics: drop }) => {
                            for topic in drop {
                                topics.remove(&topic);
                            }
                        }
                        Ok(ClientMessage::Ping) => {
                            if sender.send(Message::text("{\"type\":\"pong\"}")).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            let err = json!({ "type": "error", "error": "bad_message" });
                            let _ = sender.send(Message::text(err.to_string())).await;
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
        }
    }

    tracing::debug!(user = %claims.sub, "канал класса закрыт");
}

/// Можно ли подписаться на топик.
///
/// Правила те же, что в REST: иначе канал обошёл бы разграничение
/// доступа, а курсант увидел бы чужую ленту в обход фильтра по службе.
async fn allowed(state: &AppState, claims: &Claims, topic: &str) -> bool {
    let staff = claims.role != Role::Student;
    let Some((kind, id)) = topic.split_once(':') else {
        return false;
    };

    match kind {
        // Своя ДДС. Преподаватель ведёт занятие и видит все службы
        "service" => staff || claims.service_id.as_deref() == Some(id),

        "user" => staff || claims.sub.to_string() == id,

        "session" => {
            if staff {
                return true;
            }
            let Ok(session_id) = id.parse::<Uuid>() else { return false };
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM sessions WHERE id = $1 AND user_id = $2)",
            )
            .bind(session_id)
            .bind(claims.sub)
            .fetch_one(&state.db)
            .await
            .unwrap_or(false)
        }

        // Группа: участник, её преподаватель или администратор
        "group" => {
            let Ok(group_id) = id.parse::<Uuid>() else { return false };
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (
                    SELECT 1 FROM training_groups g
                     WHERE g.id = $1
                       AND ($3
                            OR g.teacher_id = $2
                            OR EXISTS (SELECT 1 FROM group_members m
                                        WHERE m.group_id = g.id AND m.user_id = $2)))",
            )
            .bind(group_id)
            .bind(claims.sub)
            .bind(claims.role == Role::Admin)
            .fetch_one(&state.db)
            .await
            .unwrap_or(false)
        }

        _ => false,
    }
}

/// Топики, на которые уходит событие по карточке: её служба и
/// занятие, в котором она заполнялась.
pub fn card_topics(services: &[String], session_id: Option<Uuid>) -> Vec<String> {
    let mut topics: Vec<String> = services.iter().map(|s| format!("service:{s}")).collect();
    if let Some(id) = session_id {
        topics.push(format!("session:{id}"));
    }
    topics
}

/// Группы, в которых состоит пользователь: туда уходят события
/// о начале и завершении занятия.
pub async fn group_topics(pool: &sqlx::PgPool, user_id: Uuid) -> Vec<String> {
    sqlx::query_scalar::<_, Uuid>("SELECT group_id FROM group_members WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|id| format!("group:{id}"))
        .collect()
}

/// Удобный конструктор события для карточки.
pub fn card_event(kind: &str, payload: serde_json::Value, topics: Vec<String>) -> Event {
    Event::new(kind, payload).to_all(topics)
}
