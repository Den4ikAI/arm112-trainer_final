//! Преподавателю: ход идущего занятия по местам и аналитика по группе
//! и по ученику.
//!
//! Заказчик на встрече описал удалённый контроль так: «на отдельном
//! компьютере преподаватель видит действия всех обучаемых». Монитор —
//! это он: место за местом, операторы 112 и диспетчеры ДДС отдельно,
//! у каждого текущая карточка, время против норматива и ход заполнения.

use crate::{
    api::auth::{AuthUser, Role},
    api::brigade,
    api::grading::{attempts_112, attempts_dds, Attempt},
    domain::response,
    domain::weights::{self, Group},
    error::{AppError, AppResult},
    reports::forecast::setting,
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/lessons/{id}/monitor", get(monitor))
        .route("/api/analytics/groups/{id}", get(group_analytics))
        .route("/api/analytics/students/{id}", get(student_analytics))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub key: &'static str,
    pub label: &'static str,
    pub done: bool,
}

/// Текущая карточка места.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCard {
    pub card_id: Uuid,
    pub number: String,
    /// Тема: билет и вызов, итоговый тип или начало описания
    pub theme: String,
    pub address: String,
    /// Секунд с начала вызова (112) или с поступления карточки (ДДС)
    pub elapsed_sec: i64,
    /// Норматив, секунды: набор карточки у 112, первая запись у ДДС
    pub norm_sec: i64,
    pub late: bool,
    /// Ход заполнения карточки 112
    pub steps: Vec<Step>,
    /// Текущий статус реагирования у ДДС
    pub status: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Seat {
    pub seat: usize,
    pub user_id: Uuid,
    pub fio: String,
    /// `op112` или `dds`
    pub duty: String,
    pub service_id: Option<String>,
    pub workstation: Option<String>,
    /// Выдано карточек в занятии и сдано
    pub tasks_total: i64,
    pub tasks_done: i64,
    /// 112: вызовов / сохранил / сдал. ДДС: открыл / ответил / сдал
    pub counters: [i64; 3],
    /// Сколько карточек ждёт открытия (ДДС)
    pub queue: i64,
    /// Карточек, не открытых за норматив 30 с (ДДС)
    pub not_notified: i64,
    pub refused: i64,
    pub current: Option<CurrentCard>,
    pub last_score: Option<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub lesson_id: Uuid,
    pub title: String,
    pub group_title: String,
    pub mode: String,
    pub kind: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub now: DateTime<Utc>,
    pub seats: usize,
    pub working: usize,
    pub late: usize,
    pub not_notified: i64,
    pub refused: i64,
    pub not_finished: i64,
    /// Сдано попыток и из них проверено преподавателем
    pub graded: i64,
    pub reviewed: i64,
    pub op112: Vec<Seat>,
    pub dds: Vec<Seat>,
}

