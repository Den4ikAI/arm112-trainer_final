//! Доклад диспетчера ДДС вышестоящему начальнику.
//!
//! Заказчик в чате задачи, 27.09.2026 (сообщение 781, п. 7): голосовое
//! общение «ДДС с вышестоящим начальником и ДДС с руководителем
//! бригады». Разговор с бригадой уже есть (`domain::brigade`), здесь —
//! второй собеседник.
//!
//! Что докладывают начальнику, в материалах не сказано. Наше допущение
//! (OPEN-QUESTIONS.md, вопрос 22): то, что диспетчер знает по карточке
//! и ходу работ, — что случилось, где, есть ли пострадавшие, кого ещё
//! оповестили, какую бригаду направил и на каком она этапе. Ничего
//! сверх карточки начальник не спрашивает и не сообщает: обстановки на
//! месте у нас нет, и выдумывать её нельзя.
//!
//! Устройство то же, что у анализа ручного ввода (`text_analysis`):
//!
//! * **Что доложено, решает модель, но цитату проверяет код.** Модель
//!   отмечает сведение доложенным только с дословной фразой диспетчера;
//!   фразы нет в докладе — отметка отбрасывается.
//! * **Без модели режим работает.** На заглушке и при ошибке модели
//!   доложенное ищется правилами по ключевым словам, а вопросы
//!   начальника берутся из готовых формулировок. Это грубее, но звонок
//!   не ломается и разбор есть.
//! * **Вопрос начальника не подсказывает ответ.** Модель видит сведения
//!   карточки — иначе ей не сверить доклад. Вопрос с цифрой или словом
//!   из этих сведений («Пострадавших двое?») заменяется готовой
//!   формулировкой.

use crate::domain::text_analysis::{found_in, normalize};
use crate::ml::Llm;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Сколько уточняющих вопросов задаёт начальник. Дальше — «Принял»:
/// начальник на телефоне не ведёт допрос, а разбор и так покажет,
/// чего в докладе не было.
pub const MAX_QUESTIONS: usize = 3;

pub const GREETING: &str = "Слушаю вас. Докладывайте.";
pub const ACCEPTED: &str = "Принял.";

/// Сведение, которое ждут в докладе.
#[derive(Debug, Clone)]
pub struct Fact {
    pub key: &'static str,
    pub title: &'static str,
    /// Что в карточке. Показывается в разборе как эталон
    pub expected: String,
    /// Основы слов, по которым правила узнают сведение в докладе
    cues: Vec<String>,
    /// Фразы, по которым правила узнают, что доложено неверно
    wrong_cues: Vec<String>,
}

/// Порядок, в котором начальник спрашивает о недоложенном: сначала
/// что и где, потом люди, потом силы.
const PRIORITY: [&str; 6] = ["what", "where", "victims", "brigade", "stage", "services"];

/// Готовые вопросы — на заглушке и когда вопрос модели не годится.
pub fn template_question(key: &str) -> &'static str {
    match key {
        "what" => "Что случилось? Доложите, что за происшествие.",
        "where" => "Где это? Назовите адрес.",
        "victims" => "Пострадавшие есть?",
        "brigade" => "Кого направили на место?",
        "stage" => "Что сейчас с бригадой, на каком этапе работы?",
        "services" => "Какие ещё службы оповещены?",
        _ => "Уточните, пожалуйста.",
    }
}

/// Основа слова для поиска: первые пять букв. Грубо, но «Станционная»
/// и «на Станционной» так совпадают, а словаря русской морфологии у нас
/// нет и тянуть его ради этого не стоит.
fn stem(word: &str) -> String {
    let w = normalize(word);
    w.chars().take(5).collect()
}

/// Слова адреса, которые ничего не различают.
const ADDRESS_NOISE: [&str; 12] = [
    "улиц", "проез", "просп", "перее", "шоссе", "облас", "город", "район", "посел", "корпу",
    "строе", "дом",
];

fn words(s: &str, min_len: usize) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(move |w| w.chars().count() >= min_len)
        .map(stem)
}

/// Доклад одной строкой в нормализованном виде.
fn joined(said: &[String]) -> String {
    normalize(&said.join(" \n "))
}

