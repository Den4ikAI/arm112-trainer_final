//! Заглушки моделей.
//!
//! Детерминированные: одинаковый вход даёт одинаковый выход, иначе
//! тесты будут плавать. Ни одна заглушка не выдумывает данные там,
//! где их можно взять из настоящих — см. `group_insights`.

use super::*;

pub fn suite() -> MlSuite {
    MlSuite {
        asr: Box::new(MockAsr),
        tts: Box::new(MockTts),
        scorer: Box::new(MockLabelScorer),
        grammar: Box::new(MockGrammar),
        llm: Box::new(MockLlm),
    }
}

pub struct MockAsr;

#[async_trait]
impl Asr for MockAsr {
    async fn transcribe(&self, audio: &[u8]) -> Result<Transcript> {
        // Настоящая модель появится в задаче про ONNX. Пока отдаём
        // предсказуемую заглушку, по длине входа — чтобы отличать вызовы.
        let text = format!("[расшифровка заглушки, {} байт аудио]", audio.len());
        Ok(Transcript {
            segments: vec![Segment {
                text: text.clone(),
                start_ms: 0,
                end_ms: 1000,
                conf: 0.99,
            }],
            text,
        })
    }
}

pub struct MockTts;

#[async_trait]
impl Tts for MockTts {
    async fn synthesize(&self, text: &str, voice: &str) -> Result<Audio> {
        // Примерно 60 мс на слово — чтобы длительность выглядела правдоподобно
        let words = text.split_whitespace().count() as u32;
        Ok(Audio {
            audio_ref: format!("mock://tts/{voice}/{}", stable_hash(text)),
            duration_ms: words.max(1) * 60,
        })
    }
}

pub struct MockLabelScorer;

#[async_trait]
impl LabelScorer for MockLabelScorer {
    async fn score(&self, text: &str, candidates: &[String]) -> Result<Vec<Scored>> {
        let haystack = text.to_lowercase();
        let mut out: Vec<Scored> = candidates
            .iter()
            .map(|label| {
                let needle = label.to_lowercase();
                // Прямое вхождение считаем уверенным попаданием,
                // остальным даём стабильный низкий балл
                let prob = if haystack.contains(&needle) {
                    0.9
                } else {
                    0.1 + (stable_hash(label) % 10) as f32 / 100.0
                };
                Scored { label: label.clone(), prob }
            })
            .collect();
        out.sort_by(|a, b| b.prob.total_cmp(&a.prob));
        Ok(out)
    }
}

pub struct MockGrammar;

#[async_trait]
impl Grammar for MockGrammar {
    async fn check(&self, text: &str) -> Result<Vec<GrammarIssue>> {
        let mut issues = Vec::new();

        if let Some(pos) = text.find("  ") {
            issues.push(GrammarIssue {
                start: pos,
                end: pos + 2,
                kind: "пробелы".into(),
                message: "Двойной пробел".into(),
                suggestion: Some(" ".into()),
            });
        }

        if let Some(pos) = text.find(" ,") {
            issues.push(GrammarIssue {
                start: pos,
                end: pos + 2,
                kind: "пунктуация".into(),
                message: "Пробел перед запятой".into(),
                suggestion: Some(",".into()),
            });
        }

        if let Some(first) = text.chars().next() {
            if first.is_lowercase() {
                issues.push(GrammarIssue {
                    start: 0,
                    end: first.len_utf8(),
                    kind: "регистр".into(),
                    message: "Предложение начинается со строчной буквы".into(),
                    suggestion: Some(first.to_uppercase().to_string()),
                });
            }
        }

        Ok(issues)
    }
}

pub struct MockLlm;

#[async_trait]
impl Llm for MockLlm {
    async fn generate_scenario(&self, input: serde_json::Value) -> Result<serde_json::Value> {
        // Настоящая генерация появится в задаче про сценарии.
        // Заглушка возвращает структуру нужной формы, чтобы фронтенд
        // и тесты работали уже сейчас.
        Ok(serde_json::json!({
            "legend": "Алло! У нас тут происшествие, приезжайте скорее!",
            "applicantName": "Иванов Иван Иванович",
            "applicantPhone": "916-000-00-00",
            "applicantStatus": "очевидец",
            "hidden": [
                { "key": "victims", "question": "Пострадавшие есть?",
                  "answer": "Нет, пострадавших нет.", "unlocks": ["victims"] }
            ],
            "_mock": true,
            "_input": input
        }))
    }

    async fn refine_scenario(&self, input: serde_json::Value) -> Result<serde_json::Value> {
        // Заглушка не меняет сценарий, но помечает, что правка получена
        let mut out = input.clone();
        if let Some(obj) = out.as_object_mut() {
            obj.insert("_refined".into(), serde_json::Value::Bool(true));
        }
        Ok(out)
    }

    async fn group_insights(&self, stats: serde_json::Value) -> Result<String> {
        // Принципиально: заглушка НЕ выдумывает цифры, а пересказывает
        // настоящую агрегацию. Демо выглядит честно, а при замене
        // на реальную модель контракт не меняется.
        let total = stats.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        let mut lines = vec![format!("Занятий в выборке: {total}.")];

        if let Some(items) = stats.get("topFindings").and_then(|v| v.as_array()) {
            for item in items.iter().take(3) {
                let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("поле");
                let count = item.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                lines.push(format!("{count} из {total} ошиблись в «{label}»."));
            }
        }

        Ok(lines.join(" "))
    }
}

/// Стабильный хеш строки: нужен, чтобы заглушки давали одинаковый
/// результат между запусками.
fn stable_hash(s: &str) -> u32 {
    s.bytes().fold(2166136261u32, |h, b| {
        (h ^ b as u32).wrapping_mul(16777619)
    })
}
