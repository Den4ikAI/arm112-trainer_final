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

/// Слова, которые есть почти в любом вопросе и ничего не различают.
const STOP: [&str; 34] = [
    "есть", "это", "как", "какой", "какая", "какое", "какие", "каком", "что",
    "или", "там", "где", "вас", "ваш", "вам", "мне", "пожалуйста", "уточните",
    "подскажите", "скажите", "только", "сейчас", "видно", "видите", "был",
    "была", "были", "уже", "ещё", "еще", "кто", "нибудь", "можете", "могу",
];

/// Основы слов: без регистра и «ё», короче пяти букв не режем,
/// длиннее — первые пять. Грубо, но без словаря и без модели.
fn stems(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(w))
        .map(|w| w.chars().take(5).collect())
        .collect()
}

/// Основы совпадают, если одна — начало другой: «газ» и «газиф».
fn same_stem(a: &str, b: &str) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

#[async_trait]
impl LabelScorer for MockLabelScorer {
    /// Заглушка без модели: доля слов текста, нашедшихся в варианте.
    /// «Люди пострадали?» находит «Пострадавшие есть?», а «Как вас
    /// зовут?» не находит ничего — заявитель переспросит, а не ответит
    /// не на тот вопрос.
    async fn score(&self, text: &str, candidates: &[String]) -> Result<Vec<Scored>> {
        let asked = stems(text);
        let mut out: Vec<Scored> = candidates
            .iter()
            .map(|label| {
                // Ключ варианта («city\t…») к словам не относится
                let own = stems(label.split_once('\t').map_or(label.as_str(), |(_, t)| t));
                let hits = asked.iter().filter(|a| own.iter().any(|o| same_stem(a, o))).count();
                let prob = if asked.is_empty() { 0.0 } else { hits as f32 / asked.len() as f32 };
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

    /// Заглушка диалога.
    ///
    /// Обрабатывает ровно одно замечание — «короче»: обрезает легенду
    /// до первой фразы. Это детерминированно, проверяемо и не
    /// притворяется пониманием. На остальные замечания честно
    /// отвечает, что нужна настоящая модель.
    async fn chat_json(
        &self,
        messages: serde_json::Value,
        _max_tokens: u32,
        _schema: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let last = messages
            .as_array()
            .and_then(|a| a.last())
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("");

        // Текущий вызов приходит первым сообщением пользователя
        let legend = messages
            .as_array()
            .and_then(|a| a.iter().find_map(|m| {
                let c = m.get("content")?.as_str()?;
                let start = c.find("\"legend\":")?;
                let rest = &c[start + 9..];
                let open = rest.find('"')? + 1;
                let end = rest[open..].find('"')?;
                Some(rest[open..open + end].to_string())
            }))
            .unwrap_or_default();

        if last.to_lowercase().contains("короче") && !legend.is_empty() {
            let short = legend
                .split_inclusive(['.', '!', '?'])
                .next()
                .unwrap_or(&legend)
                .trim()
                .to_string();
            return Ok(serde_json::json!({
                "reply": "Сократил легенду до первой фразы.",
                "patch": { "legend": short },
            }));
        }

        Ok(serde_json::json!({
            "reply": "Замечание записано. Для правки текста нужна языковая модель: \
                      поднимите её и укажите ML_PROVIDER=http.",
        }))
    }

    async fn group_insights(&self, stats: serde_json::Value) -> Result<String> {
        // Принципиально: заглушка НЕ выдумывает цифры, а пересказывает
        // настоящую агрегацию (reports::group::GroupStats). Демо выглядит
        // честно, а при замене на реальную модель контракт не меняется.
        let num = |key: &str| stats.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
        let real = |key: &str| stats.get(key).and_then(|v| v.as_f64());

        let sessions = num("sessions");
        if sessions == 0 {
            return Ok("За выбранный период занятий не было.".into());
        }

        let mut lines = vec![format!(
            "Занятий: {sessions}, участников: {}.",
            num("members")
        )];

        if let Some(score) = real("avgScore") {
            lines.push(format!("Средний балл {score:.0}."));
        }

        let over = num("overNorm");
        if over > 0 {
            lines.push(format!(
                "Норматив превышен в {over} занятиях из {sessions} ({:.0}%).",
                real("overNormShare").unwrap_or(0.0) * 100.0
            ));
        } else {
            lines.push("Норматив выдержан во всех занятиях.".into());
        }

        // Главное, ради чего тренажёр и делался: совпало с эталоном,
        // но выяснено не было
        let guessed = num("guessed");
        if guessed > 0 {
            lines.push(format!(
                "Угадано, а не выяснено: {guessed} значений — {:.0}% от совпавших с эталоном. \
                 Это поля, которые курсанты заполнили, не спросив заявителя.",
                real("guessedShare").unwrap_or(0.0) * 100.0
            ));
        }

        if let Some(items) = stats.get("topProblems").and_then(|v| v.as_array()) {
            let top: Vec<String> = items
                .iter()
                .take(3)
                .map(|item| {
                    let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("поле");
                    let count = item.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                    format!("«{label}» — {count}")
                })
                .collect();
            if !top.is_empty() {
                lines.push(format!("Чаще всего замечания вызывали: {}.", top.join(", ")));
            }
        }

        Ok(lines.join(" "))
    }
}

/// Стабильный хеш строки: нужен, чтобы заглушки давали одинаковый
/// результат между запусками.
pub(crate) fn stable_hash(s: &str) -> u32 {
    s.bytes().fold(2166136261u32, |h, b| {
        (h ^ b as u32).wrapping_mul(16777619)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn вопросы() -> Vec<String> {
        [
            "Уточните, пожалуйста, город — вы в Москве?",
            "Пострадавшие есть?",
            "Дом газифицирован?",
            "Что за дом — частный, многоквартирный?",
            "Открытое пламя видно или только дым?",
        ]
        .map(String::from)
        .to_vec()
    }

    async fn лучший(text: &str) -> Option<String> {
        let c = вопросы();
        let scored = MockLabelScorer.score(text, &c).await.unwrap();
        crate::domain::free_question::pick(&scored, &c).map(|i| c[i].clone())
    }

    #[tokio::test]
    async fn заглушка_понимает_вопрос_своими_словами() {
        assert_eq!(лучший("какой город?").await.as_deref(),
                   Some("Уточните, пожалуйста, город — вы в Москве?"));
        assert_eq!(лучший("люди пострадали?").await.as_deref(), Some("Пострадавшие есть?"));
        assert_eq!(лучший("газ в доме есть?").await.as_deref(), Some("Дом газифицирован?"));
        assert_eq!(лучший("пламя есть").await.as_deref(),
                   Some("Открытое пламя видно или только дым?"));
    }

    #[tokio::test]
    async fn заглушка_не_отвечает_на_вопрос_не_по_делу() {
        assert_eq!(лучший("как вас зовут?").await, None);
        assert_eq!(лучший("ваш номер телефона").await, None);
        assert_eq!(лучший("").await, None);
    }
}