/// Фраза диспетчера, в которой найдена основа: её показываем в разборе.
fn sentence_with(said: &[String], cue: &str) -> Option<String> {
    said.iter()
        .flat_map(|line| line.split_inclusive(['.', '!', '?', ';']))
        .map(str::trim)
        .find(|s| normalize(s).contains(cue))
        .map(str::to_string)
}

// ─── Что ждут в докладе ──────────────────────────────────────────────

/// Сведения карточки и хода работ, которые диспетчер знает и может
/// доложить. Только то, что он видел: доклады бригады — полученные.
pub async fn facts(pool: &PgPool, card_id: Uuid, service_id: &str) -> Result<Vec<Fact>> {
    #[allow(clippy::type_complexity)]
    let (type_ids, outcome, address, victims, description): (Vec<String>, Option<String>, Value, Option<i32>, String) =
        sqlx::query_as("SELECT type_ids, outcome_type, address, victims, description FROM cards WHERE id = $1")
            .bind(card_id)
            .fetch_one(pool)
            .await?;

    let types: Vec<String> =
        sqlx::query_scalar("SELECT title FROM incident_types WHERE id = ANY($1) ORDER BY title")
            .bind(&type_ids)
            .fetch_all(pool)
            .await?;

    let mut out = Vec::new();

    // Что случилось: итоговый тип классификатора, типы «Что случилось?»
    // и описание. Тип бывает просто номером («101») — докладывают не
    // номер, а суть, поэтому такие в сведение не берём
    let mut what: Vec<String> = Vec::new();
    if let Some(o) = outcome.as_ref().filter(|o| !o.trim().is_empty()) {
        what.push(o.clone());
    }
    what.extend(types.iter().filter(|t| t.chars().any(char::is_alphabetic)).cloned());
    let mut cues: Vec<String> = what.iter().flat_map(|t| words(t, 3).collect::<Vec<_>>()).collect();
    // Слова описания: диспетчер пересказывает его своими словами, и
    // «горит крыша» — такой же доклад о происшествии, как «пожар»
    cues.extend(words(&description, 5).filter(|s| !ADDRESS_NOISE.iter().any(|n| s.starts_with(n))));
    cues.sort();
    cues.dedup();
    let expected = if !what.is_empty() {
        what.join("; ")
    } else if !description.trim().is_empty() {
        description.trim().chars().take(120).collect()
    } else {
        "тип происшествия не выбран".into()
    };
    out.push(Fact { key: "what", title: "Что случилось", expected, cues, wrong_cues: vec![] });

    // Где: населённый пункт, улица, дом. Страну и регион не ждём —
    // их никто не докладывает
    let field = |k: &str| address.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let parts: Vec<String> = ["settlement", "district", "street", "house", "building"]
        .iter()
        .map(|k| field(k))
        .filter(|v| !v.is_empty())
        .collect();
    let descriptive = field("descriptive");
    // Город один — ещё не адрес: бригаду по нему не найти. Если в карточке
    // есть улица или дом, ждём их; населённый пункт засчитывается, только
    // когда ничего точнее нет
    let precise: Vec<String> = ["street", "house", "building"]
        .iter()
        .map(|k| field(k))
        .filter(|v| !v.is_empty())
        .collect();
    let mut cues: Vec<String> = (if precise.is_empty() { &parts } else { &precise })
        .iter()
        .flat_map(|p| {
            // Номер дома — цифры целиком, остальное — основы слов
            let digits: Vec<String> = p
                .split(|c: char| !c.is_ascii_digit())
                .filter(|d| !d.is_empty())
                .map(str::to_string)
                .collect();
            let mut w: Vec<String> = words(p, 4).filter(|s| !ADDRESS_NOISE.iter().any(|n| s.starts_with(n))).collect();
            w.extend(digits);
            w
        })
        .collect();
    cues.sort();
    cues.dedup();
    if cues.is_empty() {
        cues.push("адрес".into());
    }
    out.push(Fact {
        key: "where",
        title: "Где",
        expected: if !parts.is_empty() {
            parts.join(", ")
        } else if !descriptive.is_empty() {
            descriptive
        } else {
            "адрес в карточке не заполнен".into()
        },
        cues,
        wrong_cues: vec![],
    });

    // Пострадавшие: число из карточки. «Нет» — тоже сведение, его
    // докладывают так же обязательно
    let victim_cues = ["пострадав", "ранен", "погиб", "жертв", "травм", "постра"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (expected, wrong_cues) = match victims {
        Some(n) if n > 0 => (
            n.to_string(),
            vec!["нет пострадав", "без пострадав", "пострадавших нет", "никто не пострадал"],
        ),
        Some(_) => ("нет".to_string(), vec!["есть пострадав", "имеются пострадав"]),
        None => ("не отмечены".to_string(), vec!["есть пострадав", "имеются пострадав"]),
    };
    out.push(Fact {
        key: "victims",
        title: "Пострадавшие",
        expected,
        cues: victim_cues,
        wrong_cues: wrong_cues.into_iter().map(str::to_string).collect(),
    });

    // Бригада и этап — только если диспетчер её направил
    let assignment: Option<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, brigade_no FROM brigade_assignments WHERE card_id = $1 AND service_id = $2",
    )
    .bind(card_id)
    .bind(service_id)
    .fetch_optional(pool)
    .await?;

    if let Some((assignment_id, brigade_no)) = assignment {
        out.push(Fact {
            key: "brigade",
            title: "Кого направил",
            expected: format!("бригада {brigade_no}"),
            cues: vec!["бригад".into(), "направ".into(), "выслал".into()],
            wrong_cues: vec![],
        });

        let delivered: Vec<String> = sqlx::query_scalar(
            "SELECT stage::text FROM brigade_reports
              WHERE assignment_id = $1 AND delivered_at IS NOT NULL",
        )
        .bind(assignment_id)
        .fetch_all(pool)
        .await?;
        let last = crate::domain::brigade::STAGES
            .iter()
            .rev()
            .find(|s| delivered.iter().any(|d| d == *s));
        let (expected, cues): (String, &[&str]) = match last.copied() {
            Some("Начало реагирования") => ("бригада выехала".into(), &["выех", "выезж", "следу", "в пути", "выдвин"]),
            Some("Прибытие") => ("бригада прибыла на место".into(), &["прибы", "на мест"]),
            Some("Проведение работ") => ("бригада ведёт работы".into(), &["работ", "туш", "ликвид", "приступ"]),
            Some("Работы завершены") => ("работы завершены".into(), &["заверш", "закончи", "ликвидир"]),
            _ => (
                "докладов от старшего ещё не было".into(),
                &["доклад", "без изменен", "пока нет", "еще нет", "сведений нет", "не доклад"],
            ),
        };
        out.push(Fact {
            key: "stage",
            title: "Ход работ",
            expected,
            cues: cues.iter().map(|s| s.to_string()).collect(),
            wrong_cues: vec![],
        });
    }

    // Кого ещё оповестили: соседние службы по карточке
    let others: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT COALESCE(n.short_title, s.title), COALESCE(n.aliases, '{}')
           FROM card_services cs
           JOIN services s ON s.id = cs.service_id
           LEFT JOIN service_names n ON n.service_id = s.id
          WHERE cs.card_id = $1 AND cs.service_id <> $2
          ORDER BY 1",
    )
    .bind(card_id)
    .bind(service_id)
    .fetch_all(pool)
    .await?;
    if !others.is_empty() {
        let mut cues: Vec<String> = others
            .iter()
            .flat_map(|(title, aliases)| {
                let mut w: Vec<String> = words(title, 3)
                    .filter(|s| s != "служб")
                    .collect();
                w.extend(aliases.iter().flat_map(|a| words(a, 2).collect::<Vec<_>>()));
                w
            })
            .collect();
        cues.push("служб".into());
        cues.sort();
        cues.dedup();
        out.push(Fact {
            key: "services",
            title: "Кого ещё оповестили",
            expected: others.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(", "),
            cues,
            wrong_cues: vec![],
        });
    }

    Ok(out)
}

