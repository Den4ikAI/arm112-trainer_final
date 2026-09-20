//! Разделяемое состояние приложения.

use crate::{config::Config, ml::MlSuite};
use sqlx::PgPool;
use std::{sync::Arc, time::Instant};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub ml: Arc<MlSuite>,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: PgPool, config: Config, ml: MlSuite) -> Self {
        Self {
            db,
            config: Arc::new(config),
            ml: Arc::new(ml),
            started_at: Instant::now(),
        }
    }
}
