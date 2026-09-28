//! Живой заявитель: реплику в разговоре формулирует модель.
//!
//! Что заявитель сообщает — по-прежнему решает тренажёр. Какой факт
//! раскрыт вопросом, определяет сопоставление (`free_question`) или
//! кнопка, и разбор «выяснил / угадал» от модели не зависит. Модель
//! только говорит это своими словами: коротко, по-разговорному, с
//! волнением человека, у которого горит дом.
//!
//! Утечь нераскрытому неоткуда: в запрос уходят легенда, уже
//! раскрытые ответы и ответ на текущий вопрос — ответов на незаданные
//! вопросы модель не видит вовсе. Но модель может и присочинить,
//! и потерять суть. Поэтому реплика проходит проверку до показа:
//! числа в ней должны встречаться в том, что ей дали, и второй запрос
//! к модели подтверждает, что ответ передан и ничего не добавлено.
//! Не прошла, не успела — заявитель говорит заготовленную фразу
//! сценария, как без модели.
//!
//! Включается настройкой `applicant.live_model`: на процессоре реплика
//! шла бы десятки секунд, для телефонного разговора это непригодно.

use crate::ml::Llm;
use serde_json::{json, Value};
use std::time::Duration;

/// Что знает заявитель к моменту реплики.
#[derive(Debug, Clone)]
pub struct Context {
    /// ФИО и статус («очевидец», «пострадавший») — из сценария
    pub name: String,
    pub status: String,
    /// Первая фраза заявителя
    pub legend: String,
    /// Уже раскрытые ответы: (вопрос сценария, ответ)
    pub revealed: Vec<(String, String)>,
    /// Ответ на текущий вопрос. `None` — вопрос не по делу
    pub current: Option<String>,
    /// Последние реплики разговора: (кто, текст)
    pub recent: Vec<(String, String)>,
    /// Что спросил оператор — дословно
    pub question: String,
}

/// Чем кончилась попытка.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Реплика модели прошла проверку
    Live(String),
    /// Реплика была, но проверку не прошла: отдаём заготовленную фразу,
    /// а отбракованную — в журнал, чтобы преподаватель видел, что
    /// модель хотела сказать
    Rejected { reason: &'static str, reply: String },
    /// Реплики нет (модель не ответила, не успела): заготовленная фраза
    Fallback(&'static str),
}

/// Сколько реплик разговора отдаём модели: хватает, чтобы не
/// повторяться, и запрос остаётся коротким.
pub const RECENT_LINES: usize = 6;

fn system() -> String {
    "Ты — заявитель, который звонит в службу 112. Отвечаешь оператору \
     одной-двумя короткими фразами, как живой взволнованный человек по \
     телефону: своими словами, а не зачитывая записанный ответ; можно \
     поторопить, переспросить, выдать волнение («да быстрее же!», «ой, \
     не знаю…»). Сведения при этом — ТОЛЬКО из разделов «Что ты знаешь» \
     и «Ответ на этот вопрос». Ничего не придумывай: ни адресов, ни \
     этажей, ни чисел, ни людей, ни подробностей, которых там нет. Если \
     ответа на вопрос нет — скажи, что не знаешь или не понял, при чём \
     тут это, без новых сведений. Если спрашивают о том, что ты уже \
     говорил, — повтори это. Суть ответа на текущий вопрос обязательно \
     сохрани. Числа и адреса называй так же, как они записаны. \
     Не повторяй восклицания, которые уже говорил в этом разговоре."
        .into()
}

/// Сообщения для генерации реплики.
pub fn messages(ctx: &Context) -> Value {
    let mut known = format!("Ты: {} ({}).\nТвоя первая фраза в звонке: {}\n", ctx.name, ctx.status, ctx.legend);
    for (q, a) in &ctx.revealed {
        known.push_str(&format!("На вопрос «{q}» ты уже ответил: {a}\n"));
    }
    let current = match &ctx.current {
        Some(a) => format!("Ответ на этот вопрос (суть обязательна): {a}"),
        None => "Ответа на этот вопрос у тебя нет: скажи, что не знаешь или не понял.".into(),
    };
    let dialog: String = ctx
        .recent
        .iter()
        .map(|(who, text)| format!("{who}: {text}\n"))
        .collect();
    json!([
        { "role": "system", "content": system() },
        { "role": "user", "content": format!(
            "Что ты знаешь:\n{known}\nРазговор до этого:\n{dialog}\nОператор спрашивает: {}\n\n{current}\n\nТвоя реплика:",
            ctx.question) },
    ])
}

/// Реплика — одна короткая строка. Лимит длины в схеме не даёт модели
/// уйти в рассуждения: без него она на крупной модели ходила по кругу,
/// пока не кончались токены (так было с правкой вызова).
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["reply"],
        "properties": { "reply": { "type": "string", "minLength": 1, "maxLength": 240 } },
    })
}

