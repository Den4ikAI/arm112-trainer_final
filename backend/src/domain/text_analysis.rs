//! Нейросетевой анализ ручного ввода.
//!
//! ТЗ: «оценка уровня подготовки специалистов, в том числе на основе
//! анализа ручного ввода текста» и «система оценки действий операторов
//! на основе нейросетевого анализа».
//!
//! Правила уже проверяют всё, что проверяется сравнением: признаки,
//! адрес, службы, статусы, время. Чего правила не умеют — прочитать
//! свободный текст. Оператор 112 пишет «описание со слов заявителя»,
//! и по нему диспетчер ДДС понимает, что случилось; диспетчер ДДС
//! пишет комментарии к статусам и мотивирует отказ. Здесь модель
//! сверяет этот текст с тем, что курсант на самом деле слышал.
//!
//! Две границы, ради которых модуль так устроен:
//!
//! * **Балл модель не меняет.** Замечания лежат рядом с баллом правил,
//!   а решает по ним преподаватель: подтверждает или отклоняет. Модель
//!   на десять миллиардов параметров ошибается, и превращать её ошибку
//!   в оценку человека нельзя.
//! * **Каждое замечание подтверждено цитатой, и цитата проверяется
//!   кодом.** Если фрагмента нет ни в тексте курсанта, ни в разговоре —
//!   замечание отбрасывается и учитывается в `dropped`. Так модель не
//!   может приписать курсанту слова, которых он не писал.

use crate::ml::Llm;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Вид задачи в очереди `ml_jobs`.
pub const JOB_KIND: &str = "analysis.text";

/// Сколько замечаний просим у модели. Больше — это уже не разбор,
/// а пересказ; и на процессоре каждое замечание стоит секунды.
const MAX_FINDINGS: usize = 6;

/// Короче этого цитата ничего не подтверждает: «да» найдётся в любом
/// разговоре.
const MIN_QUOTE_CHARS: usize = 3;

/// Чья попытка разбирается.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Описание со слов заявителя против стенограммы вызова
    Card112,
    /// Комментарии диспетчера к статусам против докладов бригады
    Dds,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Card112 => "card112",
            Self::Dds => "dds",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "card112" => Some(Self::Card112),
            "dds" => Some(Self::Dds),
            _ => None,
        }
    }

    /// Какие виды замечаний допустимы.
    fn allowed(self) -> &'static [&'static str] {
        match self {
            Self::Card112 => &["missing", "contradiction", "invented"],
            Self::Dds => &["no_reason", "inconsistent", "invented"],
        }
    }
}

/// Замечание модели после проверки цитат.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextFinding {
    /// `missing` — сказанное заявителем не попало в текст;
    /// `contradiction` — текст противоречит сказанному;
    /// `invented` — в тексте то, чего никто не сообщал;
    /// `no_reason` — отказ без мотивировки;
    /// `inconsistent` — комментарий расходится с докладом бригады
    pub kind: String,
    /// Суть расхождения одним предложением
    pub text: String,
    /// Фрагмент текста курсанта, дословно
    pub quote_input: Option<String>,
    /// Фрагмент того, что курсант слышал: реплика заявителя, доклад
    pub quote_source: Option<String>,
}

/// Что сравниваем.
#[derive(Debug, Clone, Default)]
pub struct Material {
    /// Текст курсанта: подпись поля → содержимое
    pub input: Vec<(String, String)>,
    /// Что курсант слышал или видел: кто → что
    pub source: Vec<(String, String)>,
}

impl Material {
    fn input_is_empty(&self) -> bool {
        self.input.iter().all(|(_, t)| t.trim().is_empty())
    }
}

// ─── Запрос к модели ─────────────────────────────────────────────────

const SYSTEM_112: &str = "\
Ты проверяешь, как оператор службы 112 записал в карточку происшествия \
то, что сообщил заявитель. Тебе дают стенограмму разговора и текст, \
который оператор ввёл вручную. По этому тексту диспетчеры служб решают, \
кого и с чем отправить на место.

Работай в два шага.

Шаг 1, statements. Разбей текст оператора на отдельные утверждения: \
одно утверждение — одна мысль («горит баня», «есть пострадавший»). \
Каждое предложение текста оператора должно попасть хотя бы в одно \
утверждение, ничего не пропускай. Для каждого:
- quoteInput — утверждение, дословно скопированное из текста оператора;
- verdict: «подтверждается» — заявитель сказал то же самое; \
«противоречит» — заявитель сказал иначе; «не сообщалось» — заявитель \
об этом ничего не говорил;
- quoteSource — дословная фраза заявителя, которая подтверждает или \
опровергает утверждение (для «не сообщалось» — пустая строка);
- text — одно короткое предложение: почему такой вердикт.
Сверяй смысл, а не слова: «горит баня» и «горит крыша дома» — разные \
объекты, это «противоречит»; «пострадавших нет» и «никто не пострадал» — \
одно и то же, это «подтверждается».

Шаг 2, missing. Перечисли существенные для выезда служб сведения, \
которые заявитель сказал, а в тексте оператора их нет совсем: что \
случилось и что именно горит, есть ли пострадавшие и угроза людям, \
видно ли пламя, ориентиры места. Для каждого: quoteSource — дословная \
фраза заявителя именно с этим сведением; text — что не записано. Адрес, \
ФИО и телефон записываются в отдельные поля карточки — их не \
перечисляй. Если сведение в тексте оператора есть, пусть и другими \
словами, — это не пропуск.

Цитаты копируй символ в символ, не пересказывай и не склеивай куски \
разных реплик. Грамотность и стиль не оценивай.";

