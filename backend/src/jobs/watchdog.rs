//! Сторож нормативов.
//!
//! Каждые пять секунд пересчитывает статус сохранённых карточек по
//! статусам реагирования служб. Норматив подтверждения — 30 секунд,
//! порог «не завершено» — 48 часов (памятка ДДС).
//!
//! Пересчёт, а не набор переходов: задача идемпотентна, переживает
//! перезапуск сервера и сама исправляет карточку, если диспетчер
//! передумал. Пишем только когда статус действительно изменился —
//! иначе аудит завалило бы одинаковыми записями раз в пять секунд.
//!
//! Запись идёт условным UPDATE по старому статусу. Экземпляров
//! сервера может быть несколько, такты у них совпадут, и без этого
//! условия один переход попал бы в аудит столько раз, сколько
//! экземпляров успело его заметить.

use crate::{
    audit,
    domain::card_status::{derive, is_manual, ServiceOutcome},
};
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

/// Период опроса. Норматив 30 секунд, шаг 5 — карточка краснеет
/// с точностью до пяти секунд.
const TICK: Duration = Duration::from_secs(5);

pub async fn run(pool: PgPool) {
    let mut ticker = tokio::time::interval(TICK);
    // Если тик пропущен (сервер был занят), догонять пачкой незачем
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;
        if let Err(e) = sweep(&pool).await {
            tracing::error!(error = %e, "сторож нормативов не отработал такт");
        }
    }
}

/// Один проход. Вынесен отдельно, чтобы его можно было вызвать
/// из теста, не поднимая фоновую задачу.
pub async fn sweep(pool: &PgPool) -> anyhow::Result<usize> {
    // Незавершённые карточки: завершённые и проверенные пересчитывать
    // незачем, а их со временем станет большинство
    let cards: Vec<(Uuid, String, i64)> = sqlx::query_as(
        "SELECT id, status::text, EXTRACT(EPOCH FROM (now() - saved_at))::bigint
           FROM cards
          WHERE saved_at IS NOT NULL
            AND status::text NOT IN ('Завершена', 'Проверена')",
    )
    .fetch_all(pool)
    .await?;

    let mut changed = 0;

    for (card_id, status, elapsed_sec) in cards {
        if is_manual(&status) {
            continue;
        }

        let chains: Vec<(String, Vec<String>)> = sqlx::query_as(
            "SELECT cs.service_id,
                    COALESCE(
                      array_agg(re.status::text ORDER BY re.at, re.id)
                        FILTER (WHERE re.id IS NOT NULL),
                      '{}'
                    ) AS chain
               FROM card_services cs
               LEFT JOIN response_entries re
                      ON re.card_id = cs.card_id AND re.service_id = cs.service_id
              WHERE cs.card_id = $1
              GROUP BY cs.service_id",
        )
        .bind(card_id)
        .fetch_all(pool)
        .await?;

        let outcomes: Vec<ServiceOutcome> = chains
            .iter()
            .map(|(_, chain)| ServiceOutcome::from_chain(chain))
            .collect();

        let next = derive(&outcomes, elapsed_sec);
        if next == status {
            continue;
        }

        // Условие по старому статусу — это и есть блокировка: если
        // такт уже отработал другой экземпляр сервера, строк не будет
        // и мы не продублируем запись в аудите
        let updated = sqlx::query(
            "UPDATE cards SET status = $2::card_status
              WHERE id = $1 AND status::text = $3",
        )
        .bind(card_id)
        .bind(next)
        .bind(&status)
        .execute(pool)
        .await?
        .rows_affected();

        if updated == 0 {
            continue;
        }

        // Требование раздела «Аудит»: каждый автоматический переход
        // оставляет след, иначе потом не объяснить, почему карточка
        // оказалась в «Не оповещено»
        audit::log(
            pool,
            audit::Event::new("card.status.auto")
                .about("card", card_id)
                .with(json!({
                    "from": status,
                    "to": next,
                    "elapsedSec": elapsed_sec,
                    "services": chains.iter().map(|(id, chain)| json!({
                        "serviceId": id,
                        "last": ServiceOutcome::from_chain(chain).last,
                    })).collect::<Vec<_>>(),
                })),
        )
        .await;

        tracing::info!(%card_id, from = %status, to = next, elapsed_sec, "статус карточки изменён сторожем");
        changed += 1;
    }

    Ok(changed)
}
