//! Разделяемое состояние приложения.

use crate::{config::Config, ml::MlSuite, ws::Hub};
use sqlx::PgPool;
use std::{sync::Arc, time::Instant};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub ml: Arc<MlSuite>,
    /// Шина событий класса. Пустая шина не мешает: занятие идёт
    /// и без открытого журнала у преподавателя.
    pub ws: Hub,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: PgPool, config: Config, ml: MlSuite) -> Self {
        Self {
            db,
            config: Arc::new(config),
            ml: Arc::new(ml),
            ws: Hub::new(),
            started_at: Instant::now(),
        }
    }
}