const SYSTEM_DDS: &str = "\
Ты проверяешь комментарии, которые диспетчер дежурно-диспетчерской \
службы записал к статусам реагирования по карточке происшествия. Тебе \
дают описание происшествия из карточки 112, доклады старшего бригады \
(то, что диспетчер знал) и статусы с комментариями диспетчера.

statements — по одному на каждый непустой комментарий диспетчера:
- quoteInput — комментарий или его существенная часть, дословно;
- verdict: «понятно» — комментарий ясен и не расходится с тем, что знал \
диспетчер; «нет причины» — статус «Не принята» или «Отказ от выполнения \
работ», а комментарий не объясняет почему («нет», «отказ», набор букв); \
«противоречит» — комментарий расходится с докладом или описанием; \
«не сообщалось» — в комментарии сведение, которого нет ни в докладах, \
ни в описании;
- quoteSource — дословный фрагмент доклада или описания, с которым \
расходится комментарий (для остальных вердиктов — пустая строка);
- text — одно короткое предложение: почему такой вердикт.
missing оставь пустым списком.

Короткий, но понятный комментарий — это «понятно». Грамотность не \
оценивай. Цитаты копируй символ в символ.";

/// Вердикты по утверждениям. Вид замечания из вердикта выводит код,
/// а не модель: модели этого размера проще ответить на вопрос про одно
/// утверждение, чем самой найти все расхождения — пробовали, она
/// пропускала «баню» вместо «крыши дома».
fn verdicts(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Card112 => &["подтверждается", "противоречит", "не сообщалось"],
        Kind::Dds => &["понятно", "нет причины", "противоречит", "не сообщалось"],
    }
}

/// Схема ответа. Движок не даст модели выйти за неё: без схемы модель
/// этого размера регулярно кладёт цитату в `text` и наоборот.
pub fn schema(kind: Kind) -> Value {
    json!({
        "type": "object",
        "properties": {
            "statements": {
                "type": "array",
                "maxItems": 12,
                "items": {
                    "type": "object",
                    "properties": {
                        "quoteInput": { "type": "string" },
                        "verdict": { "type": "string", "enum": verdicts(kind) },
                        "quoteSource": { "type": "string" },
                        "text": { "type": "string" },
                    },
                    "required": ["quoteInput", "verdict", "quoteSource", "text"],
                }
            },
            "missing": {
                "type": "array",
                "maxItems": MAX_FINDINGS,
                "items": {
                    "type": "object",
                    "properties": {
                        "quoteSource": { "type": "string" },
                        "text": { "type": "string" },
                    },
                    "required": ["quoteSource", "text"],
                }
            }
        },
        "required": ["statements", "missing"],
    })
}

/// Ответ по утверждениям → замечания. Подтверждённое и понятное
/// замечанием не становится, пропуски у ДДС не бывают.
fn to_findings(kind: Kind, raw: &Value) -> Result<Value> {
    let statements = raw
        .get("statements")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("в ответе модели нет списка statements"))?;

    let mut out = Vec::new();
    for s in statements {
        let verdict = s.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
        let finding_kind = match (kind, verdict) {
            (_, "подтверждается" | "понятно") => continue,
            (Kind::Card112, "противоречит") => "contradiction",
            (Kind::Dds, "противоречит") => "inconsistent",
            (Kind::Dds, "нет причины") => "no_reason",
            (_, "не сообщалось") => "invented",
            // Чужой вердикт — пусть отбросит проверка
            _ => "unknown",
        };
        out.push(json!({
            "kind": finding_kind,
            "text": s.get("text"),
            "quoteInput": s.get("quoteInput"),
            "quoteSource": s.get("quoteSource"),
        }));
    }

    if kind == Kind::Card112 {
        for m in raw.get("missing").and_then(|v| v.as_array()).into_iter().flatten() {
            out.push(json!({
                "kind": "missing",
                "text": m.get("text"),
                "quoteInput": "",
                "quoteSource": m.get("quoteSource"),
            }));
        }
    }

    Ok(json!({ "findings": out }))
}

pub fn messages(kind: Kind, m: &Material) -> Value {
    let (system, source_title, input_title) = match kind {
        Kind::Card112 => (SYSTEM_112, "стенограмма разговора", "текст оператора"),
        Kind::Dds => (SYSTEM_DDS, "что знал диспетчер", "статусы и комментарии диспетчера"),
    };

    // Ключи по-русски: модель читает их как текст задания
    let body = json!({
        source_title: m.source.iter()
            .map(|(who, text)| json!({ "кто": who, "текст": text }))
            .collect::<Vec<_>>(),
        input_title: m.input.iter()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(field, text)| json!({ "поле": field, "текст": text }))
            .collect::<Vec<_>>(),
    });

    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": body.to_string() },
    ])
}

// ─── Проверка ответа ─────────────────────────────────────────────────

/// Приведение к виду, в котором ищется цитата: регистр, «ё», знаки
/// препинания и пробелы не должны решать, нашлась она или нет. Модель
/// охотно меняет «ё» на «е» и теряет запятые — это не выдумка.
pub(crate) fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = if c == 'ё' { 'е' } else { c };
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

pub(crate) fn found_in(quote: &str, haystack: &str) -> bool {
    let q = normalize(quote);
    q.chars().count() >= MIN_QUOTE_CHARS && haystack.contains(&q)
}

