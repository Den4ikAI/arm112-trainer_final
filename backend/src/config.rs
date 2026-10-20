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
    /// Куда класть резервные копии. Не задан — копии не делаются:
    /// на машине разработчика они не нужны, а молча писать гигабайты
    /// в чужой каталог нельзя.
    pub backup_dir: Option<std::path::PathBuf>,
    /// Сколько суток хранить копии. ТЗ: 30.
    pub backup_keep_days: i64,
    /// Пароль учебных учётных записей демонстрационного стенда. Задан —
    /// экран входа показывает их списком (просьба экспертов 18.10: «не
    /// искать логины и пароли в документации»). В рабочей установке не
    /// задаётся, и список пуст.
    pub demo_password: Option<String>,
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
            backup_dir: std::env::var("BACKUP_DIR").ok().map(Into::into),
            backup_keep_days: env_or("BACKUP_KEEP_DAYS", "30").parse().unwrap_or(30),
            demo_password: std::env::var("DEMO_PASSWORD").ok().filter(|p| !p.is_empty()),
        })
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
