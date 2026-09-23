//! Доклады старших бригад.
//!
//! Первый вариант из ответа заказчика (чат задачи, 21.09.2026,
//! сообщение 691): «старший группы звонит в ДДС докладывает об
//! обстановке и ходе работ». Этап наступает по шкале, а старший
//! звонит спустя `brigade.report_delay_sec`. Диспетчер, который
//! позвонил ему раньше, об этапе уже знает — такой доклад не повторяется.
//!
//! Доставка — условный UPDATE по `delivered_at IS NULL`, как у сторожа
//! нормативов: при нескольких экземплярах сервера один доклад не придёт
//! дважды.

use crate::{
    reports::forecast::setting,
    ws::{self, Hub},
};
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// Период опроса. Этапы учебной шкалы — десятки секунд, опаздывать
/// с докладом больше чем на пару секунд незачем.
const TICK: Duration = Duration::from_secs(2);

pub async fn run(pool: PgPool, hub: Hub) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(e) = deliver(&pool, &hub).await {
            tracing::error!(error = %e, "доклады бригад не разосланы");
        }
    }
}

/// Один проход. Вынесен отдельно, чтобы его можно было вызвать из
/// проверки, не поднимая фоновую задачу.
pub async fn deliver(pool: &PgPool, hub: &Hub) -> anyhow::Result<usize> {
    let delay = setting(pool, "brigade.report_delay_sec", 25.0).await;

    // После отказа от выполнения работ бригада не работает, и
    // докладывать ей не о чем
    let delivered: Vec<(Uuid, String, i32, String, String)> = sqlx::query_as(
        "UPDATE brigade_reports r
            SET delivered_at = now(), channel = 'incoming'
           FROM brigade_assignments a
          WHERE a.id = r.assignment_id
            AND r.delivered_at IS NULL
            AND r.due_at + make_interval(secs => $1) <= now()
            AND NOT EXISTS (
                  SELECT 1 FROM response_entries e
                   WHERE e.card_id = a.card_id AND e.service_id = a.service_id
                     AND e.status = 'Отказ от выполнения работ')
      RETURNING a.card_id, a.service_id, a.brigade_no, r.stage::text, r.text",
    )
    .bind(delay)
    .fetch_all(pool)
    .await?;

    for (card_id, service_id, brigade_no, stage, text) in &delivered {
        let (session_id, services): (Option<Uuid>, Vec<String>) = sqlx::query_as(
            "SELECT c.session_id, array(SELECT service_id FROM card_services WHERE card_id = c.id)
               FROM cards c WHERE c.id = $1",
        )
        .bind(card_id)
        .fetch_one(pool)
        .await?;

        // Входящий звонок: карточка ДДС показывает его сразу, без
        // обновления страницы
        hub.publish(ws::routes::card_event(
            "brigade.report",
            json!({
                "cardId": card_id,
                "serviceId": service_id,
                "brigadeNo": brigade_no,
                "stage": stage,
                "text": text,
            }),
            ws::routes::card_topics(&services, session_id),
        ));
    }

    Ok(delivered.len())
}
