//! Адаптеры моделей.
//!
//! Ни один эндпоинт не зависит от модели напрямую — только от этих
//! трейтов. Провайдер выбирается переменной `ML_PROVIDER`.
//! В MVP работает `mock`, и система функциональна без моделей.

pub mod mock;

use anyhow::Result;
use async_trait::async_trait;

/// Фрагмент расшифровки с временными метками — нужен, чтобы
/// показывать курсанту, из какой секунды разговора взято поле.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Segment {
    pub text: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub conf: f32,
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
}

#[async_trait]
pub trait Asr: Send + Sync {
    async fn transcribe(&self, audio: &[u8]) -> Result<Transcript>;
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Audio {
    pub audio_ref: String,
    pub duration_ms: u32,
}

#[async_trait]
pub trait Tts: Send + Sync {
    async fn synthesize(&self, text: &str, voice: &str) -> Result<Audio>;
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Scored {
    pub label: String,
    pub prob: f32,
}

/// Классификация по закрытому словарю: признаки опросной карты.
/// Это NLI или логит-скоринг, но НЕ генерация — модель может только
/// ранжировать предложенные варианты и физически не выдумает признак,
/// которого нет в классификаторе.
#[async_trait]
pub trait LabelScorer: Send + Sync {
    async fn score(&self, text: &str, candidates: &[String]) -> Result<Vec<Scored>>;
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct GrammarIssue {
    pub start: usize,
    pub end: usize,
    pub kind: String,
    pub message: String,
    pub suggestion: Option<String>,
}

#[async_trait]
pub trait Grammar: Send + Sync {
    async fn check(&self, text: &str) -> Result<Vec<GrammarIssue>>;
}

/// Генеративная модель. Вызывается **только офлайн**: при подготовке
/// занятия и при разборе. В момент вызова её нет — на железе из ТЗ
/// (6 ядер, без GPU) держать её в горячем пути нельзя.
#[async_trait]
pub trait Llm: Send + Sync {
    /// Заполняет текстовую обвязку сценария. Эталон НЕ генерируется —
    /// он выводится из позиции классификатора.
    async fn generate_scenario(&self, input: serde_json::Value) -> Result<serde_json::Value>;

    /// Правка по комментарию преподавателя. Требование ТЗ.
    async fn refine_scenario(&self, input: serde_json::Value) -> Result<serde_json::Value>;

    /// Сводка по типичным ошибкам группы.
    async fn group_insights(&self, stats: serde_json::Value) -> Result<String>;
}

/// Набор адаптеров, который кладётся в состояние приложения.
pub struct MlSuite {
    pub asr: Box<dyn Asr>,
    pub tts: Box<dyn Tts>,
    pub scorer: Box<dyn LabelScorer>,
    pub grammar: Box<dyn Grammar>,
    pub llm: Box<dyn Llm>,
}

impl MlSuite {
    pub fn build(provider: crate::config::MlProvider) -> Result<Self> {
        use crate::config::MlProvider as P;
        match provider {
            P::Mock => Ok(mock::suite()),
            // Появятся в задачах про ONNX и внешний сервис моделей
            P::Onnx | P::Http => {
                anyhow::bail!("провайдер {provider:?} ещё не реализован, используйте ML_PROVIDER=mock")
            }
        }
    }
}
