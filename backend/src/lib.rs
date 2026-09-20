//! Тренажёр оператора ДДС города Москвы — серверная часть.
//!
//! Архитектура и контракт API описаны в BACKEND.md. Слои:
//!   api/     HTTP-хендлеры, валидация, RBAC
//!   ws/      канал класса, события занятия
//!   domain/  классификатор, машина статусов, оценка
//!   ingest/  импорт классификатора из XLSX
//!   jobs/    фоновые задачи
//!   ml/      адаптеры моделей (в MVP — заглушки)
//!   store/   PostgreSQL, миграции
//!   audit/   журнал действий

pub mod api;
pub mod audit;
pub mod domain;
pub mod jobs;
pub mod ws;
pub mod config;
pub mod error;
pub mod ingest;
pub mod ml;
pub mod state;
pub mod store;
