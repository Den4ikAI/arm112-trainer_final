//! Веса оценки: просмотр, пересчёт «было → стало» без сохранения,
//! сохранение с пересчётом всех попыток.
//!
//! Балл оператора 112 хранится в `grades.score` и при сохранении весов
//! пересчитывается по сохранённым замечаниям — разбор заново не
//! запускается, эталон не пересчитывается. Балл диспетчера ДДС
//! считается при каждом показе (`api::brigade::load`) и новые веса
//! подхватывает сам.

use crate::{
    api::auth::{AuthUser, Role},
    api::brigade,
    audit,
    domain::grade::Finding,
    domain::weights::{self, Group, TextChecks, Weights},
    error::{AppError, AppResult},
    reports::forecast::setting,
    state::AppState,
};
use axum::{extract::State, routing::{get, post}, Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/grading/weights", get(view).put(save))
        .route("/api/grading/weights/preview", post(preview))
}

/// Попытка для пересчёта и аналитики: 112 — вызов, ДДС — служба на карточке.
#[derive(Debug)]
pub struct Attempt {
    pub user_id: Uuid,
    /// `card112` или `dds`
    pub kind: &'static str,
    pub at: DateTime<Utc>,
    pub card_id: Uuid,
    pub card_number: String,
    /// Сохранённый балл (у ДДС — по сохранённым весам)
    pub stored: Option<i32>,
    /// Проверки по группам: (группа, пройдена)
    pub checks: Vec<(Group, bool)>,
    /// Подписи непройденных проверок — для «типичных ошибок»
    pub issues: Vec<String>,
    pub elapsed_sec: Option<i32>,
    pub norm_sec: Option<i32>,
    /// Только у 112: для пересчёта
    pub session_id: Option<Uuid>,
    findings_112: Option<(Vec<Finding>, bool, TextChecks)>,
    findings_dds: Option<Vec<crate::domain::brigade::DdsFinding>>,
}

impl Attempt {
    pub fn score(&self, w: &Weights) -> Option<i32> {
        if let Some((f, over, text)) = &self.findings_112 {
            return Some(weights::score_112(f, *over, *text, w));
        }
        self.findings_dds.as_ref().and_then(|f| weights::score_dds(f, w))
    }
}

/// Подпись ошибки оператора 112 для сводки по группе.
fn issue_label_112(f: &Finding) -> String {
    use crate::domain::grade::FindingKind as K;
    let what = if f.field.starts_with("service.") {
        match f.kind {
            K::Missing => "Не оповещена служба".to_string(),
            _ => "Лишняя служба".to_string(),
        }
    } else {
        let how = match f.kind {
            K::Wrong => "неверно",
            K::Missing => "не заполнено",
            K::Guessed => "угадано, не выяснено",
            K::Extra => "лишнее",
            K::Ok => "",
        };
        format!("{}: {how}", f.label)
    };
    what
}

/// Подпись ошибки диспетчера ДДС.
fn issue_label_dds(k: crate::domain::brigade::DdsFindingKind) -> String {
    use crate::domain::brigade::DdsFindingKind as K;
    match k {
        K::OpenedLate => "Карточка открыта позже 30 с",
        K::FirstRecordLate => "Первая запись позже 3 мин",
        K::FirstRecordNoText => "Первая запись без текста",
        K::Late => "Статус обновлён с опозданием",
        K::NotByFact => "Статус не по факту доклада",
        K::Missed => "Доклад без статуса",
        K::NoBrigade => "Бригада не направлена",
        K::Ok => "",
    }
    .to_string()
}

/// Замечаний, не отклонённых преподавателем.
fn text_issues(findings: &Value, verdicts: &Value) -> usize {
    let n = findings.as_array().map_or(0, Vec::len);
    (0..n)
        .filter(|i| {
            verdicts
                .get(i.to_string())
                .and_then(|v| v.get("verdict"))
                .and_then(Value::as_str)
                != Some("rejected")
        })
        .count()
}