// ─── Разбор доклада ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemReview {
    pub key: String,
    pub title: String,
    /// Что в карточке
    pub expected: String,
    /// `reported` — доложено, `missing` — нет, `wrong` — доложено неверно
    pub status: String,
    /// Фраза диспетчера, дословно
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChiefReview {
    pub items: Vec<ItemReview>,
    /// `model` — доложенное отмечала модель, `rules` — ключевые слова
    pub mode: String,
    /// Отметок модели, отброшенных проверкой цитат
    pub dropped: i32,
    pub reported: usize,
    pub total: usize,
    pub note: Option<String>,
    /// Разбор пока по ключевым словам, модель его ещё уточняет
    #[serde(default)]
    pub pending: bool,
    /// Сколько модель разбирала доклад
    #[serde(default)]
    pub model_ms: Option<i32>,
}

impl ChiefReview {
    fn new(items: Vec<ItemReview>, mode: &str, dropped: i32, note: Option<String>) -> Self {
        let reported = items.iter().filter(|i| i.status == "reported").count();
        let total = items.len();
        Self { items, mode: mode.into(), dropped, reported, total, note, pending: false, model_ms: None }
    }

    pub fn missing(&self) -> Vec<&str> {
        self.items.iter().filter(|i| i.status == "missing").map(|i| i.key.as_str()).collect()
    }
}