fn address_line(a: &Value) -> String {
    ["settlement", "street", "house", "building", "flat"]
        .iter()
        .filter_map(|k| a.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn filled(v: Option<&Value>) -> bool {
    match v {
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

/// Ход заполнения карточки 112: какие разделы уже заполнены.
fn steps_112(name: &Option<String>, addr: &Value, types: &[String], answers: &Value,
             services: i64, description: &str) -> Vec<Step> {
    let address = filled(addr.get("street")) && filled(addr.get("house")) || filled(addr.get("raw"));
    vec![
        Step { key: "applicant", label: "Заявитель", done: name.as_deref().is_some_and(|s| !s.trim().is_empty()) },
        Step { key: "address", label: "Адрес", done: address },
        Step { key: "type", label: "Тип", done: !types.is_empty() },
        Step { key: "survey", label: "Опросная карта", done: answers.as_object().is_some_and(|o| o.values().any(|v| filled(Some(v)))) },
        Step { key: "services", label: "Службы", done: services > 0 },
        Step { key: "description", label: "Описание", done: !description.trim().is_empty() },
    ]
}

async fn staff_can_see(pool: &PgPool, user: &AuthUser, lesson: Uuid) -> AppResult<()> {
    user.require(&[Role::Teacher, Role::Admin])?;
    if user.role == Role::Admin {
        return Ok(());
    }
    let teacher: Option<Uuid> = sqlx::query_scalar("SELECT teacher_id FROM lessons WHERE id = $1")
        .bind(lesson)
        .fetch_optional(pool)
        .await?;
    match teacher {
        None => Err(AppError::NotFound),
        Some(t) if t == user.id => Ok(()),
        Some(_) => Err(AppError::Forbidden),
    }
}

#[utoipa::path(
    get, path = "/api/lessons/{id}/monitor", tag = "Занятия",
    params(("id" = Uuid, Path, description = "Занятие")),
    responses(
        (status = 200, description = "Ход занятия по местам: операторы 112 и диспетчеры ДДС отдельно", body = Monitor),
        (status = 403, description = "Чужое занятие"),
        (status = 404, description = "Нет такого занятия"),
    ),
)]
pub async fn monitor(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Monitor>> {
    let pool = &state.db;
    staff_can_see(pool, &user, id).await?;
    let now = Utc::now();

    let (title, group_id, group_title, mode, kind, started_at, finished_at): (
        String, Uuid, String, String, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT l.title, l.group_id, g.title, l.mode::text, l.kind::text, l.started_at, l.finished_at
           FROM lessons l JOIN training_groups g ON g.id = l.group_id WHERE l.id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    let members: Vec<(Uuid, String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT u.id, u.fio, u.duty::text, u.service_id, u.workstation
           FROM group_members m JOIN users u ON u.id = m.user_id
          WHERE m.group_id = $1 AND u.role = 'student' AND u.deleted_at IS NULL
          ORDER BY u.fio",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    let ack = setting(pool, "norm.ack_sec", 30.0).await as i64;
    let first_record = setting(pool, "norm.first_record_sec", 180.0).await as i64;

    let mut op112 = Vec::new();
    let mut dds = Vec::new();
    let mut graded_cards: Vec<(String, Uuid, Option<String>)> = Vec::new();

    for (n, (uid, fio, duty, service_id, ws)) in members.into_iter().enumerate() {
        let duty = duty.unwrap_or_else(|| if service_id.is_some() { "dds".into() } else { "op112".into() });
        let mut seat = Seat {
            seat: n + 1, user_id: uid, fio, duty: duty.clone(), service_id: service_id.clone(),
            workstation: ws, tasks_total: 0, tasks_done: 0, counters: [0, 0, 0], queue: 0,
            not_notified: 0, refused: 0, current: None, last_score: None,
        };

        if duty == "dds" {
            if let Some(svc) = &service_id {
                let cards: Vec<(Uuid, DateTime<Utc>, String, Value, Option<String>, String)> = sqlx::query_as(
                    "SELECT lc.card_id, lc.release_at, c.number, c.address, c.outcome_type, c.description
                       FROM lesson_cards lc JOIN cards c ON c.id = lc.card_id
                      WHERE lc.lesson_id = $1
                        AND EXISTS (SELECT 1 FROM card_services cs
                                     WHERE cs.card_id = lc.card_id AND cs.service_id = $2)
                      ORDER BY lc.release_at",
                )
                .bind(id)
                .bind(svc)
                .fetch_all(pool)
                .await?;
                seat.tasks_total = cards.len() as i64;
                let ids: Vec<Uuid> = cards.iter().map(|c| c.0).collect();
                let entries: Vec<(Uuid, String)> = sqlx::query_as(
                    "SELECT card_id, status::text FROM response_entries
                      WHERE service_id = $1 AND card_id = ANY($2) ORDER BY at, id",
                )
                .bind(svc)
                .bind(&ids)
                .fetch_all(pool)
                .await?;
                let mut chain: HashMap<Uuid, Vec<String>> = HashMap::new();
                for (c, s) in entries {
                    chain.entry(c).or_default().push(s);
                }
                let mut last_done: Option<Uuid> = None;
                for (card_id, release_at, number, address, outcome, description) in &cards {
                    if *release_at > now {
                        continue;
                    }
                    let ch = chain.get(card_id).cloned().unwrap_or_default();
                    let opened = !ch.is_empty();
                    let content: Vec<&String> = ch
                        .iter()
                        .filter(|s| !matches!(s.as_str(), "Добавлена" | "Получена службой"))
                        .collect();
                    let done = content.last().is_some_and(|s| response::is_terminal(s));
                    if ch.iter().any(|s| s == "Не принята" || s == "Отказ от выполнения работ") {
                        seat.refused += 1;
                    }
                    let waited = (now - *release_at).num_seconds();
                    if opened { seat.counters[0] += 1 } else {
                        seat.queue += 1;
                        if waited > ack { seat.not_notified += 1; }
                    }
                    if !content.is_empty() { seat.counters[1] += 1; }
                    if done {
                        seat.counters[2] += 1;
                        seat.tasks_done += 1;
                        last_done = Some(*card_id);
                        graded_cards.push(("dds".into(), *card_id, Some(svc.clone())));
                    }
                    // Текущая — открытая и не закрытая; её вытесняет только более поздняя
                    if opened && !done {
                        seat.current = Some(CurrentCard {
                            card_id: *card_id,
                            number: number.clone(),
                            theme: outcome.clone().unwrap_or_else(|| description.chars().take(60).collect()),
                            address: address_line(address),
                            elapsed_sec: waited,
                            norm_sec: first_record,
                            late: content.is_empty() && waited > first_record,
                            steps: Vec::new(),
                            status: content.last().map(|s| s.to_string()),
                        });
                    }
                }
                if let Some(c) = last_done {
                    seat.last_score = brigade::score_for(pool, c, svc).await?;
                }
            }
            dds.push(seat);
            continue;
        }

        type S = (Uuid, DateTime<Utc>, i32, Option<Uuid>, Option<String>, Option<DateTime<Utc>>,
                  Option<i32>, Option<i32>, Option<i32>, Option<Value>, Option<Vec<String>>,
                  Option<Value>, Option<String>, Option<String>, Option<i64>);
        let sessions: Vec<S> = sqlx::query_as(
            "SELECT s.id, s.started_at, s.norm_sec, c.id, c.number, c.saved_at, g.score,
                    sc.ticket, sc.call_no, c.address, c.type_ids, c.answers, c.description,
                    c.applicant_name,
                    (SELECT count(*) FROM card_services cs WHERE cs.card_id = c.id)
               FROM sessions s
               JOIN assignments a ON a.id = s.assignment_id AND a.lesson_id = $1
          LEFT JOIN cards c ON c.session_id = s.id
          LEFT JOIN grades g ON g.session_id = s.id
          LEFT JOIN scenarios sc ON sc.id = s.scenario_id
              WHERE s.user_id = $2
              ORDER BY s.started_at",
        )
        .bind(id)
        .bind(uid)
        .fetch_all(pool)
        .await?;
        seat.tasks_total = sqlx::query_scalar(
            "SELECT count(*) FROM assignments WHERE lesson_id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(uid)
        .fetch_one(pool)
        .await?;
        for (_sid, started, norm, card_id, number, saved, score, ticket, call_no, addr, types, answers,
             description, name, services) in &sessions {
            seat.counters[0] += 1;
            if saved.is_some() { seat.counters[1] += 1; }
            if let Some(sc) = score {
                seat.counters[2] += 1;
                seat.last_score = Some(*sc);
                if let Some(c) = card_id { graded_cards.push(("card112".into(), *c, None)); }
            }
            if saved.is_none() {
                if let Some(c) = card_id {
                    let elapsed = (now - *started).num_seconds();
                    let addr = addr.clone().unwrap_or(Value::Null);
                    let theme = match (ticket, call_no) {
                        (Some(t), Some(c)) => format!("Билет {t} · вызов {c}"),
                        _ => "Вызов 112".into(),
                    };
                    seat.current = Some(CurrentCard {
                        card_id: *c,
                        number: number.clone().unwrap_or_default(),
                        theme,
                        address: address_line(&addr),
                        elapsed_sec: elapsed,
                        norm_sec: *norm as i64,
                        late: elapsed > *norm as i64,
                        steps: steps_112(name, &addr, types.as_deref().unwrap_or(&[]),
                                         answers.as_ref().unwrap_or(&Value::Null),
                                         services.unwrap_or(0), description.as_deref().unwrap_or("")),
                        status: None,
                    });
                }
            }
        }
        seat.tasks_done = seat.counters[2];
        op112.push(seat);
    }

    let reviewed: i64 = {
        let mut n = 0;
        for (kind, card, svc) in &graded_cards {
            let has: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM expert_grades
                                 WHERE kind = $1 AND card_id = $2 AND service_id IS NOT DISTINCT FROM $3)",
            )
            .bind(kind)
            .bind(card)
            .bind(svc)
            .fetch_one(pool)
            .await?;
            if has { n += 1; }
        }
        n
    };

    let all: Vec<&Seat> = op112.iter().chain(dds.iter()).collect();
    Ok(Json(Monitor {
        lesson_id: id,
        title,
        group_title,
        mode,
        kind,
        started_at,
        finished_at,
        now,
        seats: all.len(),
        working: all.iter().filter(|s| s.current.is_some()).count(),
        late: all.iter().filter(|s| s.current.as_ref().is_some_and(|c| c.late) || s.not_notified > 0).count(),
        not_notified: all.iter().map(|s| s.not_notified).sum(),
        refused: all.iter().map(|s| s.refused).sum(),
        not_finished: dds.iter().map(|s| s.counters[0] - s.counters[2]).sum(),
        graded: graded_cards.len() as i64,
        reviewed,
        op112,
        dds,
    }))
}

// ─── Аналитика ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DutyQuery {
    /// `op112` или `dds`; не задан — оба
    pub duty: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupRate {
    pub group: Group,
    /// Доля непройденных проверок группы, %
    pub error_pct: Option<f64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudentRow {
    pub user_id: Uuid,
    pub fio: String,
    pub duty: String,
    pub attempts: usize,
    pub avg_score: Option<f64>,
    /// Среднее «время / норматив» у 112, 1.0 — ровно норматив
    pub time_ratio: Option<f64>,
    pub rates: Vec<GroupRate>,
    /// На чём чаще всего ошибается
    pub top_issues: Vec<String>,
    /// Последний прогноз и средняя ошибка прогноза
    pub forecast: Option<i32>,
    pub forecast_error: Option<f64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IssueCount {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GroupAnalytics {
    pub group_id: Uuid,
    pub title: String,
    pub attempts: usize,
    pub avg_score: Option<f64>,
    pub time_ratio: Option<f64>,
    /// Группы проверок, по которым строится тепловая карта
    pub checks: Vec<Group>,
    pub typical: Vec<IssueCount>,
    pub students: Vec<StudentRow>,
}

fn checks_for(duty: &str) -> Vec<Group> {
    if duty == "dds" {
        vec![Group::Time, Group::Statuses, Group::Comments]
    } else {
        vec![Group::Time, Group::Address, Group::TypeServices, Group::Completeness]
    }
}

fn rates(attempts: &[&Attempt], groups: &[Group]) -> Vec<GroupRate> {
    groups
        .iter()
        .map(|g| {
            let (fail, all) = attempts
                .iter()
                .flat_map(|a| a.checks.iter())
                .filter(|(cg, _)| cg == g)
                .fold((0usize, 0usize), |(f, n), (_, ok)| (f + usize::from(!ok), n + 1));
            GroupRate { group: *g, error_pct: (all > 0).then(|| 100.0 * fail as f64 / all as f64) }
        })
        .collect()
}

fn top_issues(attempts: &[&Attempt], n: usize) -> Vec<IssueCount> {
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for a in attempts {
        for i in &a.issues {
            *count.entry(i.as_str()).or_default() += 1;
        }
    }
    let mut v: Vec<IssueCount> = count
        .into_iter()
        .map(|(label, count)| IssueCount { label: label.to_string(), count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    v.truncate(n);
    v
}

fn mean(v: impl Iterator<Item = f64>) -> Option<f64> {
    let (s, n) = v.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| s / n as f64)
}

async fn forecasts(pool: &PgPool, users: &[Uuid]) -> AppResult<HashMap<Uuid, (Option<i32>, Option<f64>)>> {
    let rows: Vec<(Uuid, i32, Option<i32>)> = sqlx::query_as(
        "SELECT p.user_id, p.predicted, g.score
           FROM predictions p
           JOIN sessions s ON s.id = p.session_id
      LEFT JOIN grades g ON g.session_id = p.session_id
          WHERE p.user_id = ANY($1)
          ORDER BY s.started_at",
    )
    .bind(users)
    .fetch_all(pool)
    .await?;
    let mut by: HashMap<Uuid, Vec<(i32, Option<i32>)>> = HashMap::new();
    for (u, p, f) in rows {
        by.entry(u).or_default().push((p, f));
    }
    Ok(by
        .into_iter()
        .map(|(u, v)| {
            let last = v.last().map(|x| x.0);
            let err = mean(v.iter().filter_map(|(p, f)| f.map(|f| (p - f).abs() as f64)));
            (u, (last, err))
        })
        .collect())
}

#[utoipa::path(
    get, path = "/api/analytics/groups/{id}", tag = "Отчёты",
    params(("id" = Uuid, Path, description = "Учебная группа"), DutyQuery),
    responses(
        (status = 200, description = "Аналитика по группе: баллы, время, типичные ошибки, тепловая карта, прогноз", body = GroupAnalytics),
        (status = 403, description = "Только преподаватель и администратор"),
    ),
)]
pub async fn group_analytics(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<DutyQuery>,
) -> AppResult<Json<GroupAnalytics>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let pool = &state.db;
    let title: String = sqlx::query_scalar("SELECT title FROM training_groups WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let members: Vec<(Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT u.id, u.fio, u.duty::text, u.service_id FROM group_members m
           JOIN users u ON u.id = m.user_id
          WHERE m.group_id = $1 AND u.role = 'student' AND u.deleted_at IS NULL ORDER BY u.fio",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let duty_of = |d: &Option<String>, s: &Option<String>| {
        d.clone().unwrap_or_else(|| if s.is_some() { "dds".into() } else { "op112".into() })
    };
    let members: Vec<_> = members
        .into_iter()
        .map(|(u, f, d, s)| (u, f, duty_of(&d, &s)))
        .filter(|(_, _, d)| q.duty.as_deref().is_none_or(|w| w == d))
        .collect();
    let duty = q.duty.clone().unwrap_or_else(|| "op112".into());
    let ids: Vec<Uuid> = members.iter().map(|m| m.0).collect();

    let mut all = attempts_112(pool, None).await?;
    all.extend(attempts_dds(pool, None).await?);
    let w = weights::load(pool).await;
    let mine: Vec<&Attempt> = all
        .iter()
        .filter(|a| ids.contains(&a.user_id))
        .filter(|a| q.duty.as_deref().is_none_or(|d| (d == "dds") == (a.kind == "dds")))
        .collect();
    let groups = checks_for(&duty);
    let fc = forecasts(pool, &ids).await?;

    let students: Vec<StudentRow> = members
        .iter()
        .map(|(uid, fio, d)| {
            let a: Vec<&Attempt> = mine.iter().copied().filter(|x| x.user_id == *uid).collect();
            let scores: Vec<f64> = a.iter().filter_map(|x| x.score(&w)).map(f64::from).collect();
            StudentRow {
                user_id: *uid,
                fio: fio.clone(),
                duty: d.clone(),
                attempts: a.len(),
                avg_score: mean(scores.into_iter()),
                time_ratio: mean(a.iter().filter_map(|x| match (x.elapsed_sec, x.norm_sec) {
                    (Some(e), Some(n)) if n > 0 => Some(e as f64 / n as f64),
                    _ => None,
                })),
                rates: rates(&a, &checks_for(d)),
                top_issues: top_issues(&a, 3).into_iter().map(|i| i.label).collect(),
                forecast: fc.get(uid).and_then(|x| x.0),
                forecast_error: fc.get(uid).and_then(|x| x.1),
            }
        })
        .collect();

    Ok(Json(GroupAnalytics {
        group_id: id,
        title,
        attempts: mine.len(),
        avg_score: mean(mine.iter().filter_map(|a| a.score(&w)).map(f64::from)),
        time_ratio: mean(mine.iter().filter_map(|x| match (x.elapsed_sec, x.norm_sec) {
            (Some(e), Some(n)) if n > 0 => Some(e as f64 / n as f64),
            _ => None,
        })),
        checks: groups,
        typical: top_issues(&mine, 10),
        students,
    }))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRow {
    pub at: DateTime<Utc>,
    /// `card112` или `dds`
    pub kind: String,
    pub card_id: Uuid,
    pub card_number: String,
    pub score: Option<i32>,
    pub elapsed_sec: Option<i32>,
    pub norm_sec: Option<i32>,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForecastPoint {
    pub at: DateTime<Utc>,
    pub predicted: i32,
    pub actual: Option<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudentAnalytics {
    pub user_id: Uuid,
    pub fio: String,
    pub duty: String,
    pub avg_score: Option<f64>,
    pub attempts: Vec<AttemptRow>,
    pub rates: Vec<GroupRate>,
    pub top_issues: Vec<IssueCount>,
    pub forecast: Vec<ForecastPoint>,
}

#[utoipa::path(
    get, path = "/api/analytics/students/{id}", tag = "Отчёты",
    params(("id" = Uuid, Path, description = "Обучающийся")),
    responses(
        (status = 200, description = "История попыток, ошибки по группам проверок, прогноз против факта", body = StudentAnalytics),
        (status = 403, description = "Только преподаватель и администратор"),
        (status = 404, description = "Нет такого обучающегося"),
    ),
)]
pub async fn student_analytics(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<StudentAnalytics>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let pool = &state.db;
    let (fio, duty, service): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT fio, duty::text, service_id FROM users WHERE id = $1 AND role = 'student'",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let duty = duty.unwrap_or_else(|| if service.is_some() { "dds".into() } else { "op112".into() });
    let mut all = attempts_112(pool, Some(id)).await?;
    all.extend(attempts_dds(pool, Some(id)).await?);
    all.sort_by_key(|a| a.at);
    let w = weights::load(pool).await;
    let refs: Vec<&Attempt> = all.iter().collect();

    let forecast: Vec<ForecastPoint> = sqlx::query_as::<_, (DateTime<Utc>, i32, Option<i32>)>(
        "SELECT s.started_at, p.predicted, g.score FROM predictions p
           JOIN sessions s ON s.id = p.session_id
      LEFT JOIN grades g ON g.session_id = p.session_id
          WHERE p.user_id = $1 ORDER BY s.started_at",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(at, predicted, actual)| ForecastPoint { at, predicted, actual })
    .collect();

    Ok(Json(StudentAnalytics {
        user_id: id,
        fio,
        rates: rates(&refs, &checks_for(&duty)),
        duty,
        avg_score: mean(all.iter().filter_map(|a| a.score(&w)).map(f64::from)),
        top_issues: top_issues(&refs, 8),
        attempts: all
            .iter()
            .map(|a| AttemptRow {
                at: a.at,
                kind: a.kind.to_string(),
                card_id: a.card_id,
                card_number: a.card_number.clone(),
                score: a.score(&w),
                elapsed_sec: a.elapsed_sec,
                norm_sec: a.norm_sec,
                issues: a.issues.clone(),
            })
            .collect(),
        forecast,
    }))
}