fn nonempty(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Разобрать ответ модели и оставить только подтверждённое.
///
/// Возвращает оставленные замечания и число отброшенных. Отброшенное
/// не прячется: счётчик показывается преподавателю — по нему видно,
/// насколько модели можно верить на этом вызове.
pub fn verify(kind: Kind, raw: &Value, m: &Material) -> Result<(Vec<TextFinding>, i32)> {
    // Модель отвечает по утверждениям (см. `schema`), проверка работает
    // с замечаниями. Готовый список замечаний тоже принимаем — так
    // устроены проверки и так проще подменить модель
    let converted;
    let raw = if raw.get("findings").is_none() && raw.get("statements").is_some() {
        converted = to_findings(kind, raw)?;
        &converted
    } else {
        raw
    };

    let items = raw
        .get("findings")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("в ответе модели нет списка findings"))?;

    let input = normalize(&m.input.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(" \n "));
    let source = normalize(&m.source.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(" \n "));

    let mut kept: Vec<TextFinding> = Vec::new();
    let mut dropped = 0;

    for item in items {
        let finding_kind = nonempty(item.get("kind")).unwrap_or_default();
        let text = nonempty(item.get("text"));
        let quote_input = nonempty(item.get("quoteInput"));
        let quote_source = nonempty(item.get("quoteSource"));

        // Какие цитаты замечанию нужны. Пропуск цитируется по разговору:
        // в тексте курсанта его по определению нет. Выдумка — по тексту
        // курсанта: в разговоре её по определению нет
        let (need_input, need_source) = match finding_kind.as_str() {
            "missing" => (false, true),
            "contradiction" | "inconsistent" => (true, true),
            "invented" | "no_reason" => (true, false),
            _ => {
                dropped += 1;
                continue;
            }
        };

        let ok = kind.allowed().contains(&finding_kind.as_str())
            && text.is_some()
            && (!need_input || quote_input.as_deref().is_some_and(|q| found_in(q, &input)))
            && (!need_source || quote_source.as_deref().is_some_and(|q| found_in(q, &source)));

        if !ok {
            dropped += 1;
            continue;
        }

        let f = TextFinding {
            kind: finding_kind,
            text: text.unwrap_or_default(),
            quote_input: if need_input { quote_input } else { None },
            quote_source: if need_source { quote_source } else { None },
        };

        // Одно и то же модель иногда повторяет разными словами — по
        // одинаковым цитатам это видно
        if kept.iter().any(|k| {
            k.kind == f.kind && k.quote_input == f.quote_input && k.quote_source == f.quote_source
        }) {
            continue;
        }
        kept.push(f);
    }

    // Пропуск с той же репликой, что и противоречие, — то же замечание
    // второй раз: сведение в тексте есть, только записано неверно. На
    // живой модели так и вышло — «баня вместо крыши» пришла дважды
    let contradicted: Vec<String> = kept
        .iter()
        .filter(|f| f.kind == "contradiction" || f.kind == "inconsistent")
        .filter_map(|f| f.quote_source.as_deref().map(normalize))
        .collect();
    kept.retain(|f| {
        f.kind != "missing"
            || !f.quote_source.as_deref().map(normalize).is_some_and(|q| contradicted.contains(&q))
    });

    kept.truncate(MAX_FINDINGS);
    Ok((kept, dropped))
}

// ─── Хранение и очередь ──────────────────────────────────────────────

/// Поставить анализ в очередь. Повторная постановка той же попытки
/// сбрасывает прежний результат вместе с вердиктами преподавателя:
/// замечания будут другими, и вердикт к старым номерам к ним не относится.
pub async fn enqueue(
    pool: &PgPool,
    kind: Kind,
    card_id: Uuid,
    service_id: Option<&str>,
    created_by: Option<Uuid>,
) -> Result<Uuid> {
    let mut tx = pool.begin().await?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO text_analyses (kind, card_id, service_id)
         VALUES ($1, $2, $3)
         ON CONFLICT (kind, card_id, service_id) DO UPDATE
            SET status = 'queued', findings = '[]'::jsonb, dropped = 0,
                note = NULL, verdicts = '{}'::jsonb, model_ms = NULL,
                engine = NULL, created_at = now(), finished_at = NULL
         RETURNING id",
    )
    .bind(kind.as_str())
    .bind(card_id)
    .bind(service_id)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("INSERT INTO ml_jobs (kind, input, created_by) VALUES ($1, $2, $3)")
        .bind(JOB_KIND)
        .bind(json!({ "analysisId": id }))
        .bind(created_by)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(id)
}

/// Собрать, что сравнивать, по строке анализа.
async fn material(pool: &PgPool, kind: Kind, card_id: Uuid, service_id: Option<&str>) -> Result<Material> {
    let (session_id, description, descriptive): (Option<Uuid>, String, Option<String>) =
        sqlx::query_as(
            "SELECT session_id, description, address ->> 'descriptive'
               FROM cards WHERE id = $1",
        )
        .bind(card_id)
        .fetch_one(pool)
        .await?;

    match kind {
        Kind::Card112 => {
            let session_id = session_id.ok_or_else(|| anyhow!("у карточки нет учебного вызова"))?;
            // Только заявитель: вопросы оператора — не источник фактов
            let lines: Vec<(String, String)> = sqlx::query_as(
                "SELECT who, text FROM transcript_lines
                  WHERE session_id = $1 ORDER BY at_sec, id",
            )
            .bind(session_id)
            .fetch_all(pool)
            .await?;
            Ok(Material {
                input: vec![
                    ("Описание со слов заявителя".into(), description),
                    ("Описательный адрес".into(), descriptive.unwrap_or_default()),
                ],
                source: lines.into_iter().filter(|(who, _)| who == "заявитель").collect(),
            })
        }
        Kind::Dds => {
            let service_id = service_id.ok_or_else(|| anyhow!("у анализа ДДС нет службы"))?;

            // Системные записи («Получена службой») комментариев не несут
            let entries: Vec<(String, Option<String>)> = sqlx::query_as(
                "SELECT status::text, comment FROM response_entries
                  WHERE card_id = $1 AND service_id = $2 AND user_id IS NOT NULL
                    AND status::text NOT IN ('Добавлена', 'Получена службой')
                  ORDER BY at, id",
            )
            .bind(card_id)
            .bind(service_id)
            .fetch_all(pool)
            .await?;

            // Доклады — только полученные: чего диспетчер не слышал,
            // того он и не мог учесть
            let reports: Vec<(String, String)> = sqlx::query_as(
                "SELECT r.stage::text, r.text FROM brigade_reports r
                   JOIN brigade_assignments a ON a.id = r.assignment_id
                  WHERE a.card_id = $1 AND a.service_id = $2 AND r.delivered_at IS NOT NULL
                  ORDER BY r.delivered_at",
            )
            .bind(card_id)
            .bind(service_id)
            .fetch_all(pool)
            .await?;

            let mut source = vec![("карточка 112".to_string(), description)];
            source.extend(reports.into_iter().map(|(stage, text)| (format!("доклад: {stage}"), text)));

            Ok(Material {
                input: entries
                    .into_iter()
                    .map(|(status, comment)| (status, comment.unwrap_or_default()))
                    .collect(),
                source,
            })
        }
    }
}