/// Разбор правилами: ключевые слова в докладе.
pub fn evaluate_rules(facts: &[Fact], said: &[String]) -> ChiefReview {
    let text = joined(said);
    let items = facts
        .iter()
        .map(|f| {
            let wrong = f.wrong_cues.iter().map(|c| normalize(c)).find(|c| text.contains(c.as_str()));
            let hit = f.cues.iter().find(|c| !c.is_empty() && text.contains(c.as_str()));
            let (status, cue) = match (wrong, hit) {
                (Some(w), _) => ("wrong", Some(w)),
                (None, Some(h)) => ("reported", Some(h.clone())),
                (None, None) => ("missing", None),
            };
            ItemReview {
                key: f.key.into(),
                title: f.title.into(),
                expected: f.expected.clone(),
                status: status.into(),
                quote: cue.and_then(|c| sentence_with(said, &c)),
            }
        })
        .collect();
    ChiefReview::new(items, "rules", 0, None)
}

const SYSTEM: &str = "\
Ты — начальник дежурной смены. Диспетчер дежурно-диспетчерской службы \
докладывает тебе по телефону о происшествии. Тебе даны сведения из \
карточки происшествия — то, что диспетчер знает, — и всё, что он уже \
сказал.

items — по каждому сведению из карточки, ключ key как в карточке:
- status «доложено» — диспетчер сообщил это сведение, пусть и своими \
словами; «неверно» — сообщил, но не то, что в карточке (другой адрес, \
«пострадавших нет», когда они есть); «не доложено» — не упоминал. \
Адрес доложен, если названы улица и дом; одного города мало;
- quote — дословная фраза диспетчера, где он это сообщил (для \
«не доложено» — пустая строка).

question — если что-то не доложено: один короткий вопрос начальника по \
телефону об одном из недоложенных сведений, key — ключ этого сведения. \
Спрашивай, а не подсказывай: не называй в вопросе ни адреса, ни чисел, \
ни названий из карточки. Хорошо: «Пострадавшие есть?», «Где это?». Если \
всё доложено — key и text пустые.

Цитаты копируй символ в символ. Грамотность и стиль не оценивай.";

pub fn schema(facts: &[Fact]) -> Value {
    let keys: Vec<&str> = facts.iter().map(|f| f.key).collect();
    let mut ask_keys = keys.clone();
    ask_keys.push("");
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "maxItems": keys.len(),
                "items": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "enum": keys },
                        "status": { "type": "string", "enum": ["доложено", "не доложено", "неверно"] },
                        "quote": { "type": "string" },
                    },
                    "required": ["key", "status", "quote"],
                }
            },
            "question": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "enum": ask_keys },
                    "text": { "type": "string" },
                },
                "required": ["key", "text"],
            }
        },
        "required": ["items", "question"],
    })
}

pub fn messages(facts: &[Fact], said: &[String]) -> Value {
    // Ключи по-русски: модель читает их как текст задания
    let body = json!({
        "сведения карточки": facts.iter()
            .map(|f| json!({ "key": f.key, "что": f.title, "в карточке": f.expected }))
            .collect::<Vec<_>>(),
        "доклад диспетчера": said,
    });
    json!([
        { "role": "system", "content": SYSTEM },
        { "role": "user", "content": body.to_string() },
    ])
}

/// Вопрос начальника годится, если не подсказывает ответ: без цифр и
/// без слов из сведений карточки, короткий и действительно вопрос.
pub fn question_is_clean(q: &str, facts: &[Fact]) -> bool {
    let q = q.trim();
    if q.is_empty() || q.chars().count() > 160 || !q.contains('?') {
        return false;
    }
    if q.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let nq = normalize(q);
    let leaks = facts.iter().flat_map(|f| words(&f.expected, 5).collect::<Vec<_>>());
    !leaks.into_iter().any(|w| nq.contains(&w))
}

