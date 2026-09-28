//! Адаптеры моделей.
//!
//! Ни один эндпоинт не зависит от модели напрямую — только от этих
//! трейтов. Провайдер выбирается переменной `ML_PROVIDER`.
//! В MVP работает `mock`, и система функциональна без моделей.

pub mod http;
pub mod mock;
pub mod voice;

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

    /// Подключено настоящее распознавание. У заглушки — нет: её
    /// «расшифровку» нельзя выдавать за слова курсанта
    fn available(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Audio {
    pub audio_ref: String,
    pub duration_ms: u32,
}

#[async_trait]
pub trait Tts: Send + Sync {
    async fn synthesize(&self, text: &str, voice: &str) -> Result<Audio>;

    /// Сам звук, WAV. У заглушки звука нет
    async fn wav(&self, _text: &str, _voice: &str) -> Result<Vec<u8>> {
        anyhow::bail!("синтез речи не подключён")
    }

    /// Подключён настоящий синтез
    fn available(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct Scored {
    pub label: String,
    pub prob: f32,
}

/// Классификация по закрытому словарю: признаки опросной карты, факт
/// вызова, о котором спросили. Это NLI или логит-скоринг, но НЕ
/// генерация — модель может только ранжировать предложенные варианты и
/// физически не выдумает вариант, которого нет в списке.
///
/// Вариант может нести короткий ключ через табуляцию: `city\tКакой
/// город?`. Модели ключ со смыслом помогает — по нему она увереннее
/// отличает «ни про какой вариант» (замер —
/// `backend/docs/замеры/СВОБОДНЫЙ-ВОПРОС.md`). В `Scored::label`
/// вариант возвращается целиком, с ключом.
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

    /// Свободный диалог: правка сценария по замечанию преподавателя.
    ///
    /// На вход — массив сообщений в формате OpenAI, на выход — JSON.
    /// В трейте, а не в конкретном адаптере, чтобы эндпоинт не зависел
    /// от того, поднята модель или нет: на заглушке диалог тоже
    /// работает, просто правок не предлагает.
    ///
    /// `schema` — схема JSON, которой обязан соответствовать ответ.
    /// Движок ограничивает выбор токенов так, что нарушить её
    /// невозможно: модель на десять миллиардов параметров иначе
    /// регулярно кладёт значения не в те поля.
    async fn chat_json(
        &self,
        messages: serde_json::Value,
        max_tokens: u32,
        schema: Option<serde_json::Value>,
    ) -> Result<serde_json::Value>;
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
        let mut suite = Self::build_models(provider)?;
        // Голос — отдельный сервер и от языковой модели не зависит:
        // он нужен и на заглушке модели, и при модели во внешнем контуре
        voice::attach(&mut suite);
        Ok(suite)
    }

    fn build_models(provider: crate::config::MlProvider) -> Result<Self> {
        use crate::config::MlProvider as P;
        match provider {
            P::Mock => Ok(mock::suite()),
            // Внешний сервер моделей: llama.cpp, vLLM, Ollama —
            // все говорят на диалекте OpenAI
            P::Http => http::suite(
                std::env::var("ML_BASE_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:8090".into()),
                std::env::var("ML_MODEL").unwrap_or_else(|_| "local".into()),
            ),
            // Появится в задаче про ONNX на процессоре
            P::Onnx => {
                anyhow::bail!("провайдер onnx ещё не реализован, используйте mock или http")
            }
        }
    }
}
