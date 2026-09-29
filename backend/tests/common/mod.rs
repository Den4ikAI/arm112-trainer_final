//! Общие помощники серверных тестов.
#![allow(dead_code)]

use sqlx::PgPool;
use uuid::Uuid;

/// Выдать карточку диспетчеру ДДС, как это делает преподаватель.
///
/// С 29.09 диспетчер ДДС видит только учебные происшествия, созданные
/// преподавателем и выданные занятием: карточки курсантов 112 к нему не
/// попадают. Тесты, которым нужна карточка у диспетчера, заводят её
/// сами — этот помощник делает её учебным происшествием и выдаёт
/// курсанту уже завершённым занятием его группы (своей группы нет —
/// заводит). Завершённым — чтобы не подменять ленту идущего занятия.
pub async fn выдать(pool: &PgPool, card: Uuid, user: Uuid) {
    sqlx::query("UPDATE cards SET origin = 'teacher' WHERE id = $1")
        .bind(card).execute(pool).await.unwrap();

    let group: Uuid = match sqlx::query_scalar::<_, Uuid>(
        "SELECT group_id FROM group_members WHERE user_id = $1 LIMIT 1",
    )
    .bind(user).fetch_optional(pool).await.unwrap()
    {
        Some(g) => g,
        None => {
            let g: Uuid = sqlx::query_scalar(
                "INSERT INTO training_groups (title, teacher_id)
                 SELECT $1, id FROM users WHERE login = 'teacher1' RETURNING id",
            )
            .bind(format!("выдача {}", Uuid::new_v4().simple()))
            .fetch_one(pool).await.unwrap();
            sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
                .bind(g).bind(user).execute(pool).await.unwrap();
            g
        }
    };

    let lesson: Uuid = sqlx::query_scalar(
        "INSERT INTO lessons (group_id, teacher_id, title, mode, started_at, finished_at)
         SELECT $1, id, 'выдача происшествия', 'dds', now(), now()
           FROM users WHERE login = 'teacher1'
         RETURNING id",
    )
    .bind(group).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO lesson_cards (lesson_id, card_id, release_at) VALUES ($1, $2, now())")
        .bind(lesson).bind(card).execute(pool).await.unwrap();
}
