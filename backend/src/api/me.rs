//! Разделы курсанта: выданное ему преподавателем.
//!
//! Денис, 29.09: у оператора 112 — «Учебные вызовы», «Экзамены» и
//! «Тренировка по частям» (блиц); у диспетчера ДДС — «Учебные
//! происшествия», «Экзамены», «Тренировка по частям». В каждой строке —
//! дата, тема, статус с цветом (не начато / выполняется / закончено) и
//! балл; закончённое открывается только на просмотр разбора.
//!
//! Материал раздаёт только преподаватель — занятием. Что роздано, то и
//! показывается, в том числе из уже завершённых занятий: курсант должен
//! видеть свои прошлые ошибки.

use crate::{
    api::auth::{AuthUser, Duty, Role},
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/me/work", get(work))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct WorkQuery {
    /// `training` — учебные вызовы или происшествия, `exam` — экзамены
    /// (марафон), `blitz` — тренировка по частям карточки
    pub section: String,
}

/// Одна строка раздела.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    /// Задание (оператор 112) или выданное происшествие (ДДС)
    pub id: Uuid,
    pub lesson_id: Uuid,
    pub lesson_title: String,
    /// `training`, `marathon` или `blitz`
    pub kind: String,
    /// Сегмент тренировки по частям
    pub segment: Option<String>,
    /// Идёт ли занятие: начать можно только в идущем
    pub lesson_running: bool,
    /// Когда выдано: начало занятия или выпуск происшествия потоком
    pub issued_at: Option<DateTime<Utc>>,
    /// Тема — тип происшествия, а если его нет — начало легенды. В
    /// экзамене и блице по типу у оператора 112 — «Учебный вызов»:
    /// тип там и есть ответ
    pub topic: String,
    /// Подробнее: первая фраза заявителя или адрес происшествия
    pub detail: String,
    /// `not_started`, `in_progress`, `done` или `missed` (занятие
    /// закончилось, а задание не начато)
    pub status: String,
    /// Балл. Нет — ещё не оценено или скрыто до конца экзамена
    pub score: Option<i32>,
    /// Балл скрыт: экзамен идёт, разбор — после последнего задания
    pub hidden: bool,
    /// Порядок в экзамене
    pub seq: Option<i32>,
    /// Оператор 112: вызов, попытка и карточка
    pub scenario_id: Option<Uuid>,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    pub session_id: Option<Uuid>,
    pub card_number: Option<String>,
    /// Диспетчер ДДС: карточка происшествия
    pub card_id: Option<Uuid>,
}

const ВИДЫ: [(&str, &str); 3] = [("training", "training"), ("exam", "marathon"), ("blitz", "blitz")];

#[utoipa::path(
    get,
    path = "/api/me/work",
    tag = "Занятия",
    params(WorkQuery),
    responses(
        (status = 200, description = "Выданное курсанту в разделе", body = Vec<WorkItem>),
        (status = 400, description = "Неизвестный раздел"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Только для курсанта"),
    ),
)]
pub async fn work(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<WorkQuery>,
) -> AppResult<Json<Vec<WorkItem>>> {
    if user.role != Role::Student {
        return Err(AppError::Forbidden);
    }
    let Some((_, kind)) = ВИДЫ.iter().find(|(s, _)| *s == q.section) else {
        return Err(AppError::Validation(
            "раздел — «training», «exam» или «blitz»".into(),
        ));
    };

    let items = match user.duty {
        Some(Duty::Dds) => dds(&state, &user, kind).await?,
        _ => op112(&state, &user, kind).await?,
    };
    Ok(Json(items))
}

#[derive(sqlx::FromRow)]
struct Строка112 {
    id: Uuid,
    lesson_id: Uuid,
    title: String,
    kind: String,
    segment: Option<String>,
    running: bool,
    started: Option<DateTime<Utc>>,
    seq: Option<i32>,
    sid: Uuid,
    ticket: Option<i32>,
    call_no: Option<i32>,
    legend: String,
    type_title: Option<String>,
    session_id: Option<Uuid>,
    finished: Option<DateTime<Utc>>,
    score: Option<i32>,
    number: Option<String>,
    hidden: bool,
}

