//! Утверждения фактов вызова для сверки описания кросс-энкодером.
//!
//! Кросс-энкодер (ml/nli.rs) сравнивает описание курсанта с фактом и
//! отвечает «подтверждает / противоречит / молчит». Факт вызова хранится
//! ответом заявителя — «Нет, дом не газифицирован.», «Нет, что вы, это
//! Королёв…». Такой ответ модель читает плохо: «Нет,» в начале она
//! принимает за отрицание, а легенда несёт в одной фразе несколько
//! сведений. На замере (backend/docs/замеры/ОПИСАНИЯ.md) точность на
//! сырых ответах 67–73 %, на утверждениях — 88–91 %.
//!
//! Поэтому при подготовке вызова модель переписывает каждый ответ одним
//! утверждением («Дом не газифицирован»), а легенду — утверждением на
//! сведение. Это офлайн-работа той же очереди, что и подготовка вызова;
//! преподаватель видит утверждения рядом с ответами и правит их.

use crate::ml::Llm;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

/// Вид задачи в очереди `ml_jobs`.
pub const JOB_KIND: &str = "scenario.statements";

/// Длиннее этого утверждение — уже пересказ, а не одно сведение.
const MAX_STATEMENT_CHARS: usize = 200;
/// Сведений в первой фразе заявителя больше не бывает.
const MAX_LEGEND_STATEMENTS: usize = 6;

const SYSTEM: &str = "\
Ты готовишь учебный вызов тренажёра операторов Системы-112 к проверке \
описания, которое оператор запишет со слов заявителя.

Тебе дают первую фразу заявителя (legend) и ответы заявителя на \
вопросы оператора (facts: key, question, answer). Перепиши их \
утверждениями о происшествии. Каждое утверждение — одно главное \
сведение, коротко, до десяти слов, грамотным русским языком. Без \
обращения, без слова «заявитель», без «да», «нет», «что вы» в начале. \
Ничего не добавляй: чего нет в ответе, того нет и в утверждении. \
Второстепенные подробности отбрасывай.

legend — список утверждений по первой фразе, по одному на сведение: \
что случилось и что именно горит или повреждено, есть ли люди в \
опасности. Адрес в legend не пиши — он записывается в отдельные поля.
facts — по одному утверждению на каждый ключ key. Вопрос и ответ \
читай вместе: ответ «нет» на вопрос «Есть ли X?» — это «X нет».

Примеры (не из этого вызова):
первая фраза «Алло! Срочно, у соседей из окна дым валит, приезжайте, \
Ленина, 5!» → legend: [«Из окна квартиры идёт дым.»] — просьбы, \
обращения и адрес в утверждения не попадают.
вопрос «Лифт работает?», ответ «Нет, лифт не работает.» → «Лифт не работает.»
вопрос «Есть пострадавшие?», ответ «Да, двое, их уже вынесли.» → \
«Есть двое пострадавших.»
вопрос «Чем-нибудь пахнет?», ответ «Нет, ничем.» → «Посторонних \
запахов нет.»
вопрос «Когда это началось?», ответ «Минут десять назад, наверное.» → \
«Началось около десяти минут назад.»
вопрос «Какой город?», ответ «Нет, это Мытищи! Улица Мира, 5.» → \
«Происшествие в городе Мытищи.»";

/// Схема ответа: легенда списком, факты — по ключам вызова. Ключи
/// перечислены явно, чтобы модель не выдумала своих.
pub fn schema(keys: &[String]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "legend": {
                "type": "array",
                "maxItems": MAX_LEGEND_STATEMENTS,
                "items": { "type": "string" },
            },
            "facts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "enum": keys },
                        "statement": { "type": "string" },
                    },
                    "required": ["key", "statement"],
                }
            }
        },
        "required": ["legend", "facts"],
    })
}

/// Факт вызова, как он лежит в базе.
#[derive(Debug, Clone)]
pub struct Fact {
    pub key: String,
    pub question: String,
    pub answer: String,
}

pub fn messages(legend: &str, facts: &[Fact]) -> Value {
    let body = json!({
        "legend": legend,
        "facts": facts.iter()
            .map(|f| json!({ "key": f.key, "question": f.question, "answer": f.answer }))
            .collect::<Vec<_>>(),
    });
    json!([
        { "role": "system", "content": SYSTEM },
        { "role": "user", "content": body.to_string() },
    ])
}