/// Попытки оператора 112 — по сохранённым разборам. `user` — одного ученика.
pub async fn attempts_112(pool: &PgPool, user: Option<Uuid>) -> AppResult<Vec<Attempt>> {
    type Row = (Uuid, Uuid, DateTime<Utc>, Uuid, String, i32, Value, bool, i32, i32,
                Option<String>, Option<Value>, Option<Value>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT g.session_id, s.user_id, s.started_at, c.id, c.number, g.score, g.findings,
                g.over_norm, g.elapsed_sec, g.norm_sec, ta.status, ta.findings, ta.verdicts
           FROM grades g
           JOIN sessions s ON s.id = g.session_id
           JOIN cards c ON c.session_id = s.id
      LEFT JOIN text_analyses ta ON ta.kind = 'card112' AND ta.card_id = c.id
          WHERE ($1::uuid IS NULL OR s.user_id = $1)
          ORDER BY s.started_at",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(sid, uid, at, cid, number, score, findings, over, el, norm, ta_status, ta_f, ta_v)| {
            let findings: Vec<Finding> = serde_json::from_value(findings).unwrap_or_default();
            let text = TextChecks {
                ready: ta_status.as_deref() == Some("ready"),
                issues: match (&ta_f, &ta_v) {
                    (Some(f), Some(v)) => text_issues(f, v),
                    _ => 0,
                },
            };
            let mut checks: Vec<(Group, bool)> = findings
                .iter()
                .map(|f| (weights::group_112(&f.field), f.kind == crate::domain::grade::FindingKind::Ok))
                .collect();
            checks.push((Group::Time, !over));
            let mut issues: Vec<String> = findings
                .iter()
                .filter(|f| f.kind != crate::domain::grade::FindingKind::Ok)
                .map(|f| issue_label_112(f))
                .collect();
            if over {
                issues.push("Норматив набора карточки превышен".into());
            }
            Attempt {
                issues,
                user_id: uid,
                kind: "card112",
                at,
                card_id: cid,
                card_number: number,
                stored: Some(score),
                checks,
                elapsed_sec: Some(el),
                norm_sec: Some(norm),
                session_id: Some(sid),
                findings_112: Some((findings, over, text)),
                findings_dds: None,
            }
        })
        .collect())
}

/// Попытки диспетчеров ДДС — служба на карточке, где назначена бригада.
pub async fn attempts_dds(pool: &PgPool, user: Option<Uuid>) -> AppResult<Vec<Attempt>> {
    let rows: Vec<(Uuid, String, Uuid, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT ba.card_id, ba.service_id, ba.user_id, ba.assigned_at, c.number
           FROM brigade_assignments ba
           JOIN cards c ON c.id = ba.card_id
          WHERE ba.user_id IS NOT NULL AND ($1::uuid IS NULL OR ba.user_id = $1)
          ORDER BY ba.assigned_at
          LIMIT 2000",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (card_id, service_id, uid, at, number) in rows {
        let view = brigade::load(pool, card_id, &service_id, true).await?;
        let f = view.debrief.findings;
        let checks = f
            .iter()
            .map(|x| (weights::group_dds(x.kind), x.kind == crate::domain::brigade::DdsFindingKind::Ok))
            .collect();
        let issues = f
            .iter()
            .filter(|x| x.kind != crate::domain::brigade::DdsFindingKind::Ok)
            .map(|x| issue_label_dds(x.kind))
            .collect();
        out.push(Attempt {
            issues,
            user_id: uid,
            kind: "dds",
            at,
            card_id,
            card_number: number,
            stored: view.debrief.score,
            checks,
            elapsed_sec: None,
            norm_sec: None,
            session_id: None,
            findings_112: None,
            findings_dds: Some(f),
        });
    }
    Ok(out)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WeightsView {
    pub weights: Weights,
    pub defaults: Weights,
    /// Потолок балла при критичной ошибке
    pub critical_cap: i32,
}

#[utoipa::path(
    get, path = "/api/grading/weights", tag = "Оценка",
    responses(
        (status = 200, description = "Сохранённые веса и значения по умолчанию", body = WeightsView),
        (status = 403, description = "Только преподаватель и администратор"),
    ),
)]
pub async fn view(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<WeightsView>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    Ok(Json(WeightsView {
        weights: weights::load(&state.db).await,
        defaults: Weights::default(),
        critical_cap: weights::CRITICAL_CAP,
    }))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub avg: Option<f64>,
    /// Попыток с баллом не ниже порога аттестации
    pub passing: usize,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudentShift {
    pub user_id: Uuid,
    pub fio: String,
    pub duty: Option<String>,
    pub attempts: usize,
    pub before: Option<f64>,
    pub after: Option<f64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub total: usize,
    /// Сколько попыток сменят балл
    pub changed: usize,
    pub threshold: i32,
    pub before: Totals,
    pub after: Totals,
    pub students: Vec<StudentShift>,
}

fn avg(v: &[i32]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<i32>() as f64 / v.len() as f64)
}