async fn op112(state: &AppState, user: &AuthUser, kind: &str) -> AppResult<Vec<WorkItem>> {
    let rows: Vec<Строка112> = sqlx::query_as(
        "SELECT a.id, l.id AS lesson_id, l.title, l.kind::text AS kind, l.segment,
                l.finished_at IS NULL AS running, l.started_at AS started, a.seq,
                s.id AS sid, s.ticket, s.call_no, s.legend,
                (SELECT t.title FROM incident_types t
                  WHERE t.id = s.expected->'typeIds'->>0) AS type_title,
                ses.id AS session_id, ses.finished_at AS finished, g.score, c.number,
                -- Экзамен: пока у курсанта есть незаконченные вызовы
                -- идущего марафона, баллы не показываются
                (l.kind = 'marathon' AND l.finished_at IS NULL AND EXISTS (
                    SELECT 1 FROM assignments a2
                     WHERE a2.lesson_id = l.id AND a2.user_id = a.user_id
                       AND NOT EXISTS (SELECT 1 FROM sessions s2
                                        WHERE s2.assignment_id = a2.id
                                          AND s2.finished_at IS NOT NULL))) AS hidden
           FROM assignments a
           JOIN lessons l ON l.id = a.lesson_id
           JOIN scenarios s ON s.id = a.scenario_id
           LEFT JOIN LATERAL (
                 SELECT id, finished_at FROM sessions
                  WHERE assignment_id = a.id ORDER BY started_at DESC LIMIT 1) ses ON TRUE
           LEFT JOIN grades g ON g.session_id = ses.id
           LEFT JOIN cards c ON c.session_id = ses.id
          WHERE a.user_id = $1 AND l.mode = 'card112'
            AND l.started_at IS NOT NULL AND l.kind::text = $2
          ORDER BY l.started_at DESC, a.seq NULLS LAST, s.ticket NULLS LAST, s.call_no NULLS LAST",
    )
    .bind(user.id)
    .bind(kind)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|Строка112 { id, lesson_id, title, kind, segment, running, started, seq, sid,
                          ticket, call_no, legend, type_title, session_id, finished, score,
                          number, hidden }| {
            let topic = тема_112(&kind, segment.as_deref(), type_title, &legend);
            let status = if finished.is_some() {
                "done"
            } else if session_id.is_some() {
                "in_progress"
            } else if running {
                "not_started"
            } else {
                "missed"
            };
            WorkItem {
                id,
                lesson_id,
                lesson_title: title,
                kind,
                segment,
                lesson_running: running,
                issued_at: started,
                topic,
                detail: начало(&legend, 120),
                status: status.into(),
                score: if hidden { None } else { score },
                hidden,
                seq,
                scenario_id: Some(sid),
                ticket,
                call_no,
                session_id,
                card_number: number,
                card_id: None,
            }
        })
        .collect())
}

type СтрокаДдс = (
    Uuid, Uuid, String, String, Option<String>, bool, Option<DateTime<Utc>>,
    Uuid, String, Option<String>, String, serde_json::Value, Vec<String>, bool,
);

