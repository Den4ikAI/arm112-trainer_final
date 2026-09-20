//! Отчёт по занятию.
//!
//! ТЗ перечисляет, что в нём должно быть: действия обучающегося,
//! замечания, время заполнения карточки, отличие от норматива и
//! результат проверки грамматики. Здесь ровно эти пять блоков.
//!
//! Грамматика вынесена отдельно не случайно: в ТЗ она упомянута
//! трижды и стоит отдельным пунктом отчёта преподавателя.

use crate::{
    domain::grade::{self, Finding, Grade},
    ml::GrammarIssue,
    reports::csv,
    state::AppState,
};
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReport {
    pub session_id: Uuid,
    pub student: Student,
    pub scenario: ScenarioRef,
    pub card_number: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub timing: Timing,
    pub score: i32,
    /// Что курсант делал и когда
    pub actions: Vec<Action>,
    /// Замечания разбора
    pub findings: Vec<Finding>,
    pub services: grade::ServiceDiff,
    pub grammar: Grammar,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Student {
    pub id: Uuid,
    pub fio: String,
    pub login: String,
    pub service_title: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioRef {
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    pub legend: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    pub elapsed_sec: i32,
    pub norm_sec: i32,
    /// Со знаком: сколько секунд сверх норматива, отрицательное — запас
    pub diff_sec: i32,
    pub over_norm: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub at_sec: i32,
    /// `вопрос`, `ответ заявителя`, `сохранение`
    pub kind: String,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grammar {
    /// Какие поля проверялись — свободный текст, который уйдёт в службу
    pub checked_fields: Vec<String>,
    pub issues: Vec<GrammarField>,
    pub total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrammarField {
    pub field: String,
    pub label: String,
    pub text: String,
    pub issues: Vec<GrammarIssue>,
}

pub async fn build(state: &AppState, session_id: Uuid) -> Result<SessionReport> {
    let pool = &state.db;

    #[allow(clippy::type_complexity)]
    let row: Option<(
        Uuid, String, String, Option<String>,
        Option<i32>, Option<i32>, Option<String>,
        chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        // Норматив берём из оценки, а не отсюда: она зафиксировала тот,
        // что действовал в момент сохранения
        "SELECT u.id, u.fio, u.login, sv.title,
                sc.ticket, sc.call_no, sc.legend,
                s.started_at, s.finished_at
           FROM sessions s
           JOIN users u ON u.id = s.user_id
           LEFT JOIN services sv ON sv.id = u.service_id
           LEFT JOIN scenarios sc ON sc.id = s.scenario_id
          WHERE s.id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    let Some((
        user_id, fio, login, service_title,
        ticket, call_no, legend,
        started_at, finished_at,
    )) = row
    else {
        return Err(anyhow!("занятие не найдено"));
    };

    // Оценка считается один раз при сохранении и дальше только читается
    let Some(g): Option<Grade> = grade::load(pool, session_id).await? else {
        return Err(anyhow!("занятие ещё не сохранено, разбора нет"));
    };

    let (card_number, description, descriptive): (String, String, Option<String>) =
        sqlx::query_as(
            "SELECT number, description, address ->> 'descriptive'
               FROM cards WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow!("у занятия нет карточки"))?;

    let actions = actions(pool, session_id, &g).await?;
    let grammar = check_grammar(state, &description, descriptive.as_deref()).await?;

    Ok(SessionReport {
        session_id,
        student: Student { id: user_id, fio, login, service_title },
        scenario: ScenarioRef {
            ticket,
            call_no,
            legend: legend.unwrap_or_else(|| "свободная тренировка".into()),
        },
        card_number,
        started_at,
        finished_at,
        timing: Timing {
            elapsed_sec: g.elapsed_sec,
            norm_sec: g.norm_sec,
            diff_sec: g.elapsed_sec - g.norm_sec,
            over_norm: g.over_norm,
        },
        score: g.score,
        actions,
        findings: g.findings,
        services: g.services,
        grammar,
    })
}

/// Действия обучающегося — таймлайн занятия. Строится из стенограммы:
/// каждая реплика оператора это заданный вопрос, каждая реплика
/// заявителя — полученный ответ.
async fn actions(pool: &sqlx::PgPool, session_id: Uuid, g: &Grade) -> Result<Vec<Action>> {
    let lines: Vec<(String, String, i32)> = sqlx::query_as(
        "SELECT who, text, at_sec FROM transcript_lines
          WHERE session_id = $1 ORDER BY at_sec, id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    let mut actions: Vec<Action> = lines
        .into_iter()
        .map(|(who, text, at_sec)| Action {
            at_sec,
            kind: if who == "оператор" { "вопрос".into() } else { "ответ заявителя".into() },
            text,
        })
        .collect();

    actions.push(Action {
        at_sec: g.elapsed_sec,
        kind: "сохранение".into(),
        text: format!(
            "Карточка сохранена. Норматив {} с, затрачено {} с.",
            g.norm_sec, g.elapsed_sec
        ),
    });

    Ok(actions)
}

/// Проверяются поля, которые курсант пишет словами и которые уйдут
/// в службу как есть. Плашки признаков проверять нечего: там выбор
/// из готовых формулировок классификатора.
async fn check_grammar(
    state: &AppState,
    description: &str,
    descriptive: Option<&str>,
) -> Result<Grammar> {
    let fields: Vec<(&str, &str, &str)> = [
        ("description", "Описание со слов заявителя", description),
        ("address.descriptive", "Описательный адрес", descriptive.unwrap_or("")),
    ]
    .into_iter()
    .filter(|(_, _, text)| !text.trim().is_empty())
    .collect();

    let mut issues = Vec::new();
    let mut total = 0;

    for (field, label, text) in &fields {
        let found = state.ml.grammar.check(text).await?;
        total += found.len();
        issues.push(GrammarField {
            field: (*field).to_string(),
            label: (*label).to_string(),
            text: (*text).to_string(),
            issues: found,
        });
    }

    Ok(Grammar {
        checked_fields: fields.iter().map(|(_, label, _)| (*label).to_string()).collect(),
        issues,
        total,
    })
}

/// Та же информация в CSV: преподаватели сводят результаты в Excel.
pub fn to_csv(r: &SessionReport) -> String {
    let mut lines = Vec::new();

    let meta = |k: &str, v: String| csv::row([k, v.as_str()]);
    lines.push(meta("Отчёт по занятию", String::new()));
    lines.push(meta("Обучающийся", r.student.fio.clone()));
    lines.push(meta("Логин", r.student.login.clone()));
    lines.push(meta("Служба", r.student.service_title.clone().unwrap_or_default()));
    lines.push(meta(
        "Вызов",
        match (r.scenario.ticket, r.scenario.call_no) {
            (Some(t), Some(c)) => format!("билет {t}, вызов {c}"),
            _ => "свободная тренировка".into(),
        },
    ));
    lines.push(meta("Карточка", r.card_number.clone()));
    lines.push(meta("Начало", r.started_at.to_rfc3339()));
    lines.push(meta("Время заполнения, с", r.timing.elapsed_sec.to_string()));
    lines.push(meta("Норматив, с", r.timing.norm_sec.to_string()));
    lines.push(meta(
        "Отличие от норматива, с",
        // Знак показываем явно: «+18» читается быстрее, чем «18»
        format!("{:+}", r.timing.diff_sec),
    ));
    lines.push(meta("Балл", r.score.to_string()));
    lines.push(meta("Замечаний по грамматике", r.grammar.total.to_string()));
    lines.push(csv::row([""]));

    lines.push(csv::row(["Раздел", "Вид", "Поле", "Эталон", "Внесено", "Последствие"]));
    for f in &r.findings {
        lines.push(csv::row([
            "разбор",
            kind_label(&f.kind),
            f.label.as_str(),
            f.expected.as_deref().unwrap_or(""),
            f.actual.as_deref().unwrap_or(""),
            f.consequence.as_deref().unwrap_or(""),
        ]));
    }
    for field in &r.grammar.issues {
        for i in &field.issues {
            lines.push(csv::row([
                "грамматика",
                i.kind.as_str(),
                field.label.as_str(),
                i.suggestion.as_deref().unwrap_or(""),
                &field.text,
                i.message.as_str(),
            ]));
        }
    }
    lines.push(csv::row([""]));

    lines.push(csv::row(["Секунда", "Действие", "Текст"]));
    for a in &r.actions {
        lines.push(csv::row([a.at_sec.to_string().as_str(), a.kind.as_str(), a.text.as_str()]));
    }

    csv::document(lines)
}

fn kind_label(kind: &crate::domain::grade::FindingKind) -> &'static str {
    use crate::domain::grade::FindingKind as K;
    match kind {
        K::Ok => "верно",
        K::Guessed => "угадал",
        K::Wrong => "ошибка",
        K::Missing => "не заполнено",
        K::Extra => "лишнее",
    }
}

/// Для очереди: тот же отчёт как JSON.
pub async fn build_json(state: &AppState, session_id: Uuid) -> Result<Value> {
    Ok(serde_json::to_value(build(state, session_id).await?)?)
}