async fn compute(pool: &PgPool, new: &Weights) -> AppResult<Preview> {
    let old = weights::load(pool).await;
    let threshold = setting(pool, "attestation.threshold", 70.0).await as i32;
    let mut all = attempts_112(pool, None).await?;
    all.extend(attempts_dds(pool, None).await?);

    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut changed = 0;
    let mut by_user: BTreeMap<Uuid, (Vec<i32>, Vec<i32>)> = BTreeMap::new();
    for a in &all {
        let (Some(b), Some(n)) = (a.score(&old), a.score(new)) else { continue };
        if b != n {
            changed += 1;
        }
        before.push(b);
        after.push(n);
        let e = by_user.entry(a.user_id).or_default();
        e.0.push(b);
        e.1.push(n);
    }

    let ids: Vec<Uuid> = by_user.keys().copied().collect();
    let names: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT id, fio, duty::text FROM users WHERE id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    let mut students: Vec<StudentShift> = names
        .into_iter()
        .map(|(id, fio, duty)| {
            let (b, n) = by_user.get(&id).cloned().unwrap_or_default();
            StudentShift { user_id: id, fio, duty, attempts: b.len(), before: avg(&b), after: avg(&n) }
        })
        .collect();
    students.sort_by(|a, b| b.after.partial_cmp(&a.after).unwrap_or(std::cmp::Ordering::Equal));

    Ok(Preview {
        total: before.len(),
        changed,
        threshold,
        before: Totals { avg: avg(&before), passing: before.iter().filter(|s| **s >= threshold).count() },
        after: Totals { avg: avg(&after), passing: after.iter().filter(|s| **s >= threshold).count() },
        students,
    })
}

#[utoipa::path(
    post, path = "/api/grading/weights/preview", tag = "Оценка",
    request_body = Weights,
    responses(
        (status = 200, description = "Баллы всех попыток при этих весах, без сохранения", body = Preview),
        (status = 400, description = "Вес вне 0–5"),
        (status = 403, description = "Только преподаватель и администратор"),
    ),
)]
pub async fn preview(
    State(state): State<AppState>,
    user: AuthUser,
    Json(w): Json<Weights>,
) -> AppResult<Json<Preview>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    if !w.valid() {
        return Err(AppError::Validation("вес группы — от 0 до 5".into()));
    }
    Ok(Json(compute(&state.db, &w).await?))
}

#[utoipa::path(
    put, path = "/api/grading/weights", tag = "Оценка",
    request_body = Weights,
    responses(
        (status = 200, description = "Веса сохранены, баллы пересчитаны", body = Preview),
        (status = 400, description = "Вес вне 0–5"),
        (status = 403, description = "Только преподаватель и администратор"),
    ),
)]
pub async fn save(
    State(state): State<AppState>,
    user: AuthUser,
    Json(w): Json<Weights>,
) -> AppResult<Json<Preview>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    if !w.valid() {
        return Err(AppError::Validation("вес группы — от 0 до 5".into()));
    }
    let pool = &state.db;
    let old = weights::load(pool).await;
    // Сводку «было → стало» считаем до сохранения — после него «было»
    // уже не восстановить
    let summary = compute(pool, &w).await?;

    let mut tx = pool.begin().await?;
    for (key, g) in weights::KEYS {
        sqlx::query(
            "UPDATE settings SET value = to_jsonb($2::int), updated_at = now(), updated_by = $3
              WHERE key = $1",
        )
        .bind(key)
        .bind(w.of(g))
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    }
    let mut recalculated = 0usize;
    for a in attempts_112(pool, None).await? {
        let (Some(sid), Some(score)) = (a.session_id, a.score(&w)) else { continue };
        if Some(score) != a.stored {
            sqlx::query("UPDATE grades SET score = $2 WHERE session_id = $1")
                .bind(sid)
                .bind(score)
                .execute(&mut *tx)
                .await?;
            recalculated += 1;
        }
    }
    tx.commit().await?;

    // Изменение оценок без записи в журнал ТЗ запрещает
    audit::log(
        pool,
        audit::Event::new("grading.weights")
            .from_user(&user)
            .with(json!({ "before": old, "after": w, "recalculated": recalculated })),
    )
    .await;

    Ok(Json(summary))
}