/// Ответ модели → разбор. Отметка без подтверждённой цитаты
/// отбрасывается, и по этому сведению решают правила.
pub fn verify(raw: &Value, facts: &[Fact], said: &[String]) -> (ChiefReview, Option<(String, String)>) {
    let rules = evaluate_rules(facts, said);
    let text = joined(said);
    let mut dropped = 0;

    let model: Vec<&Value> = raw.get("items").and_then(|v| v.as_array()).map(|a| a.iter().collect()).unwrap_or_default();

    let items = rules
        .items
        .iter()
        .map(|r| {
            let Some(m) = model.iter().find(|m| m.get("key").and_then(|k| k.as_str()) == Some(r.key.as_str())) else {
                return r.clone();
            };
            let status = m.get("status").and_then(|s| s.as_str()).unwrap_or("");
            let quote = m.get("quote").and_then(|s| s.as_str()).map(str::trim).unwrap_or("");
            let mapped = match status {
                "доложено" => "reported",
                "неверно" => "wrong",
                "не доложено" => "missing",
                _ => return r.clone(),
            };
            if mapped == "missing" {
                // Ключевое слово могло найтись и там, где сведения нет
                // («работаем» — не доклад о работах бригады): смысл модель
                // читает лучше правил, и «не доложено» цитаты не требует
                return ItemReview { status: "missing".into(), quote: None, ..r.clone() };
            }
            if found_in(quote, &text) {
                ItemReview { status: mapped.into(), quote: Some(quote.to_string()), ..r.clone() }
            } else {
                dropped += 1;
                r.clone()
            }
        })
        .collect::<Vec<_>>();

    let review = ChiefReview::new(items, "model", dropped, None);

    let question = raw.get("question").and_then(|q| {
        let key = q.get("key")?.as_str()?.to_string();
        let text = q.get("text")?.as_str()?.trim().to_string();
        (!key.is_empty() && question_is_clean(&text, facts)).then_some((key, text))
    });

    (review, question)
}

/// Разбор доклада моделью — один раз, когда разговор окончен. Ошибка
/// модели — не ошибка звонка: остаётся разбор правилами с пометкой.
pub async fn review_with_model(llm: &dyn Llm, facts: &[Fact], said: &[String]) -> (ChiefReview, i32) {
    let started = std::time::Instant::now();
    let answer = llm.chat_json(messages(facts, said), 500, Some(schema(facts))).await;
    let ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
    let mut review = match answer {
        Ok(raw) => verify(&raw, facts, said).0,
        Err(e) => {
            let mut r = evaluate_rules(facts, said);
            r.note = Some(format!("модель не ответила, разбор по ключевым словам: {e}").chars().take(200).collect());
            r
        }
    };
    review.model_ms = Some(ms);
    (review, ms)
}

/// Что спросить дальше. `None` — пора сказать «Принял».
pub fn next_topic(review: &ChiefReview, asked: &[String], questions: usize) -> Option<&'static str> {
    if questions >= MAX_QUESTIONS {
        return None;
    }
    let missing = review.missing();
    PRIORITY
        .iter()
        .copied()
        .find(|k| missing.contains(k) && !asked.iter().any(|a| a == k))
}

/// Разобрать доклад и выбрать реплику начальника.
///
/// Возвращает разбор, реплику, её тему (`None` — «Принял») и сколько
/// думала модель.
pub async fn evaluate(
    llm: Option<&dyn Llm>,
    facts: &[Fact],
    said: &[String],
    asked: &[String],
    questions: usize,
) -> (ChiefReview, String, Option<String>, Option<i32>) {
    let mut model_ms = None;
    let (review, model_question) = match llm {
        None => (evaluate_rules(facts, said), None),
        Some(llm) => {
            let started = std::time::Instant::now();
            let answer = llm.chat_json(messages(facts, said), 500, Some(schema(facts))).await;
            model_ms = Some(started.elapsed().as_millis().min(i32::MAX as u128) as i32);
            match answer {
                Ok(raw) => {
                    let (review, q) = verify(&raw, facts, said);
                    (review, q)
                }
                Err(e) => {
                    let mut r = evaluate_rules(facts, said);
                    r.note = Some(format!("модель не ответила, разбор по ключевым словам: {e}").chars().take(200).collect());
                    (r, None)
                }
            }
        }
    };

    match next_topic(&review, asked, questions) {
        None => (review, ACCEPTED.to_string(), None, model_ms),
        Some(topic) => {
            // Вопрос модели — если он о том же, о чём пора спросить
            let text = model_question
                .filter(|(k, _)| k == topic)
                .map(|(_, t)| t)
                .unwrap_or_else(|| template_question(topic).to_string());
            (review, text, Some(topic.to_string()), model_ms)
        }
    }
}

