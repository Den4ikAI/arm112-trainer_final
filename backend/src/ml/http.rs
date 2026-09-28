//! Адаптер к внешнему серверу моделей.
//!
//! Говорит на диалекте OpenAI `/v1/chat/completions` — его понимают
//! llama.cpp, vLLM, Ollama и облачные сервисы. Благодаря этому выбор
//! движка становится вопросом конфигурации, а не переписывания кода:
//! на стенде заказчика это будет llama-server с моделью на процессоре,
//! у нас при подготовке сценариев — та же модель на видеокарте.
//!
//! Генеративная модель вызывается офлайн: подготовка сценариев,
//! коррекция по комментарию преподавателя, разборы в фоне. В горячем
//! пути заполнения карточки — только одно: понять свободный вопрос
//! курсанта заявителю (`HttpLabelScorer`). Там ответ — одно значение
//! из перечисления, несколько токенов, и это укладывается в секунды
//! даже на процессоре.

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
        self.chat_with(messages, max_tokens, temperature, None).await
    }

    async fn chat_with(
        &self,
        messages: Value,
        max_tokens: u32,
        temperature: f32,
        schema: Option<Value>,
    ) -> Result<String> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": max_tokens,
            "temperature": temperature,
        });

        // Ограничение вывода схемой: движок отбрасывает токены,
        // нарушающие её, поэтому разбирать заведомо валидный ответ
        // проще, чем чинить невалидный
        if let (Some(schema), Some(obj)) = (schema, body.as_object_mut()) {
            obj.insert(
                "response_format".into(),
                json!({
                    "type": "json_schema",
                    "json_schema": { "name": "answer", "strict": true, "schema": schema },
                }),
            );
        }

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
    pub async fn chat_json(
        &self,
        messages: Value,
        max_tokens: u32,
        schema: Option<Value>,
    ) -> Result<Value> {
        let raw = self.chat_with(messages, max_tokens, 0.2, schema).await?;
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
        self.chat_json(input, 900, None).await
    }

    async fn refine_scenario(&self, input: Value) -> Result<Value> {
        self.chat_json(input, 900, None).await
    }

    async fn chat_json(
        &self,
        messages: Value,
        max_tokens: u32,
        schema: Option<Value>,
    ) -> Result<Value> {
        HttpLlm::chat_json(self, messages, max_tokens, schema).await
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

/// Сопоставление свободного вопроса оператора с фактами вызова.
///
/// В отличие от остального здесь модель в горячем пути: курсант ждёт
/// ответа заявителя. Поэтому ответ — одно значение из перечисления
/// (движок не даст выйти за схему) и несколько токенов. Замер на наших
/// вызовах: GigaChat на процессоре — 27 из 27 верно, 1,8 с
/// (`backend/docs/замеры/README.md`).
pub struct HttpLabelScorer {
    llm: HttpLlm,
}

/// Значение «ни про какой вариант».
const NONE: &str = "нет_подходящего";

/// Дольше этого курсант ждать не должен: лучше переспросить, чем
/// молчать полминуты. Отказ модели обрабатывает вызывающий.
const SCORE_TIMEOUT: Duration = Duration::from_secs(20);

impl HttpLabelScorer {
    pub fn new(base_url: String, model: String) -> Result<Self> {
        let mut llm = HttpLlm::new(base_url, model)?;
        llm.client = reqwest::Client::builder().timeout(SCORE_TIMEOUT).build()?;
        Ok(Self { llm })
    }
}

#[async_trait]
impl super::LabelScorer for HttpLabelScorer {
    async fn score(&self, text: &str, candidates: &[String]) -> Result<Vec<super::Scored>> {
        if candidates.is_empty() || text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let (keys, texts) = split_keys(candidates);
        let listing = keys
            .iter()
            .zip(&texts)
            .map(|(k, c)| format!("  {k} — {c}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut allowed = keys.clone();
        allowed.push(NONE.into());

        let messages = json!([
            { "role": "system", "content": format!(
                "Оператор службы 112 разговаривает с заявителем. Определи, \
                 про какой из перечисленных фактов оператор сейчас спрашивает.\n\
                 Факты:\n{listing}\n  {NONE} — оператор спрашивает про что-то \
                 другое, заявителю про это сказать нечего.") },
            { "role": "user", "content": format!("Вопрос оператора: {text}") },
        ]);
        let schema = json!({
            "type": "object",
            "properties": { "fact": { "type": "string", "enum": allowed } },
            "required": ["fact"],
            "additionalProperties": false,
        });

        let answer = self.llm.chat_json(messages, 24, Some(schema)).await?;
        let chosen = answer["fact"].as_str().unwrap_or(NONE);
        Ok(keys
            .iter()
            .zip(candidates)
            .map(|(k, c)| super::Scored {
                label: c.clone(),
                prob: if k == chosen { 1.0 } else { 0.0 },
            })
            .collect())
    }
}

/// Ключи и тексты вариантов. Ключ со смыслом («city», «victims») берём,
/// если он есть у каждого варианта и не повторяется: с ним GigaChat
/// отличает «ни про какой» в 5 случаях из 7, с номерами f1…fN — в 1.
/// Иначе номера: одинаковые по форме, модель выбирает номер, а не
/// переписывает вопрос.
fn split_keys(candidates: &[String]) -> (Vec<String>, Vec<String>) {
    let parts: Vec<(Option<&str>, &str)> = candidates
        .iter()
        .map(|c| match c.split_once('\t') {
            Some((k, t)) => (Some(k.trim()), t.trim()),
            None => (None, c.trim()),
        })
        .collect();
    let keys: Vec<&str> = parts.iter().filter_map(|(k, _)| *k).collect();
    let usable = keys.len() == parts.len()
        && keys.iter().all(|k| {
            !k.is_empty() && *k != NONE && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        && keys.iter().collect::<std::collections::HashSet<_>>().len() == keys.len();
    let texts = parts.iter().map(|(_, t)| t.to_string()).collect();
    let keys = if usable {
        keys.iter().map(|k| k.to_string()).collect()
    } else {
        (1..=parts.len()).map(|i| format!("f{i}")).collect()
    };
    (keys, texts)
}

/// Распознавание речи и синтез подключает `ml/voice.rs`, если задан
/// сервер голоса; проверка грамматики регулярками хоть и проста, но
/// ничего не выдумывает.
pub fn suite(base_url: String, model: String) -> Result<MlSuite> {
    let mock = super::mock::suite();
    Ok(MlSuite {
        asr: mock.asr,
        tts: mock.tts,
        scorer: Box::new(HttpLabelScorer::new(base_url.clone(), model.clone())?),
        grammar: mock.grammar,
        llm: Box::new(HttpLlm::new(base_url, model)?),
    })
}

#[cfg(test)]
mod tests {
    use super::{extract_json, split_keys};
    use serde_json::json;

    #[test]
    fn ключи_сценария_идут_в_перечисление() {
        let c = vec!["city\tКакой город?".to_string(), "victims\tПострадавшие есть?".to_string()];
        let (keys, texts) = split_keys(&c);
        assert_eq!(keys, vec!["city", "victims"]);
        assert_eq!(texts, vec!["Какой город?", "Пострадавшие есть?"]);
    }

    #[test]
    fn без_ключей_или_с_повторами_номера() {
        let (keys, _) = split_keys(&["Какой город?".to_string(), "Пострадавшие?".to_string()]);
        assert_eq!(keys, vec!["f1", "f2"]);
        let (keys, _) = split_keys(&["a\tраз".to_string(), "a\tдва".to_string()]);
        assert_eq!(keys, vec!["f1", "f2"], "повтор ключа сделал бы выбор неоднозначным");
        let (keys, _) = split_keys(&["город\tКакой?".to_string()]);
        assert_eq!(keys, vec!["f1"], "ключ не латиницей в перечисление не берём");
    }

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
