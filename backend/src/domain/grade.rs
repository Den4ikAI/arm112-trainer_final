//! Движок оценки: сравнение трёх слоёв.
//!
//! ```text
//! слой 1  скрытая истина сценария   что было на самом деле
//! слой 2  стенограмма разговора     что курсант ВЫЯСНИЛ
//! слой 3  карточка                  что курсант ВНЁС
//! ```
//!
//! Поле, совпавшее с эталоном, но не подтверждённое разговором, —
//! это `guessed`. В боевых условиях угаданное значение ошибка, даже
//! когда оно верное: в следующий раз курсант угадает не так.
//!
//! Эталонный список оповещения НЕ хранится в сценарии, а считается
//! тем же конвейером, что и рабочий. Иначе при смене версии
//! классификатора сценарии начнут врать.

use crate::domain::classify::classify;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FindingKind {
    Ok,
    Guessed,
    Wrong,
    Missing,
    Extra,
}

impl FindingKind {
    /// Веса из BACKEND.md, раздел 7.
    fn weight(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Guessed | Self::Extra => 1,
            Self::Missing => 2,
            Self::Wrong => 3,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub kind: FindingKind,
    /// Путь поля: `address.settlement`, `answers.gasified`, `service.mchs`
    pub field: String,
    /// Подпись с экрана карточки, а не идентификатор
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Последствие ошибки вместо абстрактного балла
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consequence: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDiff {
    pub expected: Vec<String>,
    pub actual: Vec<String>,
    pub missed: Vec<String>,
    pub extra: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Grade {
    pub findings: Vec<Finding>,
    pub elapsed_sec: i32,
    pub norm_sec: i32,
    pub over_norm: bool,
    pub services: ServiceDiff,
    pub score: i32,
}

/// Подписи полей адреса и отметок. Взяты с кадров карточки.
fn field_label(field: &str) -> Option<&'static str> {
    Some(match field {
        "address.region" => "Субъект",
        "address.settlement" => "Населённый пункт",
        "address.street" => "Улица",
        "address.house" => "Дом/Вл.",
        "address.building" => "Корпус",
        "address.structure" => "Строение",
        "address.flat" => "Квартира",
        "address.entrance" => "Подъезд",
        "address.floor" => "Этаж",
        "address.code" => "Код",
        "address.district" => "Округ",
        "address.area" => "Район",
        "address.object" => "Объект",
        "address.descriptive" => "Описательный адрес",
        "address.raw" => "Единая адресная строка",
        "address.country" => "Страна",
        "victims" => "Пострадавшие",
        _ => return None,
    })
}

fn consequence(kind: FindingKind, field: &str) -> Option<String> {
    Some(match kind {
        FindingKind::Missing if field.starts_with("service.") =>
            "Служба не оповещена — фактически не выехала.".into(),
        FindingKind::Extra =>
            "Избыточное оповещение — служба отвлечена на непрофильный вызов.".into(),
        FindingKind::Missing =>
            "Поле не заполнено — служба получит неполную карточку.".into(),
        FindingKind::Wrong if field.starts_with("address.") =>
            "Силы и средства уедут не на тот адрес.".into(),
        FindingKind::Wrong =>
            "Признак искажён — список оповещения может быть неверным.".into(),
        FindingKind::Guessed =>
            "Значение верное, но заявитель его не сообщал — курсант не выяснил, а предположил.".into(),
        FindingKind::Ok => return None,
    })
}

/// «Ё» и регистр в ответах заявителя роли не играют: Королёв и
/// Королев — один город. Значения самого классификатора так
/// нормализовать нельзя (см. DESIGN-core.md §1.4), но здесь
/// сравнивается ввод человека.
fn norm(s: &str) -> String {
    s.trim().to_lowercase().replace('ё', "е")
}

/// Сказано ли значение в реплике заявителя.
///
/// Нужна для первой фразы: её заявитель говорит сам, без вопроса.
/// «Горит крыша частного дома» — и «Дом частный», «Крыша» в карточке
/// выяснены, а не угаданы, хотя в сценарии на них есть и отдельный
/// вопрос. Без этой проверки разбор ругал курсанта за то, что он
/// не переспросил очевидное.
///
/// Сравнение по основам слов: «частный» и «частного» — одно слово.
/// Значение из нескольких выбранных вариантов (через запятую) сказано,
/// только если сказан каждый. Вариант вида «Открытое пламя / Дым»
/// сказан, если сказана хоть одна его половина. «Да» и «Нет» сами
/// по себе ничего не доказывают: «нет» в реплике может относиться
/// к чему угодно, такие ответы выясняются только вопросом.
pub fn stated_in(utterance: &str, value: &str) -> bool {
    let words: Vec<String> = utterance
        .to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();

    // Служебные слова основой не служат: по ним совпадёт что угодно
    const EMPTY: [&str; 8] = ["да", "нет", "есть", "или", "без", "при", "для", "не"];

    let word_said = |w: &str| -> bool {
        if w.chars().all(|c| c.is_ascii_digit()) {
            return words.iter().any(|x| x == w);
        }
        let len = w.chars().count();
        let stem: String = w.chars().take(len.saturating_sub(2).max(4).min(len)).collect();
        words.iter().any(|x| x.starts_with(&stem))
    };

    let alternative_said = |alt: &str| -> bool {
        let significant: Vec<String> = alt
            .to_lowercase()
            .replace('ё', "е")
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty() && !EMPTY.contains(w))
            .filter(|w| w.chars().count() >= 3 || w.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
            .collect();
        !significant.is_empty() && significant.iter().all(|w| word_said(w))
    };

    let items: Vec<&str> = value.split(", ").filter(|i| !i.trim().is_empty()).collect();
    !items.is_empty() && items.iter().all(|item| item.split('/').any(alternative_said))
}

fn as_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(as_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

pub struct GradeInput<'a> {
    pub session_id: Uuid,
    pub card_id: Uuid,
    pub scenario_id: Uuid,
    pub elapsed_sec: i32,
    pub norm_sec: i32,
    pub version_id: Uuid,
    pub pool: &'a PgPool,
}

pub async fn grade(input: GradeInput<'_>) -> Result<Grade> {
    let pool = input.pool;

    let (expected, legend): (Value, String) =
        sqlx::query_as("SELECT expected, legend FROM scenarios WHERE id = $1")
            .bind(input.scenario_id)
            .fetch_one(pool)
            .await?;

    let (card_address, card_answers, card_victims, card_type_ids): (Value, Value, Option<i32>, Vec<String>) =
        sqlx::query_as(
            "SELECT address, answers, victims, type_ids FROM cards WHERE id = $1",
        )
        .bind(input.card_id)
        .fetch_one(pool)
        .await?;

    // Слой 2. Какие поля курсант действительно выяснил, а какие вообще
    // можно было выяснить только вопросом.
    //
    // Снятые правкой факты берём тоже: если курсант выяснил факт, а
    // преподаватель потом убрал вопрос, выясненное не должно задним
    // числом превратиться в угаданное.
    let facts: Vec<(Uuid, Vec<String>)> = sqlx::query_as(
        "SELECT id, unlocks FROM scenario_hidden_facts WHERE scenario_id = $1",
    )
    .bind(input.scenario_id)
    .fetch_all(pool)
    .await?;

    let revealed_ids: BTreeSet<Uuid> = sqlx::query_scalar(
        "SELECT fact_id FROM revealed_facts WHERE session_id = $1",
    )
    .bind(input.session_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    let mut askable: BTreeSet<String> = BTreeSet::new();
    let mut proven: BTreeSet<String> = BTreeSet::new();
    for (id, unlocks) in &facts {
        for f in unlocks {
            askable.insert(f.clone());
            if revealed_ids.contains(id) {
                proven.insert(f.clone());
            }
        }
    }

    // Подписи вопросов — те же, что курсант видел на экране
    let labels: BTreeMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT id, label FROM survey_questions",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    let mut findings: Vec<Finding> = Vec::new();

    let mut check = |field: String, expected_value: String, actual_value: String| {
        let label = field_label(&field)
            .map(str::to_string)
            .or_else(|| {
                field.strip_prefix("answers.").and_then(|q| labels.get(q).cloned())
            })
            // Технический ключ вместо подписи курсант видеть не должен:
            // «address.raw» в разборе выглядит поломкой, а не замечанием.
            // Совсем убрать запасной вариант нельзя — подписи вопросов
            // приходят из базы, — но о пропаже надо знать
            .unwrap_or_else(|| {
                tracing::warn!(field = %field, "в разборе поле без подписи");
                field.clone()
            });

        let kind = if actual_value.trim().is_empty() {
            FindingKind::Missing
        } else if norm(&actual_value) != norm(&expected_value) {
            FindingKind::Wrong
        } else if askable.contains(&field)
            && !proven.contains(&field)
            // Заявитель сказал это сам, первой фразой: вопрос был не нужен
            && !stated_in(&legend, &expected_value)
        {
            FindingKind::Guessed
        } else {
            FindingKind::Ok
        };

        findings.push(Finding {
            consequence: consequence(kind, &field),
            kind,
            expected: Some(expected_value),
            actual: if actual_value.is_empty() { None } else { Some(actual_value) },
            field,
            label,
        });
    };

    let empty = Map::new();

    // Адрес
    let addr_expected = expected.get("address").and_then(Value::as_object).unwrap_or(&empty);
    let addr_actual = card_address.as_object().unwrap_or(&empty);
    for (k, v) in addr_expected {
        let want = as_text(v);
        if want.is_empty() {
            continue;
        }
        let got = addr_actual.get(k).map(as_text).unwrap_or_default();
        check(format!("address.{k}"), want, got);
    }

    // Признаки опросной карты
    let ans_expected = expected.get("answers").and_then(Value::as_object).unwrap_or(&empty);
    let ans_actual = card_answers.as_object().unwrap_or(&empty);
    for (k, v) in ans_expected {
        let want = as_text(v);
        if want.is_empty() {
            continue;
        }
        let got = ans_actual.get(k).map(as_text).unwrap_or_default();
        check(format!("answers.{k}"), want, got);
    }

    // Пострадавшие. «Пострадавших нет» — это НЕ ноль в поле, а
    // ненажатая плашка: ввести туда 0 через интерфейс нельзя.
    // Поэтому пустой эталон и пустая карточка — просто совпадение,
    // а отметка при пустом эталоне — ошибка.
    let victims_expected = expected.get("victims").map(as_text).unwrap_or_default();
    let victims_actual = card_victims.map(|n| n.to_string()).unwrap_or_default();
    match (victims_expected.is_empty(), victims_actual.is_empty()) {
        (true, true) => {}
        (true, false) => {
            findings.push(Finding {
                kind: FindingKind::Wrong,
                field: "victims".into(),
                label: "Пострадавшие".into(),
                expected: None,
                actual: Some(victims_actual),
                consequence: Some(
                    "Заявитель не сообщал о пострадавших — скорая поедет впустую.".into(),
                ),
            });
        }
        _ => check("victims".into(), victims_expected, victims_actual),
    }

    // Тип происшествия
    let type_expected: Vec<String> = expected
        .get("typeIds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    for t in &type_expected {
        if !card_type_ids.contains(t) {
            findings.push(Finding {
                kind: FindingKind::Missing,
                field: "typeIds".into(),
                label: "Тип происшествия".into(),
                expected: Some(t.clone()),
                actual: None,
                consequence: Some("Без типа список оповещения не сформируется.".into()),
            });
        }
    }

    // Службы. Эталон считается движком по эталонным ответам — так он
    // остаётся верным и после смены версии классификатора.
    let expected_answers = answers_map(ans_expected);
    let reference = classify(pool, input.version_id, &type_expected, &expected_answers).await?;
    let expected_services: Vec<String> =
        reference.services.iter().map(|s| s.service_id.clone()).collect();

    let actual_services: Vec<String> = sqlx::query_scalar(
        "SELECT service_id FROM card_services WHERE card_id = $1 ORDER BY service_id",
    )
    .bind(input.card_id)
    .fetch_all(pool)
    .await?;

    let missed: Vec<String> = expected_services
        .iter()
        .filter(|s| !actual_services.contains(s))
        .cloned()
        .collect();
    let extra: Vec<String> = actual_services
        .iter()
        .filter(|s| !expected_services.contains(s))
        .cloned()
        .collect();

    // Информационные копии в card_services не попадают, штрафовать
    // за них не за что — см. OPEN-QUESTIONS.md, вопрос 5
    let titles: HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
        "SELECT id, title FROM services",
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();
    let title_of = |id: &str| titles.get(id).cloned().unwrap_or_else(|| id.to_string());

    for s in &missed {
        findings.push(Finding {
            kind: FindingKind::Missing,
            field: format!("service.{s}"),
            label: title_of(s),
            expected: Some(title_of(s)),
            actual: None,
            consequence: consequence(FindingKind::Missing, &format!("service.{s}")),
        });
    }
    for s in &extra {
        findings.push(Finding {
            kind: FindingKind::Extra,
            field: format!("service.{s}"),
            label: title_of(s),
            expected: None,
            actual: Some(title_of(s)),
            consequence: consequence(FindingKind::Extra, &format!("service.{s}")),
        });
    }

    let over_norm = input.elapsed_sec > input.norm_sec;
    let penalty: i32 =
        findings.iter().map(|f| f.kind.weight()).sum::<i32>() + if over_norm { 2 } else { 0 };
    // +2 — сам норматив и тип происшествия: чтобы одна ошибка
    // в короткой карточке не обнуляла оценку
    let total = (findings.len() as i32 + 2).max(1);
    let score = (100.0 * (1.0 - penalty as f64 / (total * 3) as f64)).round() as i32;
    let score = score.clamp(0, 100);

    Ok(Grade {
        findings,
        elapsed_sec: input.elapsed_sec,
        norm_sec: input.norm_sec,
        over_norm,
        services: ServiceDiff {
            expected: expected_services,
            actual: actual_services,
            missed,
            extra,
        },
        score,
    })
}

fn answers_map(obj: &Map<String, Value>) -> HashMap<String, Vec<String>> {
    obj.iter()
        .map(|(k, v)| {
            let values = match v {
                Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                Value::String(s) => vec![s.clone()],
                Value::Null => vec![],
                other => vec![other.to_string()],
            };
            (k.clone(), values)
        })
        .collect()
}

/// Оценка считается один раз, при сохранении карточки, и дальше
/// только читается: пересчёт после смены версии классификатора
/// задним числом менял бы уже выставленные отметки.
pub async fn store(pool: &PgPool, session_id: Uuid, g: &Grade) -> Result<()> {
    sqlx::query(
        "INSERT INTO grades (session_id, score, elapsed_sec, norm_sec, over_norm, findings, services)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (session_id) DO NOTHING",
    )
    .bind(session_id)
    .bind(g.score)
    .bind(g.elapsed_sec)
    .bind(g.norm_sec)
    .bind(g.over_norm)
    .bind(serde_json::to_value(&g.findings)?)
    .bind(serde_json::to_value(&g.services)?)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load(pool: &PgPool, session_id: Uuid) -> Result<Option<Grade>> {
    let row: Option<(i32, i32, i32, bool, Value, Value)> = sqlx::query_as(
        "SELECT score, elapsed_sec, norm_sec, over_norm, findings, services
           FROM grades WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    let Some((score, elapsed_sec, norm_sec, over_norm, findings, services)) = row else {
        return Ok(None);
    };

    Ok(Some(Grade {
        findings: serde_json::from_value(findings)?,
        elapsed_sec,
        norm_sec,
        over_norm,
        services: serde_json::from_value(services)?,
        score,
    }))
}

#[cfg(test)]
mod tests {
    use super::{field_label, stated_in};

    /// Поля адреса из карточки 112. Список снят с виджета адреса:
    /// ровно то, что оператор может заполнить.
    const ПОЛЯ_АДРЕСА: [&str; 16] = [
        "area", "building", "code", "country", "descriptive", "district",
        "entrance", "flat", "floor", "house", "object", "raw", "region",
        "settlement", "street", "structure",
    ];

    /// У каждого поля карточки есть человеческая подпись.
    ///
    /// Без неё в разбор попадает технический ключ: курсант читал
    /// «address.raw — поле не заполнено» и понимал это как поломку,
    /// а не как замечание.
    const ЛЕГЕНДА: &str =
        "Алло! Горит крыша частного дома! Пожалуйста, приезжайте скорее, это на Станционной улице, дом 28!";

    /// Заявитель сам сказал «горит крыша частного дома» — вписать
    /// «Дом частный» и «Крыша» без вопроса значит выяснить, а не угадать.
    #[test]
    fn сказанное_первой_фразой_не_угадано() {
        assert!(stated_in(ЛЕГЕНДА, "Дом частный"));
        assert!(stated_in(ЛЕГЕНДА, "Крыша"));
        assert!(stated_in(ЛЕГЕНДА, "28"));
    }

    #[test]
    fn не_сказанное_первой_фразой_остаётся_угаданным() {
        assert!(!stated_in(ЛЕГЕНДА, "Дом многоквартирный"));
        assert!(!stated_in(ЛЕГЕНДА, "Королёв"));
        assert!(!stated_in(ЛЕГЕНДА, "17"));
        assert!(!stated_in(ЛЕГЕНДА, "2"));
    }

    /// «Нет» в реплике может относиться к чему угодно.
    #[test]
    fn да_и_нет_выясняются_только_вопросом() {
        assert!(!stated_in("Нет, пожара нет, только дым", "Нет"));
        assert!(!stated_in("Да, да, приезжайте", "Да"));
    }

    #[test]
    fn вариант_через_косую_черту_сказан_по_одной_половине() {
        let легенда = "Здравствуйте, у нас в доме мусоропровод задымил, дым в подъезде идёт.";
        assert!(stated_in(легенда, "Открытое пламя / Дым"));
        assert!(stated_in(легенда, "Мусоропровод"));
    }

    /// Несколько выбранных вариантов сказаны, только если сказан каждый.
    #[test]
    fn несколько_вариантов_сказаны_только_все_вместе() {
        assert!(stated_in(ЛЕГЕНДА, "Крыша, Дом частный"));
        assert!(!stated_in(ЛЕГЕНДА, "Крыша, Подвал"));
    }

    #[test]
    fn у_каждого_поля_адреса_есть_подпись() {
        for поле in ПОЛЯ_АДРЕСА {
            let ключ = format!("address.{поле}");
            assert!(
                field_label(&ключ).is_some(),
                "нет подписи для {ключ} — в разборе появится технический ключ"
            );
        }
    }
}