// ─── Хранение ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// `dispatcher` или `chief`
    pub who: String,
    pub text: String,
    pub topic: Option<String>,
    pub model_ms: Option<i32>,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChiefView {
    pub id: Uuid,
    pub service_id: String,
    pub student: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub lines: Vec<Line>,
    /// Есть, когда начальник сказал «Принял»
    pub review: Option<ChiefReview>,
    pub max_questions: usize,
}

pub async fn lines(pool: &PgPool, report_id: Uuid) -> Result<Vec<Line>> {
    let rows: Vec<(String, String, Option<String>, Option<i32>, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT who, text, topic, model_ms, at FROM chief_report_lines
              WHERE report_id = $1 ORDER BY id",
        )
        .bind(report_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(who, text, topic, model_ms, at)| Line { who, text, topic, model_ms, at })
        .collect())
}

pub async fn add_line(
    pool: &PgPool,
    report_id: Uuid,
    who: &str,
    text: &str,
    topic: Option<&str>,
    model_ms: Option<i32>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO chief_report_lines (report_id, who, text, topic, model_ms)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(report_id)
    .bind(who)
    .bind(text)
    .bind(topic)
    .bind(model_ms)
    .execute(pool)
    .await?;
    Ok(())
}

/// Открытый звонок диспетчера по карточке или новый с приветствием.
pub async fn open(pool: &PgPool, card_id: Uuid, service_id: &str, user_id: Uuid) -> Result<Uuid> {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM chief_reports
          WHERE card_id = $1 AND service_id = $2 AND user_id = $3 AND finished_at IS NULL",
    )
    .bind(card_id)
    .bind(service_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO chief_reports (card_id, service_id, user_id) VALUES ($1, $2, $3)
         ON CONFLICT (card_id, service_id, user_id) WHERE finished_at IS NULL
         DO UPDATE SET started_at = chief_reports.started_at
         RETURNING id",
    )
    .bind(card_id)
    .bind(service_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    if lines(pool, id).await?.is_empty() {
        add_line(pool, id, "chief", GREETING, None, None).await?;
    }
    Ok(id)
}