/// Утверждение, пригодное для сверки: непустое, одно предложение-два,
/// без лишних кавычек, которыми модель любит обрамлять ответ.
pub fn clean(raw: &str) -> Option<String> {
    let mut s = raw.trim().trim_matches(|c| c == '«' || c == '»' || c == '"').trim();

    // «Нет, дом не газифицирован.» — модель оставляет начало ответа, как
    // её ни проси. Для кросс-энкодера это худшее, что может быть: «Нет,»
    // он читает как отрицание всего утверждения (на замере отсюда почти
    // все ложные «противоречит»). Смысл в остатке фразы уже есть
    loop {
        let lower = s.to_lowercase();
        let Some(word) = ["нет", "да", "ну", "что вы", "алло"]
            .into_iter()
            .find(|w| lower.starts_with(w) && lower[w.len()..].starts_with([',', '!', '.', ' ']))
        else {
            break;
        };
        let rest = s[word.len()..].trim_start_matches([',', '!', '.', ' ']);
        if rest.chars().filter(|c| c.is_alphabetic()).count() < 3 {
            break; // «Нет.» — сведение в самом «нет», резать нечего
        }
        s = rest;
    }

    // Утверждение — повествовательное предложение. Восклицательный знак
    // от заявителя («Горит крыша частного дома!») кросс-энкодер понимает
    // хуже точки: на «бане вместо крыши» вероятность противоречия
    // 0,34 против 0,68 у той же фразы с точкой
    let s = s.trim_end_matches(['!', '?', '.', ' ']);
    if s.is_empty() || s.chars().count() > MAX_STATEMENT_CHARS {
        return None;
    }
    let mut chars = s.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).chain(std::iter::once('.')).collect())
}

/// Что вышло из ответа модели.
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub legend: Vec<String>,
    pub facts: HashMap<String, String>,
}

/// Разобрать ответ. Ключи, которых нет у вызова, отбрасываются: схема
/// их не пустит, но проверять за движком дёшево.
pub fn parse(raw: &Value, keys: &[String]) -> Result<Parsed> {
    let legend = raw
        .get("legend")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("в ответе модели нет legend"))?
        .iter()
        .filter_map(|v| v.as_str().and_then(clean))
        .take(MAX_LEGEND_STATEMENTS)
        .collect();

    let mut facts = HashMap::new();
    for f in raw.get("facts").and_then(|v| v.as_array()).into_iter().flatten() {
        let (Some(key), Some(statement)) = (
            f.get("key").and_then(|v| v.as_str()),
            f.get("statement").and_then(|v| v.as_str()).and_then(clean),
        ) else {
            continue;
        };
        if keys.iter().any(|k| k == key) {
            facts.entry(key.to_string()).or_insert(statement);
        }
    }
    Ok(Parsed { legend, facts })
}

// ─── Очередь ─────────────────────────────────────────────────────────

/// Поставить подготовку утверждений в очередь. Уже стоящую в очереди
/// задачу по тому же вызову второй раз не ставим: правки черновика идут
/// подряд, и модель всё равно прочтёт последнее состояние.
pub async fn enqueue(pool: &PgPool, scenario_id: Uuid, created_by: Option<Uuid>) -> Result<()> {
    enqueue_with(pool, scenario_id, created_by).await
}

/// То же внутри чужой транзакции: вызов и задача на его утверждения
/// появляются вместе или не появляются вовсе.
pub async fn enqueue_in(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, scenario_id: Uuid) -> Result<()> {
    enqueue_with(&mut **tx, scenario_id, None).await
}

async fn enqueue_with<'e, E: sqlx::PgExecutor<'e>>(
    ex: E,
    scenario_id: Uuid,
    created_by: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO ml_jobs (kind, input, created_by)
         SELECT $1, jsonb_build_object('scenarioId', $2::text), $3
          WHERE NOT EXISTS (
                SELECT 1 FROM ml_jobs
                 WHERE kind = $1 AND status = 'queued'
                   AND input ->> 'scenarioId' = $2::text)",
    )
    .bind(JOB_KIND)
    .bind(scenario_id)
    .bind(created_by)
    .execute(ex)
    .await?;
    Ok(())
}

/// Дозаполнить утверждения у вызовов, где их нет: у заведённых до этой
/// возможности и у тех, чей факт поменяли правкой. Возвращает, сколько
/// вызовов поставлено в очередь.
pub async fn backfill(pool: &PgPool) -> Result<usize> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT s.id FROM scenarios s
          WHERE s.status <> 'archived'
            AND (s.legend_statements IS NULL
                 OR EXISTS (SELECT 1 FROM scenario_hidden_facts f
                             WHERE f.scenario_id = s.id AND NOT f.retired
                               AND f.statement IS NULL))",
    )
    .fetch_all(pool)
    .await?;
    for id in &ids {
        enqueue(pool, *id, None).await?;
    }
    Ok(ids.len())
}

async fn load(pool: &PgPool, scenario_id: Uuid) -> Result<(String, Vec<Fact>)> {
    let legend: String = sqlx::query_scalar("SELECT legend FROM scenarios WHERE id = $1")
        .bind(scenario_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow!("вызов не найден"))?;
    let facts = sqlx::query_as::<_, (String, String, String)>(
        "SELECT key, question, answer FROM scenario_hidden_facts
          WHERE scenario_id = $1 AND NOT retired ORDER BY sort_order",
    )
    .bind(scenario_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(key, question, answer)| Fact { key, question, answer })
    .collect();
    Ok((legend, facts))
}

