//! HTTP-слой. Каждый модуль здесь принадлежит отдельной задаче —
//! см. раздел «Занимает файлы» в соответствующем issue.
//!
//! Правило для параллельной работы: свой роутер регистрируем строкой
//! **в конец** списка в `routes()`, соседние не переставляем. Git
//! сливает добавления в разные места без конфликта, перестановки — нет.

pub mod catalog;
pub mod health;

use crate::state::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(catalog::routes())
    // сюда добавляются роутеры задач:
    // .merge(auth::routes())      — задача 4
    // .merge(catalog::routes())   — задача 5
    // .merge(classifier::routes())— задачи 2, 3, 6
    // .merge(sessions::routes())  — задачи 7, 8, 9
    // .merge(cards::routes())     — задачи 10, 11
    // .merge(scenarios::routes()) — задача 15
    // .merge(reports::routes())   — задача 16
    // .merge(admin::routes())     — задача 17
}