/// Заменить разбор уже оконченного звонка — когда модель дочитала доклад.
pub async fn store_review(pool: &PgPool, report_id: Uuid, review: &ChiefReview) -> Result<()> {
    sqlx::query("UPDATE chief_reports SET review = $2 WHERE id = $1")
        .bind(report_id)
        .bind(serde_json::to_value(review)?)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn close(pool: &PgPool, report_id: Uuid, review: &ChiefReview) -> Result<()> {
    sqlx::query("UPDATE chief_reports SET finished_at = now(), review = $2 WHERE id = $1")
        .bind(report_id)
        .bind(serde_json::to_value(review)?)
        .execute(pool)
        .await?;
    Ok(())
}

/// Последний доклад службы по карточке. `user_id` — только этого
/// диспетчера (курсант видит свои звонки, не соседа).
pub async fn latest(
    pool: &PgPool,
    card_id: Uuid,
    service_id: &str,
    user_id: Option<Uuid>,
) -> Result<Option<ChiefView>> {
    #[allow(clippy::type_complexity)]
    let row: Option<(Uuid, String, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, Option<Value>)> =
        sqlx::query_as(
            "SELECT r.id, u.fio, r.started_at, r.finished_at, r.review
               FROM chief_reports r JOIN users u ON u.id = r.user_id
              WHERE r.card_id = $1 AND r.service_id = $2
                AND ($3::uuid IS NULL OR r.user_id = $3)
              ORDER BY r.started_at DESC, r.id DESC LIMIT 1",
        )
        .bind(card_id)
        .bind(service_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

    let Some((id, student, started_at, finished_at, review)) = row else {
        return Ok(None);
    };
    Ok(Some(ChiefView {
        id,
        service_id: service_id.to_string(),
        student,
        started_at,
        finished_at,
        lines: lines(pool, id).await?,
        review: review.and_then(|v| serde_json::from_value(v).ok()),
        max_questions: MAX_QUESTIONS,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn факты() -> Vec<Fact> {
        vec![
            Fact {
                key: "what",
                title: "Что случилось",
                expected: "Пожар".into(),
                cues: vec!["пожар".into(), "горит".into()],
                wrong_cues: vec![],
            },
            Fact {
                key: "where",
                title: "Где",
                expected: "Королёв, Станционная, 28".into(),
                cues: vec!["корол".into(), "станц".into(), "28".into()],
                wrong_cues: vec![],
            },
            Fact {
                key: "victims",
                title: "Пострадавшие",
                expected: "2".into(),
                cues: vec!["пострадав".into()],
                wrong_cues: vec!["нет пострадав".into(), "пострадавших нет".into()],
            },
        ]
    }

    fn сказал(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn правила_находят_доложенное_и_цитату() {
        let r = evaluate_rules(&факты(), &сказал(&["Пожар в частном доме. Адрес: Королёв, Станционная 28."]));
        assert_eq!(r.items[0].status, "reported");
        assert_eq!(r.items[1].status, "reported");
        assert_eq!(r.items[1].quote.as_deref(), Some("Адрес: Королёв, Станционная 28."));
        assert_eq!(r.items[2].status, "missing");
        assert_eq!((r.reported, r.total), (2, 3));
    }

    #[test]
    fn пострадавших_нет_при_двух_это_неверно() {
        let r = evaluate_rules(&факты(), &сказал(&["Пожар, пострадавших нет."]));
        assert_eq!(r.items[2].status, "wrong");
    }

    #[test]
    fn вопросы_по_порядку_и_не_больше_трёх() {
        let r = evaluate_rules(&факты(), &сказал(&["Докладываю."]));
        assert_eq!(next_topic(&r, &[], 0), Some("what"));
        assert_eq!(next_topic(&r, &["what".into()], 1), Some("where"));
        assert_eq!(next_topic(&r, &["what".into(), "where".into()], 2), Some("victims"));
        assert_eq!(next_topic(&r, &[], MAX_QUESTIONS), None, "после трёх вопросов — «Принял»");
    }

    #[test]
    fn всё_доложено_начальник_принимает() {
        let r = evaluate_rules(&факты(), &сказал(&["Пожар, Королёв, Станционная 28, двое пострадавших."]));
        assert_eq!(next_topic(&r, &[], 0), None);
    }

    #[test]
    fn вопрос_с_подсказкой_не_годится() {
        let f = факты();
        assert!(question_is_clean("Пострадавшие есть?", &f));
        assert!(!question_is_clean("Пострадавших двое, 2?", &f), "цифра — подсказка");
        assert!(!question_is_clean("Это на Станционной?", &f), "слово из адреса — подсказка");
        assert!(!question_is_clean("Доложите адрес", &f), "не вопрос");
    }

    #[test]
    fn отметка_модели_без_цитаты_отбрасывается() {
        let f = факты();
        let said = сказал(&["Горит дом."]);
        let raw = json!({
            "items": [
                { "key": "what", "status": "доложено", "quote": "Горит дом." },
                { "key": "where", "status": "доложено", "quote": "Станционная, 28" },
                { "key": "victims", "status": "не доложено", "quote": "" },
            ],
            "question": { "key": "where", "text": "Где это?" },
        });
        let (r, q) = verify(&raw, &f, &said);
        assert_eq!(r.items[0].status, "reported");
        assert_eq!(r.items[0].quote.as_deref(), Some("Горит дом."));
        assert_eq!(r.items[1].status, "missing", "адреса диспетчер не говорил");
        assert_eq!(r.dropped, 1);
        assert_eq!(r.mode, "model");
        assert_eq!(q, Some(("where".into(), "Где это?".into())));
    }

    #[test]
    fn вопрос_модели_с_подсказкой_заменяется_готовым() {
        let f = факты();
        let raw = json!({
            "items": [],
            "question": { "key": "where", "text": "Это Станционная, 28?" },
        });
        let (_, q) = verify(&raw, &f, &сказал(&["Пожар."]));
        assert_eq!(q, None);
    }
}