/// Вопросы проверки. Ответы — перечислением, чтобы вердикт был словом,
/// а не рассуждением.
pub fn verdict_messages(ctx: &Context, reply: &str) -> Value {
    let mut known = format!("{}\n", ctx.legend);
    for (_, a) in &ctx.revealed {
        known.push_str(a);
        known.push('\n');
    }
    if let Some(a) = &ctx.current {
        known.push_str(a);
        known.push('\n');
    }
    let keeps = match &ctx.current {
        Some(a) => format!("Передаёт ли реплика суть ответа «{a}»?"),
        // Вопрос не по делу: годится любая реакция без ответа по существу —
        // «не знаю», «не понял», «при чём тут это?!». Новые сведения
        // ловит второй вопрос
        None => "Заявитель в реплике не отвечает по существу: говорит, что не знает, \
                 не понял или не понимает, при чём тут этот вопрос?"
            .into(),
    };
    json!([
        { "role": "system", "content":
          "Ты проверяешь реплику заявителя в учебном звонке 112. Отвечай строго по тексту." },
        { "role": "user", "content": format!(
            "Сведения, которые у заявителя есть:\n{known}\nРеплика заявителя: «{reply}»\n\n\
             1. {keeps}\n\
             2. Есть ли в реплике сведения о происшествии, которых нет в списке выше \
             (новые места, числа, люди, обстоятельства)? Эмоции и слова вроде «пожалуйста, \
             быстрее» сведениями не считаются.") },
    ])
}

pub fn verdict_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["keeps", "adds"],
        "properties": {
            "keeps": { "type": "string", "enum": ["да", "нет"] },
            "adds": { "type": "string", "enum": ["да", "нет"] },
        },
    })
}

/// Все числа в тексте. Выдуманное число — самая опасная выдумка:
/// этаж, дом, число пострадавших курсант запишет в карточку.
fn numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Числа реплики есть в том, что модели дали. Проверка кодом, до
/// второго запроса: дёшево и без модели ловит самое опасное.
pub fn numbers_known(ctx: &Context, reply: &str) -> bool {
    let mut known = ctx.legend.clone();
    for (_, a) in &ctx.revealed {
        known.push(' ');
        known.push_str(a);
    }
    if let Some(a) = &ctx.current {
        known.push(' ');
        known.push_str(a);
    }
    for (_, t) in &ctx.recent {
        known.push(' ');
        known.push_str(t);
    }
    known.push(' ');
    known.push_str(&ctx.question);
    let have = numbers(&known);
    numbers(reply).iter().all(|n| have.contains(n))
}

/// Разбор ответа генерации: строка без переводов строк и кавычек-ёлочек
/// по краям, иначе `None`.
pub fn parse_reply(v: &Value) -> Option<String> {
    let r = v.get("reply")?.as_str()?.trim();
    let r = r.trim_matches(|c| c == '«' || c == '»' || c == '"').trim();
    if r.is_empty() {
        return None;
    }
    Some(r.replace('\n', " "))
}

/// Реплика прошла проверку, если суть сохранена и ничего не добавлено.
pub fn verdict_ok(v: &Value) -> bool {
    v.get("keeps").and_then(Value::as_str) == Some("да")
        && v.get("adds").and_then(Value::as_str) == Some("нет")
}