async fn store(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    findings: &[TextFinding],
    dropped: i32,
    note: Option<&str>,
    model_ms: Option<i32>,
) -> Result<()> {
    sqlx::query(
        "UPDATE text_analyses
            SET status = $2, findings = $3, dropped = $4, note = $5,
                model_ms = $6, finished_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(serde_json::to_value(findings)?)
    .bind(dropped)
    .bind(note)
    .bind(model_ms)
    .execute(pool)
    .await?;
    Ok(())
}

/// Модели нет — так и пишем, а не молчим и не выдумываем замечания.
pub async fn mark_unavailable(pool: &PgPool, id: Uuid, note: &str) -> Result<()> {
    store(pool, id, "unavailable", &[], 0, Some(note), None).await
}

/// Выполнить анализ. Ошибка модели — не ошибка задачи: она записывается
/// в строку анализа, иначе разбор навсегда остался бы «готовится».
pub async fn run(pool: &PgPool, llm: &dyn Llm, id: Uuid) -> Result<&'static str> {
    let (kind, card_id, service_id): (String, Uuid, Option<String>) = sqlx::query_as(
        "SELECT kind, card_id, service_id FROM text_analyses WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    let kind = Kind::parse(&kind).ok_or_else(|| anyhow!("неизвестный вид анализа {kind}"))?;

    let m = match material(pool, kind, card_id, service_id.as_deref()).await {
        Ok(m) => m,
        Err(e) => {
            store(pool, id, "failed", &[], 0, Some(&e.to_string()), None).await?;
            return Ok("failed");
        }
    };

    set_engine(pool, id, "llm").await?;

    // Нечего проверять — модель не зовём: пустой текст сам по себе
    // замечание правил («не заполнено»), а не нейросети
    if m.input_is_empty() {
        store(pool, id, "ready", &[], 0, Some("курсант не вводил текста"), None).await?;
        return Ok("ready");
    }

    let started = std::time::Instant::now();
    // По утверждению на мысль: десяток утверждений с цитатами — около
    // восьмисот токенов
    let answer = llm.chat_json(messages(kind, &m), 1000, Some(schema(kind))).await;
    let ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;

    match answer.and_then(|raw| verify(kind, &raw, &m)) {
        Ok((findings, dropped)) => {
            store(pool, id, "ready", &findings, dropped, None, Some(ms)).await?;
            Ok("ready")
        }
        Err(e) => {
            let note: String = format!("модель не дала разбора: {e}").chars().take(300).collect();
            store(pool, id, "failed", &[], 0, Some(&note), Some(ms)).await?;
            Ok("failed")
        }
    }
}

// ─── Сверка кросс-энкодером (профиль ЦП) ─────────────────────────────
//
// Генеративная модель на процессоре сверяет описание десятки секунд и
// не видит упущенного: на замере (backend/docs/замеры/ОПИСАНИЯ.md) из
// 26 фактов, о которых описание молчит, GigaChat 16 назвал
// подтверждёнными. Кросс-энкодер OpenJEV решает ту же задачу как NLI —
// описание посылкой, факт вызова гипотезой — за секунды и одинаково при
// повторе: 88–91 % на том же наборе.
//
// Сверяются только факты, которые курсант слышал: легенда и ответы на
// заданные вопросы. Чего он не спросил, того он и не мог записать — это
// «угадал» или «не выяснил» в разборе правил, а не пропуск в тексте.
//
// Кросс-энкодер находит упущенное и противоречия. Добавленное от себя
// (обратное направление: фраза описания против всего разговора) не
// проверено на замере, и в разбор его не пускаем — ложная тревога стоит
// курсанту дороже пропущенного замечания. Это остаётся языковой модели.

/// Чем разбирать текст — настройка `analysis.engine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// Кросс-энкодер, если подключён, иначе языковая модель
    Auto,
    Nli,
    Llm,
}

impl Engine {
    pub fn from_setting(v: f64) -> Self {
        match v.round() as i64 {
            1 => Self::Nli,
            2 => Self::Llm,
            _ => Self::Auto,
        }
    }
}

/// Что делать с конкретным анализом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Nli,
    Llm,
    /// Нечем — и почему
    Unavailable(&'static str),
}