/// Подготовить утверждения моделью и записать.
///
/// Пишем только туда, где за время работы модели ничего не поменялось:
/// если преподаватель успел поправить ответ, утверждение к старому
/// ответу ему не нужно — следующая задача подготовит новое.
pub async fn run(pool: &PgPool, llm: &dyn Llm, scenario_id: Uuid) -> Result<Value> {
    let (legend, facts) = load(pool, scenario_id).await?;
    let keys: Vec<String> = facts.iter().map(|f| f.key.clone()).collect();

    let raw = llm
        .chat_json(messages(&legend, &facts), 800, Some(schema(&keys)))
        .await?;
    let parsed = parse(&raw, &keys)?;

    let mut facts_written = 0;
    for f in &facts {
        let Some(statement) = parsed.facts.get(&f.key) else { continue };
        facts_written += sqlx::query(
            "UPDATE scenario_hidden_facts SET statement = $4
              WHERE scenario_id = $1 AND key = $2 AND answer = $3 AND NOT retired",
        )
        .bind(scenario_id)
        .bind(&f.key)
        .bind(&f.answer)
        .bind(statement)
        .execute(pool)
        .await?
        .rows_affected();
    }

    let legend_written = if parsed.legend.is_empty() {
        0
    } else {
        sqlx::query("UPDATE scenarios SET legend_statements = $3 WHERE id = $1 AND legend = $2")
            .bind(scenario_id)
            .bind(&legend)
            .bind(&parsed.legend)
            .execute(pool)
            .await?
            .rows_affected()
    };

    Ok(json!({
        "scenarioId": scenario_id,
        "facts": facts_written,
        "legend": if legend_written > 0 { parsed.legend.len() } else { 0 },
    }))
}

/// Правка преподавателя. Пустая строка — «подготовить заново»: поле
/// сбрасывается, и очередь заполнит его моделью.
pub async fn save_manual(
    pool: &PgPool,
    scenario_id: Uuid,
    legend: Option<&[String]>,
    facts: &HashMap<String, String>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    if let Some(legend) = legend {
        let cleaned: Vec<String> = legend.iter().filter_map(|s| clean(s)).collect();
        sqlx::query("UPDATE scenarios SET legend_statements = $2 WHERE id = $1")
            .bind(scenario_id)
            .bind(if cleaned.is_empty() { None } else { Some(cleaned) })
            .execute(&mut *tx)
            .await?;
    }
    for (key, statement) in facts {
        let updated = sqlx::query(
            "UPDATE scenario_hidden_facts SET statement = $3
              WHERE scenario_id = $1 AND key = $2 AND NOT retired",
        )
        .bind(scenario_id)
        .bind(key)
        .bind(clean(statement))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if updated == 0 {
            anyhow::bail!("у вызова нет факта «{key}»");
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ключи() -> Vec<String> {
        vec!["gas".into(), "victims".into()]
    }

    #[test]
    fn ответ_модели_разбирается_по_ключам_вызова() {
        let raw = json!({
            "legend": ["Горит крыша частного дома.", "Адрес: Станционная, 28."],
            "facts": [
                { "key": "gas", "statement": "«Дом не газифицирован.»" },
                { "key": "victims", "statement": "Пострадавших нет, все вышли." },
                { "key": "выдуманный", "statement": "Что-то ещё." },
            ]
        });
        let p = parse(&raw, &ключи()).unwrap();
        assert_eq!(p.legend.len(), 2);
        assert_eq!(p.facts["gas"], "Дом не газифицирован.", "кавычки снимаются");
        assert_eq!(p.facts.len(), 2, "чужой ключ отброшен");
    }

    #[test]
    fn пустое_и_слишком_длинное_утверждение_не_принимается() {
        assert_eq!(clean("   "), None);
        assert_eq!(clean(&"а".repeat(MAX_STATEMENT_CHARS + 1)), None);
        assert_eq!(clean(" пламя видно ").as_deref(), Some("Пламя видно."));
    }

    /// «Нет,» в начале кросс-энкодер читает как отрицание всей фразы.
    #[test]
    fn начало_ответа_да_нет_отрезается() {
        assert_eq!(clean("Нет, дом не газифицирован.").as_deref(), Some("Дом не газифицирован."));
        assert_eq!(clean("Нет, что вы, это Королёв!").as_deref(), Some("Это Королёв."));
        assert_eq!(clean("Нетипичный запах").as_deref(), Some("Нетипичный запах."), "слово целиком, не приставка");
        assert_eq!(clean("Нет.").as_deref(), Some("Нет."), "одно «нет» — само сведение");
        assert_eq!(clean("Горит крыша частного дома!").as_deref(), Some("Горит крыша частного дома."));
    }

    #[test]
    fn схема_перечисляет_только_ключи_вызова() {
        let s = schema(&ключи());
        assert_eq!(s["properties"]["facts"]["items"]["properties"]["key"]["enum"], json!(["gas", "victims"]));
    }

    #[test]
    fn ответ_без_легенды_это_ошибка() {
        assert!(parse(&json!({ "facts": [] }), &ключи()).is_err());
    }
}
