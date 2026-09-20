//! Адаптер к внешнему серверу моделей.
//!
//! Говорит на диалекте OpenAI `/v1/chat/completions` — его понимают
//! llama.cpp, vLLM, Ollama и облачные сервисы. Благодаря этому выбор
//! движка становится вопросом конфигурации, а не переписывания кода:
//! на стенде заказчика это будет llama-server с моделью на процессоре,
//! у нас при подготовке сценариев — та же модель на видеокарте.
//!
//! Генеративная модель вызывается **только офлайн**: подготовка
//! сценариев и коррекция по комментарию преподавателя. В горячем пути
//! заполнения карточки её нет и быть не может — на железе из ТЗ
//! (6 ядер, без GPU) она туда не помещается.

use super::{Llm, MlSuite};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

/// Сколько ждём ответа. На процессоре генерация идёт 10-15 токенов
/// в секунду, и ответ на пару сотен токенов — это полминуты.
/// Пять минут с запасом: лучше дождаться, чем оборвать задачу,
/// которая почти закончилась.
const TIMEOUT: Duration = Duration::from_secs(300);

pub struct HttpLlm {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

impl HttpLlm {
    pub fn new(base_url: String, model: String) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder().timeout(TIMEOUT).build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
        })
    }

    /// Один обмен репликами. История передаётся целиком: сервер
    /// держит кэш и повторно разбирает только новое.
    pub async fn chat(&self, messages: Value, max_tokens: u32, temperature: f32) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        });

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("сервер моделей {} не ответил", self.base_url))?;

        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("сервер моделей вернул {code}: {}", text.chars().take(300).collect::<String>());
        }

        let parsed: ChatResponse = resp.json().await.context("неожиданный ответ сервера моделей")?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .context("сервер моделей вернул пустой ответ")
    }

    /// Ответ, из которого нужен JSON.
    ///
    /// Модель почти всегда оборачивает его в ```json … ``` или
    /// предваряет фразой, сколько ни проси не делать этого. Вырезаем
    /// сами, а не полагаемся на послушание.
    pub async fn chat_json(&self, messages: Value, max_tokens: u32) -> Result<Value> {
        let raw = self.chat(messages, max_tokens, 0.2).await?;
        extract_json(&raw).with_context(|| {
            format!("в ответе модели нет JSON: {}", raw.chars().take(200).collect::<String>())
        })
    }
}

/// Вытащить JSON из текста: сначала пробуем целиком, потом ищем
/// первый сбалансированный объект.
pub fn extract_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }

    // Снимаем ограждение markdown, если оно есть
    let body = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        return Some(v);
    }

    // Ищем первый объект со сбалансированными скобками, не считая
    // те, что внутри строк
    let chars: Vec<char> = body.chars().collect();
    let start = chars.iter().position(|c| *c == '{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (i, c) in chars.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let slice: String = chars[start..=i].iter().collect();
                    return serde_json::from_str(&slice).ok();
                }
            }
            _ => {}
        }
    }
    None
}

#[async_trait]
impl Llm for HttpLlm {
    async fn generate_scenario(&self, input: Value) -> Result<Value> {
        self.chat_json(input, 900).await
    }

    async fn refine_scenario(&self, input: Value) -> Result<Value> {
        self.chat_json(input, 900).await
    }

    async fn chat_json(&self, messages: Value, max_tokens: u32) -> Result<Value> {
        HttpLlm::chat_json(self, messages, max_tokens).await
    }

    async fn group_insights(&self, stats: Value) -> Result<String> {
        // Числа уже посчитаны базой; модели остаётся их пересказать.
        // Выдумать процент она при таком запросе не может — он ей
        // просто не нужен
        let messages = json!([
            { "role": "system", "content":
              "Ты помогаешь преподавателю учебного центра. По готовой статистике \
               напиши короткую сводку: три-четыре предложения, без вступлений \
               и без разметки.\n\n\
               Числа бери только из данных и только вместе с их подписью. \
               Ничего не складывай, не дели и не пересчитывай: подписи уже \
               говорят, что означает каждое число." },
            { "role": "user", "content":
              format!("Статистика по группе:\n{}\n\nНапиши сводку.",
                      serde_json::to_string_pretty(&stats)?) },
        ]);
        self.chat(messages, 400, 0.3).await
    }
}

/// Остальные адаптеры пока берём из заглушек: распознавание речи и
/// синтез — отдельная задача, а проверка грамматики регулярками
/// хоть и проста, но ничего не выдумывает.
pub fn suite(base_url: String, model: String) -> Result<MlSuite> {
    let mock = super::mock::suite();
    Ok(MlSuite {
        asr: mock.asr,
        tts: mock.tts,
        scorer: mock.scorer,
        grammar: mock.grammar,
        llm: Box::new(HttpLlm::new(base_url, model)?),
    })
}

#[cfg(test)]
mod tests {
    use super::extract_json;
    use serde_json::json;

    /// Модель почти всегда оборачивает JSON в ограждение markdown,
    /// сколько ни проси не делать этого.
    #[test]
    fn json_достаётся_из_ограждения() {
        let raw = "```json\n{\"legend\": \"Горит крыша!\"}\n```";
        assert_eq!(extract_json(raw).unwrap(), json!({ "legend": "Горит крыша!" }));
    }

    #[test]
    fn json_достаётся_из_болтовни_вокруг() {
        let raw = "Конечно! Вот сценарий:\n{\"legend\": \"Дым в подъезде\"}\nГотово.";
        assert_eq!(extract_json(raw).unwrap()["legend"], "Дым в подъезде");
    }

    /// Фигурная скобка внутри реплики не должна обрывать разбор.
    #[test]
    fn скобки_внутри_строк_не_ломают_разбор() {
        let raw = r#"{"legend": "Он кричал: {помогите}", "n": 1}"#;
        let v = extract_json(raw).unwrap();
        assert_eq!(v["legend"], "Он кричал: {помогите}");
        assert_eq!(v["n"], 1);
    }

    #[test]
    fn вложенные_объекты_не_обрываются_на_первой_скобке() {
        let raw = r#"текст {"a": {"b": 2}, "c": 3} хвост"#;
        let v = extract_json(raw).unwrap();
        assert_eq!(v["a"]["b"], 2);
        assert_eq!(v["c"], 3);
    }

    #[test]
    fn без_json_возвращается_ничего() {
        assert!(extract_json("Извините, не могу помочь.").is_none());
    }
}
