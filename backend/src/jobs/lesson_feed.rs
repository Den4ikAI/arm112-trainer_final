//! Поток карточек занятия ДДС.
//!
//! Карточки занятия приходят не разом, а по одной с интервалом
//! (`lesson_cards.release_at`, ответ заказчика в чате задачи 23.09.2026,
//! сообщение 709). Лента сама фильтрует невыпущенные, но курсант должен
//! увидеть новую карточку, не обновляя страницу. Эта задача один раз
//! рассылает событие `card.dispatched` — то же, что при сохранении
//! карточки оператором 112, — когда наступает выпуск.
//!
//! Рассылка — условный UPDATE по `announced = false`, как у докладов
//! бригад: при нескольких экземплярах сервера событие уйдёт один раз.

use crate::ws::{self, Hub};
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// Период опроса. Норматив открытия — 30 секунд, опаздывать
/// с появлением карточки больше чем на пару секунд нельзя.
const TICK: Duration = Duration::from_secs(2);

pub async fn run(pool: PgPool, hub: Hub) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(e) = announce(&pool, &hub).await {
            tracing::error!(error = %e, "поток карточек занятия не разослан");
        }
    }
}

/// Один проход. Вынесен отдельно, чтобы его можно было вызвать из
/// проверки, не поднимая фоновую задачу.
pub async fn announce(pool: &PgPool, hub: &Hub) -> anyhow::Result<usize> {
    // Завершённое занятие ленту не ограничивает, объявлять в нём нечего
    let released: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE lesson_cards lc
            SET announced = true
           FROM lessons l
          WHERE l.id = lc.lesson_id
            AND NOT lc.announced
            AND lc.release_at <= now()
            AND l.finished_at IS NULL
      RETURNING lc.lesson_id, lc.card_id",
    )
    .fetch_all(pool)
    .await?;

    for (lesson_id, card_id) in &released {
        let card: Option<(Option<Uuid>, String, Option<String>, Vec<String>)> = sqlx::query_as(
            "SELECT c.session_id, c.number, c.outcome_type,
                    array(SELECT service_id FROM card_services WHERE card_id = c.id)
               FROM cards c WHERE c.id = $1",
        )
        .bind(card_id)
        .fetch_optional(pool)
        .await?;
        let Some((session_id, number, outcome_type, services)) = card else {
            continue;
        };

        hub.publish(ws::routes::card_event(
            "card.dispatched",
            json!({
                "cardId": card_id,
                "number": number,
                "outcomeType": outcome_type,
                "services": services,
                "lessonId": lesson_id,
            }),
            ws::routes::card_topics(&services, session_id),
        ));
    }

    Ok(released.len())
}