async fn dds(state: &AppState, user: &AuthUser, kind: &str) -> AppResult<Vec<WorkItem>> {
    let Some(service) = user.service_id.clone() else {
        return Ok(Vec::new());
    };

    // Выданные преподавателем происшествия своей службы, чьё время
    // пришло. Карточки курсантов 112 сюда не попадают (origin)
    let rows: Vec<СтрокаДдс> = sqlx::query_as(
        "SELECT lc.card_id, l.id, l.title, l.kind::text, l.segment,
                l.finished_at IS NULL, lc.release_at,
                c.id, c.number,
                (SELECT t.title FROM incident_types t WHERE t.id = c.type_ids[1]),
                c.description, c.address,
                ARRAY(SELECT r.status::text FROM response_entries r
                       WHERE r.card_id = c.id AND r.service_id = $3 ORDER BY r.at, r.id),
                l.solo_user_id IS NOT NULL
           FROM lesson_cards lc
           JOIN lessons l ON l.id = lc.lesson_id
           JOIN group_members m ON m.group_id = l.group_id AND m.user_id = $1
           JOIN cards c ON c.id = lc.card_id
          WHERE l.mode = 'dds' AND l.started_at IS NOT NULL AND l.kind::text = $2
            -- Личное занятие (самостоятельная тренировка) — только своему
            AND (l.solo_user_id IS NULL OR l.solo_user_id = $1)
            AND c.origin = 'teacher' AND lc.release_at <= now()
            AND EXISTS (SELECT 1 FROM card_services cs
                         WHERE cs.card_id = c.id AND cs.service_id = $3)
          ORDER BY lc.release_at DESC, c.number",
    )
    .bind(user.id)
    .bind(kind)
    .bind(&service)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for (id, lesson_id, title, kind, segment, running, released, card_id, number, type_title,
         description, address, chain, solo) in rows
    {
        let содержательные: Vec<&String> = chain
            .iter()
            .filter(|s| !matches!(s.as_str(), "Добавлена" | "Получена службой"))
            .collect();
        let закрыта = содержательные.last().is_some_and(|s| {
            matches!(s.as_str(), "Работы завершены" | "Отказ от выполнения работ")
        });
        let status = if закрыта {
            "done"
        } else if !chain.is_empty() {
            "in_progress"
        } else if running || solo {
            // Личное занятие завершено сразу при выдаче: пропущенным
            // его происшествие не становится
            "not_started"
        } else {
            "missed"
        };
        // Экзамен ДДС: балл после конца занятия
        let hidden = kind == "marathon" && running;
        let score = if hidden || status == "not_started" {
            None
        } else {
            crate::api::brigade::score_for(&state.db, card_id, &service).await?
        };
        let адрес = ["settlement", "street", "house"]
            .iter()
            .filter_map(|k| address.get(*k).and_then(|v| v.as_str()))
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        out.push(WorkItem {
            id,
            lesson_id,
            lesson_title: title,
            kind,
            segment,
            lesson_running: running,
            issued_at: released,
            topic: type_title.unwrap_or_else(|| начало(&description, 60)),
            detail: if адрес.is_empty() { начало(&description, 120) } else { адрес },
            status: status.into(),
            score,
            hidden,
            seq: None,
            scenario_id: None,
            ticket: None,
            call_no: None,
            session_id: None,
            card_number: Some(number),
            card_id: Some(card_id),
        });
    }
    Ok(out)
}

/// Начало текста по границе символа, с многоточием, если обрезано.
/// Тема строки у оператора 112.
///
/// Тип происшествия оператор 112 определяет сам — в экзамене (и в
/// блице по типу) он и есть ответ. Тема «1. 101» выдавала его до начала
/// вызова (UX-проход 1, находка 12). Там тема нейтральная, а различает
/// вызовы первая фраза заявителя в подробностях — её курсант и так
/// услышит.
fn тема_112(kind: &str, segment: Option<&str>, type_title: Option<String>, legend: &str) -> String {
    let тип_скрыт = kind == "marathon" || segment == Some("type");
    match type_title {
        _ if тип_скрыт => "Учебный вызов".to_string(),
        Some(t) => t,
        None => начало(legend, 60),
    }
}

fn начало(text: &str, n: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= n {
        return t.to_string();
    }
    format!("{}…", t.chars().take(n).collect::<String>().trim_end())
}

#[cfg(test)]
mod tests {
    use super::тема_112;

    #[test]
    fn в_экзамене_тема_не_выдаёт_тип_происшествия() {
        let тема = тема_112("marathon", None, Some("101".into()), "Алло! Горит крыша частного дома!");
        assert_eq!(тема, "Учебный вызов");
        assert!(!тема.contains("101"));
    }

    #[test]
    fn в_блице_по_типу_тема_не_выдаёт_тип_а_по_адресу_выдаёт() {
        assert_eq!(тема_112("blitz", Some("type"), Some("101".into()), "Алло!"), "Учебный вызов");
        assert_eq!(тема_112("blitz", Some("address"), Some("101".into()), "Алло!"), "101");
    }

    #[test]
    fn в_учебном_вызове_тема_тип_а_без_типа_начало_фразы() {
        assert_eq!(тема_112("training", None, Some("101".into()), "Алло!"), "101");
        assert_eq!(тема_112("training", None, None, "Алло! Горит крыша"), "Алло! Горит крыша");
    }
}