/// Выбор движка. Комментарии диспетчера ДДС сверять не с чем — там нет
/// фактов вызова, там доклады и мотивировка отказа, — их всегда читает
/// языковая модель.
pub fn choose(engine: Engine, kind: Kind, nli_ready: bool, llm_ready: bool) -> Choice {
    let want_nli = kind == Kind::Card112
        && match engine {
            Engine::Nli => true,
            Engine::Auto => nli_ready,
            Engine::Llm => false,
        };
    if want_nli {
        return if nli_ready {
            Choice::Nli
        } else {
            Choice::Unavailable("кросс-энкодер не подключён (NLI_BASE_URL не задан или сервер недоступен)")
        };
    }
    if llm_ready {
        Choice::Llm
    } else {
        Choice::Unavailable("языковая модель не подключена (ML_PROVIDER=mock)")
    }
}

/// Вид анализа по его строке.
pub async fn kind_of(pool: &PgPool, id: Uuid) -> Result<Kind> {
    let kind: String = sqlx::query_scalar("SELECT kind FROM text_analyses WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Kind::parse(&kind).ok_or_else(|| anyhow!("неизвестный вид анализа {kind}"))
}

/// Факт, который курсант слышал, в виде для сверки.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardFact {
    /// Гипотеза для кросс-энкодера: утверждение, а если его ещё не
    /// подготовили — сырой ответ заявителя
    pub hypothesis: String,
    /// Слова заявителя — цитата для разбора
    pub source: String,
    /// Утверждение подготовлено; иначе сверка по сырому ответу
    pub prepared: bool,
}

/// Части описания для цитаты противоречия: предложения и части через
/// запятую. Цитата — часть, а не весь абзац, иначе преподавателю искать
/// глазами; и на целом предложении «Горит баня на участке, есть
/// пострадавший» кросс-энкодер не различает, какая часть спорит с каким
/// фактом.
pub fn sentences(text: &str) -> Vec<String> {
    text.split(['.', '!', '?', ';', ',', '\n'])
        .map(str::trim)
        .filter(|s| s.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_QUOTE_CHARS)
        .map(str::to_string)
        .collect()
}

/// Вердикты кросс-энкодера → замечания в том же виде, что у языковой
/// модели: дальше их проверяет тот же `verify`, и цитаты сверяются тем
/// же кодом.
///
/// `quotes` — для противоречий: предложение описания, которое сильнее
/// всего противоречит факту (индекс факта → предложение).
pub fn nli_findings(
    facts: &[HeardFact],
    verdicts: &[crate::ml::nli::Entailment],
    quotes: &std::collections::HashMap<usize, String>,
) -> Value {
    use crate::ml::nli::Verdict;

    // Замечания с одной и той же парой цитат склеиваются: несколько
    // сведений легенды цитируют одну и ту же первую фразу, и `verify`
    // оставил бы из них одно, потеряв остальные
    let mut contradictions: Vec<(Option<String>, String, Vec<String>)> = Vec::new();
    let mut missing: Vec<(String, Vec<String>)> = Vec::new();
    for (i, (fact, e)) in facts.iter().zip(verdicts).enumerate() {
        match e.verdict() {
            Verdict::Entailment => {}
            Verdict::Contradiction => {
                let quote = quotes.get(&i).cloned();
                match contradictions.iter_mut().find(|(q, s, _)| *q == quote && *s == fact.source) {
                    Some((_, _, list)) => list.push(fact.hypothesis.clone()),
                    None => contradictions.push((quote, fact.source.clone(), vec![fact.hypothesis.clone()])),
                }
            }
            Verdict::Neutral => match missing.iter_mut().find(|(s, _)| *s == fact.source) {
                Some((_, list)) => list.push(fact.hypothesis.clone()),
                None => missing.push((fact.source.clone(), vec![fact.hypothesis.clone()])),
            },
        }
    }

    // Пропуск с той же фразой заявителя, что и противоречие, `verify`
    // выбросит как повтор (у языковой модели так и есть). Здесь это
    // другое сведение той же фразы — дописываем его к противоречию
    let mut tails: Vec<(String, String)> = Vec::new();
    missing.retain(|(source, list)| {
        if contradictions.iter().any(|(_, s, _)| s == source) {
            tails.push((source.clone(), list.join(" ")));
            false
        } else {
            true
        }
    });

    // Противоречие важнее пропуска: при обрезке до MAX_FINDINGS
    // пропуски уходят первыми
    let mut out: Vec<Value> = contradictions
        .into_iter()
        .map(|(quote, source, list)| {
            let mut text = format!("В описании иначе, чем сказал заявитель: {}", list.join(" "));
            for (_, tail) in tails.iter().filter(|(s, _)| *s == source) {
                text.push_str(&format!(" Не записано: {tail}"));
            }
            json!({
                "kind": "contradiction",
                "text": text,
                "quoteInput": quote,
                "quoteSource": source,
            })
        })
        .collect();
    out.extend(missing.into_iter().map(|(source, list)| json!({
        "kind": "missing",
        "text": format!("Не записано: {}", list.join(" ")),
        "quoteInput": "",
        "quoteSource": source,
    })));
    json!({ "findings": out })
}

