//! HTTP-слой. Каждый модуль здесь принадлежит отдельной задаче —
//! см. раздел «Занимает файлы» в соответствующем issue.
//!
//! Правило для параллельной работы: свой роутер регистрируем строкой
//! **в конец** списка в `routes()`, соседние не переставляем. Git
//! сливает добавления в разные места без конфликта, перестановки — нет.

pub mod admin;
pub mod auth;
pub mod catalog;
pub mod cards;
pub mod reports;
pub mod scenarios;
pub mod sessions;
pub mod health;
pub mod openapi;

use crate::state::AppState;
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .merge(catalog::routes())
        .merge(auth::routes())
        .merge(sessions::routes())
        .merge(cards::routes())
        .merge(reports::routes())
        .merge(admin::routes())
        .merge(scenarios::routes())
    // сюда добавляются роутеры задач:
    // .merge(catalog::routes())   — задача 5
    // .merge(classifier::routes())— задачи 2, 3, 6
    // .merge(sessions::routes())  — задачи 7, 8, 9
}