/// Сформулировать реплику и проверить её. Всё — в пределах `budget`:
/// разговор не должен вставать из-за модели.
pub async fn reply(llm: &dyn Llm, ctx: &Context, budget: Duration) -> Outcome {
    let work = async {
        let generated = match llm.chat_json(messages(ctx), 200, Some(schema())).await {
            Ok(v) => v,
            Err(_) => return Outcome::Fallback("модель не ответила"),
        };
        let Some(text) = parse_reply(&generated) else {
            return Outcome::Fallback("пустая реплика");
        };
        if !numbers_known(ctx, &text) {
            return Outcome::Rejected { reason: "в реплике число, которого нет в сценарии", reply: text };
        }
        match llm.chat_json(verdict_messages(ctx, &text), 40, Some(verdict_schema())).await {
            Ok(v) if verdict_ok(&v) => Outcome::Live(text),
            Ok(v) => {
                let reason = if v.get("keeps").and_then(Value::as_str) != Some("да") {
                    "проверка: суть ответа потеряна"
                } else {
                    "проверка: добавлены сведения сверх сценария"
                };
                Outcome::Rejected { reason, reply: text }
            }
            Err(_) => Outcome::Fallback("проверка не ответила"),
        }
    };
    match tokio::time::timeout(budget, work).await {
        Ok(outcome) => outcome,
        Err(_) => Outcome::Fallback("не успела за отведённое время"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(current: Option<&str>) -> Context {
        Context {
            name: "Иванова Инна Степановна".into(),
            status: "очевидец".into(),
            legend: "Горит крыша частного дома, Станционная, 28!".into(),
            revealed: vec![("Пострадавшие есть?".into(), "Нет, пострадавших нет.".into())],
            current: current.map(Into::into),
            recent: vec![],
            question: "Город какой?".into(),
        }
    }

    #[test]
    fn число_из_сценария_проходит() {
        assert!(numbers_known(&ctx(Some("Королёв.")), "Королёв, Станционная 28, скорее!"));
    }

    #[test]
    fn выдуманное_число_не_проходит() {
        // Цифрами — ловит код. Числа словами («пятый этаж») ловит второй
        // запрос к модели: сведения сверх контекста
        assert!(!numbers_known(&ctx(Some("Королёв.")), "Королёв, дом 5"));
        assert!(!numbers_known(&ctx(Some("Королёв.")), "Королёв, 2 человека внутри"));
    }

    #[test]
    fn нераскрытое_не_попадает_в_запрос() {
        // Контекст собирается только из раскрытого: проверяем, что в
        // сообщениях ровно то, что в нём есть
        let m = messages(&ctx(Some("Королёв."))).to_string();
        assert!(m.contains("Королёв"));
        assert!(m.contains("пострадавших нет"));
        assert!(!m.contains("газ"), "в контексте не было газа — его и не должно быть");
    }

    #[test]
    fn вердикт_только_да_и_нет() {
        assert!(verdict_ok(&json!({ "keeps": "да", "adds": "нет" })));
        assert!(!verdict_ok(&json!({ "keeps": "нет", "adds": "нет" })));
        assert!(!verdict_ok(&json!({ "keeps": "да", "adds": "да" })));
    }

    /// Подставная модель: реплика, вердикт и задержка — какие скажем.
    struct Модель {
        реплика: &'static str,
        вердикт: Value,
        задержка: Duration,
    }

    #[async_trait::async_trait]
    impl Llm for Модель {
        async fn generate_scenario(&self, _: Value) -> anyhow::Result<Value> { unimplemented!() }
        async fn refine_scenario(&self, _: Value) -> anyhow::Result<Value> { unimplemented!() }
        async fn group_insights(&self, _: Value) -> anyhow::Result<String> { unimplemented!() }
        async fn chat_json(&self, _: Value, _: u32, schema: Option<Value>) -> anyhow::Result<Value> {
            tokio::time::sleep(self.задержка).await;
            let проверка = schema.is_some_and(|s| s["properties"].get("keeps").is_some());
            Ok(if проверка { self.вердикт.clone() } else { json!({ "reply": self.реплика }) })
        }
    }

    fn модель(реплика: &'static str, вердикт: Value, мс: u64) -> Модель {
        Модель { реплика, вердикт, задержка: Duration::from_millis(мс) }
    }

    #[tokio::test]
    async fn прошедшая_проверку_реплика_звучит() {
        let м = модель("Да Королёв же, Московская область!", json!({ "keeps": "да", "adds": "нет" }), 0);
        let итог = reply(&м, &ctx(Some("Нет, это Королёв, Московская область.")), Duration::from_secs(1)).await;
        assert_eq!(итог, Outcome::Live("Да Королёв же, Московская область!".into()));
    }

    #[tokio::test]
    async fn отбракованная_реплика_заменяется_заготовленной() {
        let м = модель("Королёв, и сосед обжёгся!", json!({ "keeps": "да", "adds": "да" }), 0);
        let итог = reply(&м, &ctx(Some("Королёв.")), Duration::from_secs(1)).await;
        assert_eq!(итог, Outcome::Rejected {
            reason: "проверка: добавлены сведения сверх сценария",
            reply: "Королёв, и сосед обжёгся!".into(),
        });
    }

    #[tokio::test]
    async fn выдуманное_число_отбраковывается_без_второго_запроса() {
        let м = модель("Королёв, пятый этаж, квартира 12!", json!({ "keeps": "да", "adds": "нет" }), 0);
        let итог = reply(&м, &ctx(Some("Королёв.")), Duration::from_secs(1)).await;
        assert!(matches!(итог, Outcome::Rejected { reason: "в реплике число, которого нет в сценарии", .. }),
                "{итог:?}");
    }

    #[tokio::test]
    async fn медленная_модель_не_держит_разговор() {
        let м = модель("Королёв!", json!({ "keeps": "да", "adds": "нет" }), 500);
        let итог = reply(&м, &ctx(Some("Королёв.")), Duration::from_millis(100)).await;
        assert_eq!(итог, Outcome::Fallback("не успела за отведённое время"));
    }

    #[test]
    fn реплика_без_кавычек_и_переводов_строк() {
        assert_eq!(parse_reply(&json!({ "reply": "«Королёв,\nМосковская область»" })).unwrap(),
                   "Королёв, Московская область");
        assert!(parse_reply(&json!({ "reply": "  " })).is_none());
    }
}