/// Посылка и факты по карточке 112.
///
/// Посылка — не только описание: адрес и ФИО заявителя курсант пишет
/// в свои поля карточки, и факт «это Королёв» должен находить подтверждение
/// в поле адреса, а не считаться упущенным в описании.
async fn nli_material(pool: &PgPool, card_id: Uuid) -> Result<(String, Vec<HeardFact>)> {
    let (session_id, description, address, applicant): (Option<Uuid>, String, Option<String>, String) =
        sqlx::query_as(
            "SELECT c.session_id, c.description,
                    (SELECT string_agg(value, ', ') FROM jsonb_each_text(c.address)
                      WHERE value <> ''),
                    c.applicant_name
               FROM cards c WHERE c.id = $1",
        )
        .bind(card_id)
        .fetch_one(pool)
        .await?;
    let session_id = session_id.ok_or_else(|| anyhow!("у карточки нет учебного вызова"))?;

    let mut premise = description.trim().to_string();
    if let Some(a) = address.filter(|a| !a.trim().is_empty()) {
        premise.push_str(&format!("\nАдрес: {a}."));
    }
    if !applicant.trim().is_empty() {
        premise.push_str(&format!("\nЗаявитель: {}.", applicant.trim()));
    }

    let (legend, legend_statements): (String, Option<Vec<String>>) = sqlx::query_as(
        "SELECT s.legend, s.legend_statements FROM sessions x
           JOIN scenarios s ON s.id = x.scenario_id WHERE x.id = $1",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;

    let mut facts: Vec<HeardFact> = match legend_statements.filter(|v| !v.is_empty()) {
        Some(list) => list
            .into_iter()
            .map(|s| HeardFact { hypothesis: s, source: legend.clone(), prepared: true })
            .collect(),
        None => vec![HeardFact { hypothesis: legend.clone(), source: legend.clone(), prepared: false }],
    };

    // Ответы на заданные вопросы — в том порядке, в каком курсант их
    // услышал
    let revealed: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT f.answer, f.statement FROM revealed_facts r
           JOIN scenario_hidden_facts f ON f.id = r.fact_id
          WHERE r.session_id = $1 ORDER BY r.at_sec, f.sort_order",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;
    for (answer, statement) in revealed {
        facts.push(match statement.filter(|s| !s.trim().is_empty()) {
            Some(s) => HeardFact { hypothesis: s, source: answer, prepared: true },
            None => HeardFact { hypothesis: answer.clone(), source: answer, prepared: false },
        });
    }
    Ok((premise, facts))
}

/// Разобрать описание 112 кросс-энкодером.
pub async fn run_nli(pool: &PgPool, nli: &dyn crate::ml::nli::Nli, id: Uuid) -> Result<&'static str> {
    use crate::ml::nli::Verdict;

    let card_id: Uuid = sqlx::query_scalar("SELECT card_id FROM text_analyses WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;

    let loaded = async {
        let m = material(pool, Kind::Card112, card_id, None).await?;
        let (premise, facts) = nli_material(pool, card_id).await?;
        anyhow::Ok((m, premise, facts))
    }
    .await;
    let (m, premise, facts) = match loaded {
        Ok(v) => v,
        Err(e) => {
            store(pool, id, "failed", &[], 0, Some(&e.to_string()), None).await?;
            return Ok("failed");
        }
    };
    set_engine(pool, id, "nli").await?;

    if m.input_is_empty() {
        store(pool, id, "ready", &[], 0, Some("курсант не вводил текста"), None).await?;
        return Ok("ready");
    }

    let started = std::time::Instant::now();
    let hypotheses: Vec<String> = facts.iter().map(|f| f.hypothesis.clone()).collect();
    let result = async {
        let verdicts = nli.classify(&premise, &hypotheses).await?;

        // Цитата противоречия — та часть описания, без которой противоречие
        // пропадает сильнее всего. Спрашивать части по одной нельзя: без
        // контекста кросс-энкодер считает противоречием крыше дома даже
        // «Дом газифицирован» (0,97). Поэтому убираем часть из описания и
        // смотрим, насколько упала вероятность. Проход на часть — их
        // обычно две-четыре
        let contradicted: Vec<usize> = verdicts
            .iter()
            .enumerate()
            .filter(|(_, e)| e.verdict() == Verdict::Contradiction)
            .map(|(i, _)| i)
            .collect();
        let mut quotes = std::collections::HashMap::new();
        if !contradicted.is_empty() {
            let text = m.input.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
            let parts = sentences(&text);
            let targets: Vec<String> = contradicted.iter().map(|&i| hypotheses[i].clone()).collect();
            if parts.len() == 1 {
                for &i in &contradicted {
                    quotes.insert(i, parts[0].clone());
                }
            } else if !parts.is_empty() {
                let base = nli.classify(&text, &targets).await?;
                let mut best: Vec<(f32, Option<String>)> = vec![(f32::MIN, None); targets.len()];
                for part in &parts {
                    let without = text.replacen(part.as_str(), "", 1);
                    let scores = nli.classify(&without, &targets).await?;
                    for (j, e) in scores.iter().enumerate() {
                        let drop = base[j].contradiction - e.contradiction;
                        if drop > best[j].0 {
                            best[j] = (drop, Some(part.clone()));
                        }
                    }
                }
                for (j, &i) in contradicted.iter().enumerate() {
                    if let Some(q) = best[j].1.clone() {
                        quotes.insert(i, q);
                    }
                }
            }
        }
        anyhow::Ok(nli_findings(&facts, &verdicts, &quotes))
    }
    .await;
    let ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;

    // Утверждения готовит модель при подготовке вызова. Где их ещё нет,
    // сверка шла по сырому ответу заявителя — так и пишем: на сыром
    // ответе точность заметно ниже
    let raw = facts.iter().filter(|f| !f.prepared).count();
    let note = (raw > 0).then(|| {
        format!(
            "{raw} из {} сведений сверялись по дословному ответу заявителя: утверждения к вызову \
             ещё не подготовлены, точность ниже",
            facts.len()
        )
    });

    match result.and_then(|raw| verify(Kind::Card112, &raw, &m)) {
        Ok((findings, dropped)) => {
            store(pool, id, "ready", &findings, dropped, note.as_deref(), Some(ms)).await?;
            Ok("ready")
        }
        Err(e) => {
            let note: String = format!("кросс-энкодер не дал разбора: {e}").chars().take(300).collect();
            store(pool, id, "failed", &[], 0, Some(&note), Some(ms)).await?;
            Ok("failed")
        }
    }
}

async fn set_engine(pool: &PgPool, id: Uuid, engine: &str) -> Result<()> {
    sqlx::query("UPDATE text_analyses SET engine = $2 WHERE id = $1")
        .bind(id)
        .bind(engine)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Чтение ──────────────────────────────────────────────────────────

/// Замечание вместе с решением преподавателя.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedFinding {
    #[serde(flatten)]
    pub finding: TextFinding,
    /// `confirmed`, `rejected` или нет — преподаватель ещё не смотрел
    pub verdict: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisView {
    pub id: Uuid,
    /// `queued`, `ready`, `unavailable`, `failed`
    pub status: String,
    pub findings: Vec<ReviewedFinding>,
    /// Сколько замечаний модели отброшено: цитата не нашлась
    pub dropped: i32,
    pub note: Option<String>,
    /// Сколько думала модель, миллисекунды
    pub model_ms: Option<i32>,
    /// Чем разобрано: `nli` — кросс-энкодер, `llm` — языковая модель
    pub engine: Option<String>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn view(
    pool: &PgPool,
    kind: Kind,
    card_id: Uuid,
    service_id: Option<&str>,
) -> Result<Option<AnalysisView>> {
    #[allow(clippy::type_complexity)]
    let row: Option<(Uuid, String, Value, i32, Option<String>, Value, Option<i32>, Option<chrono::DateTime<chrono::Utc>>, Option<String>)> =
        sqlx::query_as(
            "SELECT id, status, findings, dropped, note, verdicts, model_ms, finished_at, engine
               FROM text_analyses
              WHERE kind = $1 AND card_id = $2 AND service_id IS NOT DISTINCT FROM $3",
        )
        .bind(kind.as_str())
        .bind(card_id)
        .bind(service_id)
        .fetch_optional(pool)
        .await?;

    let Some((id, status, findings, dropped, note, verdicts, model_ms, finished_at, engine)) = row else {
        return Ok(None);
    };

    let findings: Vec<TextFinding> = serde_json::from_value(findings)?;
    let findings = findings
        .into_iter()
        .enumerate()
        .map(|(i, finding)| ReviewedFinding {
            verdict: verdicts
                .get(i.to_string())
                .and_then(|v| v.get("verdict"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            finding,
        })
        .collect();

    Ok(Some(AnalysisView { id, status, findings, dropped, note, model_ms, engine, finished_at }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn материал() -> Material {
        Material {
            input: vec![("Описание со слов заявителя".into(),
                         "Горит мусоропровод в подъезде, пострадавших нет".into())],
            source: vec![("заявитель".into(),
                          "Алло! У нас в подъезде мусоропровод горит, дым идёт на лестницу!".into())],
        }
    }

    #[test]
    fn цитата_ищется_без_учёта_регистра_ё_и_знаков() {
        let m = материал();
        let raw = json!({ "findings": [{
            "kind": "missing",
            "text": "Не указано, что дым идёт на лестницу.",
            "quoteInput": "",
            "quoteSource": "дым идет на лестницу",
        }]});
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert_eq!(dropped, 0);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].quote_input, None, "у пропуска цитаты из текста курсанта нет");
    }

    /// Главная защита: модель не может приписать курсанту слова,
    /// которых он не писал, или заявителю — которых тот не говорил.
    #[test]
    fn выдуманная_цитата_отбрасывается_и_считается() {
        let m = материал();
        let raw = json!({ "findings": [
            { "kind": "contradiction", "text": "Этаж не тот.",
              "quoteInput": "пятый этаж", "quoteSource": "мусоропровод горит" },
            { "kind": "invented", "text": "Пострадавших заявитель не упоминал.",
              "quoteInput": "пострадавших нет", "quoteSource": "" },
        ]});
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "invented");
    }

    #[test]
    fn вид_замечания_чужого_режима_не_принимается() {
        let m = материал();
        let raw = json!({ "findings": [
            { "kind": "no_reason", "text": "Нет причины.",
              "quoteInput": "Горит мусоропровод", "quoteSource": "" },
        ]});
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert!(kept.is_empty());
        assert_eq!(dropped, 1);
    }

    #[test]
    fn повтор_одного_замечания_схлопывается() {
        let m = материал();
        let one = json!({ "kind": "missing", "text": "Про дым.",
                          "quoteInput": "", "quoteSource": "дым идёт" });
        let raw = json!({ "findings": [one.clone(), one] });
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn слишком_короткая_цитата_ничего_не_подтверждает() {
        let m = материал();
        let raw = json!({ "findings": [
            { "kind": "missing", "text": "Что-то.", "quoteInput": "", "quoteSource": "у" },
        ]});
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert!(kept.is_empty());
        assert_eq!(dropped, 1);
    }

    #[test]
    fn ответ_без_списка_замечаний_это_ошибка() {
        assert!(verify(Kind::Card112, &json!({ "reply": "ок" }), &материал()).is_err());
    }

    #[test]
    fn схема_ограничивает_вердикты_режимом() {
        let s = schema(Kind::Dds);
        let v = &s["properties"]["statements"]["items"]["properties"]["verdict"]["enum"];
        assert_eq!(v, &json!(["понятно", "нет причины", "противоречит", "не сообщалось"]));
    }

    /// Модель судит утверждения, вид замечания выводит код.
    /// Подтверждённое замечанием не становится.
    #[test]
    fn вердикты_по_утверждениям_превращаются_в_замечания() {
        let m = Material {
            input: vec![("Описание".into(), "Горит баня, дом газифицирован, мусоропровод горит".into())],
            source: vec![("заявитель".into(), "У нас в подъезде мусоропровод горит, крыша дома горит".into())],
        };
        let raw = json!({
            "statements": [
                { "quoteInput": "Горит баня", "verdict": "противоречит",
                  "quoteSource": "крыша дома горит", "text": "Горит дом, а не баня." },
                { "quoteInput": "дом газифицирован", "verdict": "не сообщалось",
                  "quoteSource": "", "text": "Про газ не говорили." },
                { "quoteInput": "мусоропровод горит", "verdict": "подтверждается",
                  "quoteSource": "мусоропровод горит", "text": "Совпадает." },
            ],
            "missing": [{ "quoteSource": "в подъезде", "text": "Не записано, что в подъезде." }],
        });
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert_eq!(dropped, 0);
        let kinds: Vec<&str> = kept.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(kinds, ["contradiction", "invented", "missing"]);
    }

    #[test]
    fn пропуск_той_же_реплики_что_и_противоречие_не_повторяется() {
        let m = материал();
        let raw = json!({
            "statements": [{ "quoteInput": "Горит мусоропровод", "verdict": "противоречит",
                             "quoteSource": "мусоропровод горит, дым", "text": "Не то." }],
            "missing": [{ "quoteSource": "мусоропровод горит, дым", "text": "Не записано." }],
        });
        let (kept, _) = verify(Kind::Card112, &raw, &m).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "contradiction");
    }

    /// У ДДС пропусков не бывает: список пропусков модели игнорируется.
    #[test]
    fn у_ддс_пропуски_не_считаются() {
        let m = Material {
            input: vec![("Не принята".into(), "нет".into())],
            source: vec![("карточка 112".into(), "Горит крыша".into())],
        };
        let raw = json!({
            "statements": [{ "quoteInput": "нет", "verdict": "нет причины",
                             "quoteSource": "", "text": "Причина не названа." }],
            "missing": [{ "quoteSource": "Горит крыша", "text": "—" }],
        });
        let (kept, _) = verify(Kind::Dds, &raw, &m).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "no_reason");
    }

    // ─── Кросс-энкодер ───────────────────────────────────────────────

    /// «Авто» берёт кросс-энкодер, если он есть; ДДС — всегда языковая
    /// модель; нечем — так и написано, почему.
    #[test]
    fn выбор_движка_по_настройке_и_наличию() {
        use Choice::*;
        assert_eq!(choose(Engine::Auto, Kind::Card112, true, true), Nli);
        assert_eq!(choose(Engine::Auto, Kind::Card112, false, true), Llm);
        assert_eq!(choose(Engine::Llm, Kind::Card112, true, true), Llm);
        assert_eq!(choose(Engine::Auto, Kind::Dds, true, true), Llm, "у ДДС нечего сверять с фактами");
        assert!(matches!(choose(Engine::Nli, Kind::Card112, false, true), Unavailable(n) if n.contains("кросс-энкодер")));
        assert!(matches!(choose(Engine::Auto, Kind::Card112, false, false), Unavailable(n) if n.contains("mock")));
        assert_eq!(Engine::from_setting(1.0), Engine::Nli);
        assert_eq!(Engine::from_setting(2.0), Engine::Llm);
        assert_eq!(Engine::from_setting(7.0), Engine::Auto);
    }

    #[test]
    fn описание_режется_на_предложения_без_огрызков() {
        assert_eq!(
            sentences("Горит баня на участке, есть пострадавший!\nДа. —"),
            vec!["Горит баня на участке", "есть пострадавший"],
        );
    }

    fn факт(h: &str, src: &str) -> HeardFact {
        HeardFact { hypothesis: h.into(), source: src.into(), prepared: true }
    }

    fn вер(c: f32, e: f32, n: f32) -> crate::ml::nli::Entailment {
        crate::ml::nli::Entailment { contradiction: c, entailment: e, neutral: n }
    }

    /// Сведения одной фразы заявителя склеиваются в одно замечание, иначе
    /// `verify` оставил бы одно и потерял остальные; пропуск той же фразы,
    /// что и противоречие, дописывается к противоречию.
    #[test]
    fn вердикты_одной_фразы_склеиваются() {
        let легенда = "Горит крыша частного дома, Станционная, 28!";
        let facts = vec![
            факт("Горит крыша частного дома.", легенда),
            факт("Адрес: Станционная, 28.", легенда),
            факт("Пострадавших нет.", "Нет, пострадавших нет."),
            факт("Дом не газифицирован.", "Нет, дом не газифицирован."),
        ];
        let verdicts = vec![вер(0.9, 0.05, 0.05), вер(0.1, 0.1, 0.8), вер(0.1, 0.8, 0.1), вер(0.1, 0.1, 0.8)];
        let quotes = std::collections::HashMap::from([(0, "Горит баня".to_string())]);
        let raw = nli_findings(&facts, &verdicts, &quotes);
        let list = raw["findings"].as_array().unwrap();
        assert_eq!(list.len(), 2, "{raw}");
        assert_eq!(list[0]["kind"], "contradiction");
        assert!(list[0]["text"].as_str().unwrap().contains("Не записано: Адрес"), "{raw}");
        assert_eq!(list[1]["kind"], "missing");
        assert_eq!(list[1]["quoteSource"], "Нет, дом не газифицирован.");

        // И всё это проходит общую проверку цитат
        let m = Material {
            input: vec![("Описание со слов заявителя".into(), "Горит баня. Людей нет.".into())],
            source: vec![("заявитель".into(), легенда.into()),
                         ("заявитель".into(), "Нет, дом не газифицирован.".into())],
        };
        let (kept, dropped) = verify(Kind::Card112, &raw, &m).unwrap();
        assert_eq!((kept.len(), dropped), (2, 0));
    }
}
