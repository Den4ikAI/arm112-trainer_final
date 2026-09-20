//! Конфигурация из переменных окружения. Образец — `.env.example`.

use anyhow::{Context, Result};

/// Какой набор адаптеров моделей поднимать.
///
/// В MVP по умолчанию `Mock`: система полностью работоспособна
/// без единой модели, это сознательное требование архитектуры.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlProvider {
    Mock,
    Onnx,
    Http,
}

impl MlProvider {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "mock" => Ok(Self::Mock),
            "onnx" => Ok(Self::Onnx),
            "http" => Ok(Self::Http),
            other => anyhow::bail!("неизвестный ML_PROVIDER: {other} (ожидается mock|onnx|http)"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub jwt_secret: String,
    pub jwt_ttl_hours: i64,
    pub ml_provider: MlProvider,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        // .env необязателен: в проде переменные приходят из окружения
        let _ = dotenvy::dotenv();

        let database_url = std::env::var("DATABASE_URL")
            .context("не задан DATABASE_URL — скопируйте .env.example в .env")?;

        Ok(Self {
            database_url,
            bind_addr: env_or("BIND_ADDR", "127.0.0.1:8080"),
            jwt_secret: env_or("JWT_SECRET", "dev-secret-change-me"),
            jwt_ttl_hours: env_or("JWT_TTL_HOURS", "8").parse().unwrap_or(8),
            ml_provider: MlProvider::parse(&env_or("ML_PROVIDER", "mock"))?,
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
