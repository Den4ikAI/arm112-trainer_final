//! IP-телефон диспетчера ДДС: справочник, вызовы, журнал и запись
//! разговора.
//!
//! Обратная связь экспертов 18.10 — «виртуальный IP-телефон для
//! специалиста ДДС». В ТЗ — «программная эмуляция IP-телефона» на
//! рабочем месте, обработка вызовов через виртуальную IP-телефонию,
//! записи учебных вызовов в WAV и параметры телефонии у администратора.
//!
//! Сам разговор ведут прежние модули: что старший бригады докладывает —
//! `api::brigade`, как начальник принимает доклад — `api::chief`.
//! Телефон — то, что вокруг: кто кому звонил, когда, взял ли трубку,
//! что прозвучало, и запись, которую потом слушает преподаватель.
//! Голос собеседника — синтез, поэтому в изолированном контуре
//! телефону не нужна ни АТС, ни второй человек на линии.
//!
//! Номера внутренние, четырёхзначные, как в инструкции АРМ («номер
//! телефона оператора: например, 2407»): у службы сотня номеров по её
//! месту в справочнике, диспетчер — первый. Кому ещё можно позвонить,
//! решает преподаватель — справочник службы (`api::phonebook`,
//! рекомендации финалистам, п. 8): руководитель организации, начальник
//! отдела реагирования, начальник и специалист аварийной бригады и,
//! если преподаватель включил, заявитель по номеру из карточки.

use crate::{
    api::{
        auth::{AuthUser, Duty, Role},
        cards::visible_card,
        phonebook::{self, role_title},
    },
    audit,
    domain::{free_question, live_applicant, phone_audio},
    error::{AppError, AppResult},
    reports::forecast::setting,
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// Реплика длиннее — уже не реплика, а вставленный текст.
const MAX_LINE_CHARS: usize = 1000;
/// Минута речи WAV 16 кГц — около 2 МБ; с запасом на 44,1 кГц.
const MAX_AUDIO_BYTES: usize = 8 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/phone/config", get(config))
        .route("/api/cards/{id}/phone", get(view))
        .route("/api/cards/{id}/phone/calls", post(create))
        .route("/api/phone/calls/{id}/answer", post(answer))
        .route("/api/phone/calls/{id}/end", post(end))
        .route("/api/phone/calls/{id}/lines", post(add_line))
        .route("/api/phone/calls/{id}/reply", post(reply))
        .route("/api/phone/calls/{id}/lines/{line}/audio", put(line_audio).layer(DefaultBodyLimit::max(MAX_AUDIO_BYTES)))
        .route("/api/phone/calls/{id}/recording", get(recording))
}

// ─── Номера ─────────────────────────────────────────────────────────

/// Первый номер сотни службы: 2000 + 20 × место в справочнике служб.
/// Службы вне справочника (учебные, заведённые вручную) — в общей сотне.
pub async fn base_number(pool: &PgPool, service_id: &str) -> AppResult<i32> {
    let position: Option<i32> =
        sqlx::query_scalar("SELECT position FROM service_catalog WHERE service_id = $1")
            .bind(service_id)
            .fetch_optional(pool)
            .await?;
    Ok(match position {
        Some(p) if (1..=48).contains(&p) => 2000 + 20 * p,
        _ => 2980,
    })
}

/// Прежние имена собеседников — из старых вызовов и старых клиентов:
/// `brigade` — старший бригады, `chief` — вышестоящий начальник.
fn role_of(peer: &str) -> &str {
    match peer {
        "brigade" => "brigade_head",
        "chief" => "response_head",
        other => other,
    }
}

/// Роль говорит через бригаду: доклады по этапам работ.
fn is_brigade(peer: &str) -> bool {
    matches!(role_of(peer), "brigade_head" | "brigade_specialist")
}

/// Подпись абонента: из справочника в момент вызова; у бригады — с её
/// номером; старые вызовы без подписи — по прежним названиям.
fn title_of(peer: &str, stored: &str, brigade_no: Option<i32>) -> String {
    let base = match (stored.is_empty(), peer) {
        (false, _) => stored.to_string(),
        (true, "brigade") => "Старший бригады".into(),
        (true, "chief") => "Вышестоящий начальник".into(),
        (true, other) => role_title(other).into(),
    };
    match (is_brigade(peer), brigade_no) {
        (true, Some(n)) => format!("{base} № {n}"),
        _ => base,
    }
}

/// Голос синтеза для роли: бригада, начальники, заявитель по полу
/// (как в разговоре оператора 112 с заявителем).
fn voice_of(peer: &str, applicant_name: &str) -> &'static str {
    match role_of(peer) {
        "brigade_head" | "brigade_specialist" => "brigade",
        "applicant" => {
            if free_question::applicant_female(applicant_name) { "applicant_f" } else { "applicant_m" }
        }
        _ => "chief",
    }
}

/// Номер заявителя по карточке: АОН, а без него — продиктованный.
async fn applicant_of(pool: &PgPool, card_id: Uuid) -> AppResult<(String, String, String)> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT phone_aon, phone_provided, applicant_name, description FROM cards WHERE id = $1",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await?;
    let (aon, provided, name, description) = row.ok_or(AppError::NotFound)?;
    let number = if aon.trim().is_empty() { provided } else { aon };
    Ok((number.trim().to_string(), name, description))
}

// ─── Параметры ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneConfig {
    /// Звонки идут через IP-телефон (`ui.phone`)
    pub enabled: bool,
    /// Голос диспетчера сохраняется для записи разговора (`phone.record`)
    pub record: bool,
    /// Сколько звонит входящий, прежде чем стать пропущенным
    pub ring_sec: i64,
    /// Сколько длятся гудки исходящего до ответа
    pub answer_sec: i64,
    /// Распознавание речи подключено: можно говорить голосом
    pub asr: bool,
    /// Синтез подключён: собеседника слышно
    pub tts: bool,
    /// Свой внутренний номер. Есть у диспетчера со службой
    pub extension: Option<String>,
}

pub async fn enabled(pool: &PgPool) -> bool {
    setting(pool, "ui.phone", 1.0).await >= 0.5
}

#[utoipa::path(
    get,
    path = "/api/phone/config",
    tag = "IP-телефон",
    responses(
        (status = 200, description = "Параметры телефонии и свой номер", body = PhoneConfig),
        (status = 401, description = "Требуется вход"),
    ),
)]
pub async fn config(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<PhoneConfig>> {
    let extension = match (&user.service_id, user.duty) {
        (Some(sv), Some(Duty::Dds)) => Some(base_number(&state.db, sv).await?.to_string()),
        _ => None,
    };
    Ok(Json(PhoneConfig {
        enabled: enabled(&state.db).await,
        record: setting(&state.db, "phone.record", 1.0).await >= 0.5,
        ring_sec: setting(&state.db, "phone.ring_sec", 20.0).await as i64,
        answer_sec: setting(&state.db, "phone.answer_sec", 2.0).await as i64,
        asr: state.ml.asr.available(),
        tts: state.ml.tts.available(),
        extension,
    }))
}

// ─── Справочник и журнал ────────────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    /// Роль абонента из справочника службы: `org_head`, `response_head`,
    /// `brigade_head`, `brigade_specialist`, `applicant`
    pub peer: String,
    pub title: String,
    pub number: String,
    /// Позвонить можно сейчас
    pub available: bool,
    /// Почему нельзя
    pub note: Option<String>,
    /// Голос синтеза для реплик абонента
    pub voice: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    pub id: i64,
    /// `dispatcher` или `peer`
    pub who: String,
    pub text: String,
    pub at: DateTime<Utc>,
    /// Голос диспетчера сохранён
    pub has_audio: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    pub id: Uuid,
    /// ФИО диспетчера — преподавателю, который смотрит всех
    pub dispatcher: String,
    pub service_id: String,
    pub peer: String,
    pub title: String,
    /// `in` — входящий, `out` — исходящий
    pub direction: String,
    pub number: String,
    /// `ringing`, `talking`, `ended`, `missed`, `declined`
    pub outcome: String,
    pub started_at: DateTime<Utc>,
    pub answered_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Длительность разговора, с — от ответа до конца
    pub duration_sec: Option<i64>,
    pub lines: Vec<LineView>,
    /// Запись разговора можно прослушать
    pub recording: bool,
    /// Голос синтеза для реплик абонента
    pub voice: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    /// Свой номер диспетчера
    pub extension: Option<String>,
    /// Кому можно позвонить по этой карточке
    pub phonebook: Vec<Contact>,
    /// Вызовы по карточке, новые сверху. Курсанту — свои,
    /// преподавателю — всех диспетчеров
    pub calls: Vec<CallView>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ServiceQuery {
    /// Служба. Курсанту не нужна — берётся своя
    pub service_id: Option<String>,
}

/// Строка вызова из базы.
type CallRow = (
    Uuid, String, String, String, String, String, String,
    DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<i32>, String, String,
);

const CALL_SELECT: &str =
    "SELECT pc.id, u.fio, pc.service_id, pc.peer, pc.direction, pc.number, pc.outcome,
            pc.started_at, pc.answered_at, pc.ended_at,
            (SELECT a.brigade_no FROM brigade_assignments a
              WHERE a.card_id = pc.card_id AND a.service_id = pc.service_id),
            pc.title, c.applicant_name
       FROM phone_calls pc JOIN users u ON u.id = pc.user_id JOIN cards c ON c.id = pc.card_id";

async fn calls(state: &AppState, rows: Vec<CallRow>) -> AppResult<Vec<CallView>> {
    let ids: Vec<Uuid> = rows.iter().map(|r| r.0).collect();
    let lines: Vec<(Uuid, i64, String, String, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT call_id, id, who, text, at, audio IS NOT NULL FROM phone_call_lines
          WHERE call_id = ANY($1) ORDER BY id",
    )
    .bind(&ids)
    .fetch_all(&state.db)
    .await?;
    let tts = state.ml.tts.available();

    Ok(rows
        .into_iter()
        .map(|(id, fio, service_id, peer, direction, number, outcome, started_at, answered_at, ended_at, brigade_no, title, applicant)| {
            let lines: Vec<LineView> = lines
                .iter()
                .filter(|l| l.0 == id)
                .map(|(_, line_id, who, text, at, has_audio)| LineView {
                    id: *line_id,
                    who: who.clone(),
                    text: text.clone(),
                    at: *at,
                    has_audio: *has_audio,
                })
                .collect();
            let recording = !lines.is_empty() && (tts || lines.iter().any(|l| l.has_audio));
            CallView {
                id,
                dispatcher: fio,
                title: title_of(&peer, &title, brigade_no),
                voice: voice_of(&peer, &applicant).into(),
                service_id,
                peer,
                direction,
                number,
                duration_sec: answered_at
                    .zip(ended_at)
                    .map(|(a, e)| (e - a).num_seconds().max(0)),
                outcome,
                started_at,
                answered_at,
                ended_at,
                lines,
                recording,
            }
        })
        .collect())
}

async fn one(state: &AppState, call_id: Uuid) -> AppResult<CallView> {
    let row: Option<CallRow> = sqlx::query_as(&format!("{CALL_SELECT} WHERE pc.id = $1"))
        .bind(call_id)
        .fetch_optional(&state.db)
        .await?;
    let row = row.ok_or(AppError::NotFound)?;
    Ok(calls(state, vec![row]).await?.remove(0))
}

/// Служба, за которую звонит курсант: своя, и она — в списке оповещения.
async fn dispatcher_service(pool: &PgPool, card_id: Uuid, user: &AuthUser) -> AppResult<String> {
    if user.role != Role::Student {
        // Звонит диспетчер; преподаватель слушает журнал и записи
        return Err(AppError::Forbidden);
    }
    user.require_duty(Duty::Dds)?;
    let own = user
        .service_id
        .clone()
        .ok_or_else(|| AppError::Validation("у пользователя не задана служба".into()))?;
    let listed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM card_services WHERE card_id = $1 AND service_id = $2)",
    )
    .bind(card_id)
    .bind(&own)
    .fetch_one(pool)
    .await?;
    if !listed {
        return Err(AppError::Validation("ваша служба не в списке оповещения по карточке".into()));
    }
    Ok(own)
}

async fn phonebook(pool: &PgPool, card_id: Uuid, service_id: &str, user_id: Uuid) -> AppResult<Vec<Contact>> {
    let closed = crate::api::brigade::service_closed(pool, card_id, service_id).await?;
    let brigade_no: Option<i32> = sqlx::query_scalar(
        "SELECT brigade_no FROM brigade_assignments WHERE card_id = $1 AND service_id = $2",
    )
    .bind(card_id)
    .bind(service_id)
    .fetch_optional(pool)
    .await?;
    let chief_open: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM chief_reports
                         WHERE card_id = $1 AND service_id = $2 AND user_id = $3
                           AND finished_at IS NULL)",
    )
    .bind(card_id)
    .bind(service_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    let (entries, _) = phonebook::entries(pool, service_id).await?;
    let (applicant_number, applicant_name, _) = applicant_of(pool, card_id).await?;

    let finished = "служба закончила работу по карточке";
    Ok(entries
        .into_iter()
        .map(|e| {
            let voice = voice_of(&e.role, &applicant_name).into();
            match e.role.as_str() {
                "brigade_head" | "brigade_specialist" => Contact {
                    title: title_of(&e.role, &e.title, brigade_no),
                    peer: e.role,
                    number: e.number,
                    available: brigade_no.is_some() && !closed,
                    note: if brigade_no.is_none() {
                        Some("бригада не назначена".into())
                    } else if closed {
                        Some(finished.into())
                    } else {
                        None
                    },
                    voice,
                },
                "response_head" => Contact {
                    peer: e.role,
                    title: e.title,
                    number: e.number,
                    // Начатый доклад договорить можно и после конца работ
                    available: !closed || chief_open,
                    note: (closed && !chief_open).then(|| finished.into()),
                    voice,
                },
                // Заявитель — по номеру из карточки: преподаватель включил
                // его в перечень, а диспетчер набирает то, что видит в
                // карточке (рекомендации финалистам, п. 8)
                "applicant" => Contact {
                    peer: e.role,
                    title: if applicant_name.trim().is_empty() {
                        e.title
                    } else {
                        format!("{} — {}", e.title, applicant_name.trim())
                    },
                    number: applicant_number.clone(),
                    available: !applicant_number.is_empty(),
                    note: applicant_number.is_empty().then(|| "в карточке нет номера заявителя".into()),
                    voice,
                },
                _ => Contact {
                    peer: e.role,
                    title: e.title,
                    number: e.number,
                    available: !closed,
                    note: closed.then(|| finished.into()),
                    voice,
                },
            }
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/cards/{id}/phone",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор карточки"), ServiceQuery),
    responses(
        (status = 200, description = "Справочник и журнал вызовов по карточке", body = PhoneView),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Не диспетчер ДДС"),
        (status = 404, description = "Карточка не найдена"),
    ),
)]
pub async fn view(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<ServiceQuery>,
) -> AppResult<Json<PhoneView>> {
    user.require_duty(Duty::Dds)?;
    visible_card(&state.db, id, &user).await?;

    if user.role == Role::Student {
        let service_id = dispatcher_service(&state.db, id, &user).await?;
        let rows: Vec<CallRow> = sqlx::query_as(&format!(
            "{CALL_SELECT} WHERE pc.card_id = $1 AND pc.user_id = $2 ORDER BY pc.started_at DESC"
        ))
        .bind(id)
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
        return Ok(Json(PhoneView {
            extension: Some(base_number(&state.db, &service_id).await?.to_string()),
            phonebook: phonebook(&state.db, id, &service_id, user.id).await?,
            calls: calls(&state, rows).await?,
        }));
    }

    // Преподаватель: все вызовы по карточке, при желании — одной службы
    let rows: Vec<CallRow> = sqlx::query_as(&format!(
        "{CALL_SELECT} WHERE pc.card_id = $1 AND ($2::text IS NULL OR pc.service_id = $2)
          ORDER BY pc.started_at DESC"
    ))
    .bind(id)
    .bind(q.service_id.filter(|s| !s.is_empty()))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(PhoneView { extension: None, phonebook: Vec::new(), calls: calls(&state, rows).await? }))
}

// ─── Вызов ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewCall {
    /// Роль абонента из справочника службы (`brigade_head`, `response_head`,
    /// `org_head`, `brigade_specialist`, `applicant`); прежние `brigade` и
    /// `chief` принимаются как начальник бригады и начальник отдела
    pub peer: String,
    /// `in` — входящий (звонит начальник бригады), `out` — диспетчер набрал
    pub direction: String,
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/phone/calls",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = NewCall,
    responses(
        (status = 201, description = "Вызов начат: звонит", body = CallView),
        (status = 400, description = "Неизвестный собеседник или звонить некому"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Звонит только диспетчер своей службы"),
        (status = 409, description = "Служба закончила работу по карточке"),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<NewCall>,
) -> AppResult<(StatusCode, Json<CallView>)> {
    user.require_duty(Duty::Dds)?;
    visible_card(&state.db, id, &user).await?;
    let service_id = dispatcher_service(&state.db, id, &user).await?;

    if !matches!(req.direction.as_str(), "in" | "out") {
        return Err(AppError::Validation("направление — in или out".into()));
    }
    let peer = role_of(&req.peer).to_string();
    // Сам звонит только начальник бригады с докладом; начальники и
    // заявитель диспетчеру не звонят — доклад и уточнение начинает он
    if peer != "brigade_head" && req.direction == "in" {
        return Err(AppError::Validation("сам звонит только начальник бригады — остальным звонит диспетчер".into()));
    }
    let contact = phonebook(&state.db, id, &service_id, user.id)
        .await?
        .into_iter()
        .find(|c| c.peer == peer)
        .ok_or_else(|| AppError::Validation("такого абонента нет в справочнике службы".into()))?;
    if !contact.available {
        let why = contact.note.unwrap_or_else(|| "звонить некому".into());
        return Err(if why.starts_with("служба закончила") {
            AppError::Conflict(why)
        } else {
            AppError::Validation(why)
        });
    }

    let mut tx = state.db.begin().await?;
    // Брошенные вызовы (закрыли вкладку посреди разговора) закрываются:
    // на одной линии одновременно один разговор
    sqlx::query(
        "UPDATE phone_calls
            SET outcome = CASE outcome WHEN 'ringing' THEN 'missed' ELSE 'ended' END,
                ended_at = now()
          WHERE user_id = $1 AND outcome IN ('ringing', 'talking')",
    )
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    let call_id: Uuid = sqlx::query_scalar(
        "INSERT INTO phone_calls (card_id, user_id, service_id, peer, direction, number, title)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(id)
    .bind(user.id)
    .bind(&service_id)
    .bind(&peer)
    .bind(&req.direction)
    .bind(&contact.number)
    // Без «№ бригады»: он добавляется при показе по текущему назначению
    .bind(contact.title.split(" № ").next().unwrap_or(&contact.title))
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    audit::log(
        &state.db,
        audit::Event::new("phone.call")
            .from_user(&user)
            .about("card", id)
            .with(json!({
                "callId": call_id, "serviceId": service_id, "peer": peer,
                "direction": req.direction, "number": contact.number,
            })),
    )
    .await;

    Ok((StatusCode::CREATED, Json(one(&state, call_id).await?)))
}

/// Вызов курсанта: чужой — 404, как будто его нет.
async fn own_call(pool: &PgPool, call_id: Uuid, user: &AuthUser) -> AppResult<(Uuid, String)> {
    let row: Option<(Uuid, Uuid, String)> =
        sqlx::query_as("SELECT user_id, card_id, outcome FROM phone_calls WHERE id = $1")
            .bind(call_id)
            .fetch_optional(pool)
            .await?;
    match row {
        Some((owner, card_id, outcome)) if owner == user.id => Ok((card_id, outcome)),
        _ => Err(AppError::NotFound),
    }
}

#[utoipa::path(
    post,
    path = "/api/phone/calls/{id}/answer",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор вызова")),
    responses(
        (status = 200, description = "Трубку сняли, идёт разговор", body = CallView),
        (status = 404, description = "Вызова нет"),
        (status = 409, description = "Вызов уже не звонит"),
    ),
)]
pub async fn answer(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CallView>> {
    own_call(&state.db, id, &user).await?;
    let done = sqlx::query(
        "UPDATE phone_calls SET outcome = 'talking', answered_at = now()
          WHERE id = $1 AND outcome = 'ringing'",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::Conflict("вызов уже не звонит".into()));
    }
    Ok(Json(one(&state, id).await?))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EndCall {
    /// Для звонящего вызова: `declined` — сбросили, иначе — пропущен
    /// (не ответили или не дождались ответа)
    #[serde(default)]
    pub outcome: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/phone/calls/{id}/end",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор вызова")),
    request_body = EndCall,
    responses(
        (status = 200, description = "Вызов окончен", body = CallView),
        (status = 404, description = "Вызова нет"),
    ),
)]
pub async fn end(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<EndCall>>,
) -> AppResult<Json<CallView>> {
    let (card_id, before) = own_call(&state.db, id, &user).await?;
    let declined = body.and_then(|b| b.0.outcome).as_deref() == Some("declined");
    // Повторное «положить трубку» ничего не меняет
    let updated = sqlx::query(
        "UPDATE phone_calls
            SET outcome = CASE WHEN outcome = 'talking' THEN 'ended'
                               WHEN $2 THEN 'declined' ELSE 'missed' END,
                ended_at = now()
          WHERE id = $1 AND outcome IN ('ringing', 'talking')",
    )
    .bind(id)
    .bind(declined)
    .execute(&state.db)
    .await?;
    let view = one(&state, id).await?;
    if updated.rows_affected() > 0 {
        audit::log(
            &state.db,
            audit::Event::new("phone.end")
                .from_user(&user)
                .about("card", card_id)
                .with(json!({ "callId": id, "was": before, "outcome": view.outcome })),
        )
        .await;
    }
    Ok(Json(view))
}

// ─── Реплики и запись ───────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewLine {
    /// `dispatcher` или `peer`
    pub who: String,
    pub text: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LineCreated {
    pub id: i64,
}

#[utoipa::path(
    post,
    path = "/api/phone/calls/{id}/lines",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор вызова")),
    request_body = NewLine,
    responses(
        (status = 201, description = "Реплика записана в журнал разговора", body = LineCreated),
        (status = 400, description = "Пустая или слишком длинная реплика"),
        (status = 404, description = "Вызова нет"),
        (status = 409, description = "Разговора не было: вызов не принят"),
    ),
)]
pub async fn add_line(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<NewLine>,
) -> AppResult<(StatusCode, Json<LineCreated>)> {
    let (_, outcome) = own_call(&state.db, id, &user).await?;
    // Ответ собеседника может прийти, когда трубку уже положили
    if !matches!(outcome.as_str(), "talking" | "ended") {
        return Err(AppError::Conflict("разговора не было: вызов не принят".into()));
    }
    if !matches!(req.who.as_str(), "dispatcher" | "peer") {
        return Err(AppError::Validation("кто говорит — dispatcher или peer".into()));
    }
    let text = req.text.trim();
    if text.is_empty() {
        return Err(AppError::Validation("реплика пустая".into()));
    }
    if text.chars().count() > MAX_LINE_CHARS {
        return Err(AppError::Validation(format!("реплика длиннее {MAX_LINE_CHARS} знаков")));
    }
    let line: i64 = sqlx::query_scalar(
        "INSERT INTO phone_call_lines (call_id, who, text) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(id)
    .bind(&req.who)
    .bind(text)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(LineCreated { id: line })))
}

// ─── Реплики абонентов, у которых нет своего модуля ─────────────────

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplyRequest {
    /// Что сказал диспетчер. Нет — абонент только снял трубку
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    /// Что ответил абонент
    pub text: String,
    /// Абонент сказал всё и кладёт трубку
    pub finished: bool,
}

/// Сколько секунд ждём модель с репликой заявителя: разговор по
/// телефону не должен вставать.
const APPLICANT_BUDGET: std::time::Duration = std::time::Duration::from_secs(12);

/// Заявитель по карточке: повторяет то, что сообщил в 112, — описание
/// карточки, и ничего сверх него. Модель, если есть, говорит это своими
/// словами с той же проверкой, что у живого заявителя оператора 112:
/// числа только из карточки, второй запрос подтверждает, что ничего не
/// добавлено. Не прошла — заготовленная фраза.
async fn applicant_reply(
    state: &AppState,
    name: &str,
    description: &str,
    said: Option<&str>,
    recent: Vec<(String, String)>,
    turn: usize,
) -> String {
    let female = free_question::applicant_female(name);
    let (a, la) = if female { ("а", "ла") } else { ("", "л") };
    let what = description.trim();
    let what = if what.is_empty() { "то, что уже сообщал в 112".to_string() } else { what.to_string() };
    let fallback = match (said, turn) {
        (None, _) => format!("Да, слушаю. Это я звонил{a} в 112: {what}"),
        (Some(_), 0) => format!("Я уже всё сказа{la} оператору: {what} Когда вас ждать?"),
        _ => "Ничего нового, всё как говорил. Жду вас.".replace("говорил", if female { "говорила" } else { "говорил" }),
    };
    let Some(question) = said else { return fallback };
    if state.config.ml_provider == crate::config::MlProvider::Mock {
        return fallback;
    }
    let ctx = live_applicant::Context {
        name: if name.trim().is_empty() { "заявитель".into() } else { name.trim().into() },
        status: "заявитель, уже сообщил о происшествии в 112; теперь ему перезвонил диспетчер службы".into(),
        legend: what.clone(),
        revealed: Vec::new(),
        current: Some(what),
        recent,
        question: question.into(),
    };
    match live_applicant::reply(state.ml.llm.as_ref(), &ctx, APPLICANT_BUDGET).await {
        live_applicant::Outcome::Live(text) => text,
        live_applicant::Outcome::Rejected { reason, reply } => {
            tracing::info!(reason, reply, "реплика заявителя по телефону отклонена");
            fallback
        }
        live_applicant::Outcome::Fallback(reason) => {
            tracing::info!(reason, "реплика заявителя по телефону — заготовленная");
            fallback
        }
    }
}

/// Руководитель организации принимает доклад. Ничего сверх карточки он
/// не знает, поэтому только слушает, один раз уточняет и принимает.
fn org_head_reply(title: &str, said: Option<&str>, turn: usize) -> (String, bool) {
    match (said, turn) {
        (None, _) => (format!("{title} слушает. Докладывайте."), false),
        (Some(_), 0) => ("Понял вас. Что по силам и срокам, и нужна ли помощь?".into(), false),
        _ => ("Принял доклад. Держите меня в курсе.".into(), true),
    }
}

#[utoipa::path(
    post,
    path = "/api/phone/calls/{id}/reply",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор вызова")),
    request_body = ReplyRequest,
    responses(
        (status = 200, description = "Реплика абонента; она же записана в журнал разговора", body = Reply),
        (status = 400, description = "Этот абонент отвечает через бригаду или доклад начальнику"),
        (status = 404, description = "Вызова нет"),
        (status = 409, description = "Разговора нет: вызов не принят или окончен"),
    ),
)]
pub async fn reply(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<ReplyRequest>>,
) -> AppResult<Json<Reply>> {
    let (card_id, outcome) = own_call(&state.db, id, &user).await?;
    if outcome != "talking" {
        return Err(AppError::Conflict("разговора нет: вызов не принят".into()));
    }
    let (peer, title): (String, String) =
        sqlx::query_as("SELECT peer, title FROM phone_calls WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let said = body
        .and_then(|b| b.0.text)
        .map(|t| t.trim().chars().take(MAX_LINE_CHARS).collect::<String>())
        .filter(|t| !t.is_empty());

    // Сколько раз диспетчер уже говорил — не считая реплики этого запроса,
    // которую клиент записал только что
    let lines: Vec<(String, String)> =
        sqlx::query_as("SELECT who, text FROM phone_call_lines WHERE call_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(&state.db)
            .await?;
    let turn = lines.iter().filter(|l| l.0 == "dispatcher").count().saturating_sub(usize::from(said.is_some()));

    let (text, finished) = match role_of(&peer) {
        "org_head" => org_head_reply(&title, said.as_deref(), turn),
        "applicant" => {
            let (_, name, description) = applicant_of(&state.db, card_id).await?;
            let recent = lines
                .iter()
                .rev()
                .take(live_applicant::RECENT_LINES)
                .rev()
                .map(|(who, t)| ((if who == "peer" { "Заявитель" } else { "Диспетчер" }).to_string(), t.clone()))
                .collect();
            (applicant_reply(&state, &name, &description, said.as_deref(), recent, turn).await, false)
        }
        _ => {
            return Err(AppError::Validation(
                "этот абонент отвечает через бригаду или доклад начальнику".into(),
            ))
        }
    };
    sqlx::query("INSERT INTO phone_call_lines (call_id, who, text) VALUES ($1, 'peer', $2)")
        .bind(id)
        .bind(&text)
        .execute(&state.db)
        .await?;
    Ok(Json(Reply { text, finished }))
}

#[utoipa::path(
    put,
    path = "/api/phone/calls/{id}/lines/{line}/audio",
    tag = "IP-телефон",
    params(
        ("id" = String, Path, description = "Идентификатор вызова"),
        ("line" = i64, Path, description = "Реплика диспетчера"),
    ),
    request_body(content = Vec<u8>, content_type = "audio/wav", description = "Голос диспетчера, WAV"),
    responses(
        (status = 204, description = "Голос сохранён; запись выключена — не сохранён"),
        (status = 400, description = "Не WAV или реплика не диспетчера"),
        (status = 404, description = "Вызова или реплики нет"),
    ),
)]
pub async fn line_audio(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, line)): Path<(Uuid, i64)>,
    body: Bytes,
) -> AppResult<StatusCode> {
    own_call(&state.db, id, &user).await?;
    let who: Option<String> =
        sqlx::query_scalar("SELECT who FROM phone_call_lines WHERE id = $1 AND call_id = $2")
            .bind(line)
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    match who.as_deref() {
        None => return Err(AppError::NotFound),
        Some("dispatcher") => {}
        Some(_) => return Err(AppError::Validation("голос сохраняется только у реплик диспетчера".into())),
    }
    if phone_audio::parse(&body).is_none_or(|p| p.samples.is_empty()) {
        return Err(AppError::Validation("нужен WAV с речью".into()));
    }
    // Запись выключена администратором — голос не храним, разговор
    // остаётся текстом
    if setting(&state.db, "phone.record", 1.0).await < 0.5 {
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query("UPDATE phone_call_lines SET audio = $3 WHERE id = $1 AND call_id = $2")
        .bind(line)
        .bind(id)
        .bind(body.as_ref())
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/phone/calls/{id}/recording",
    tag = "IP-телефон",
    params(("id" = String, Path, description = "Идентификатор вызова")),
    responses(
        (status = 200, description = "Запись разговора, WAV 16 кГц моно", content_type = "audio/wav", body = Vec<u8>),
        (status = 401, description = "Требуется вход"),
        (status = 404, description = "Вызова нет"),
        (status = 409, description = "Записывать нечего: ни голоса, ни синтеза"),
    ),
)]
pub async fn recording(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Response> {
    // Слушает сам диспетчер и преподаватель
    if user.role == Role::Student {
        own_call(&state.db, id, &user).await?;
    }
    let call = one(&state, id).await?;
    let (peer_voice, number, started_at) = (call.voice, call.number, call.started_at);

    let lines: Vec<(String, String, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT who, text, audio FROM phone_call_lines WHERE call_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let tts = state.ml.tts.available();
    let mut parts = Vec::new();
    for (who, text, audio) in lines {
        if let Some(pcm) = audio.as_deref().and_then(phone_audio::parse) {
            parts.push(pcm);
            continue;
        }
        if !tts {
            continue;
        }
        // Реплика без своего звука — голосом её роли: собеседник своим,
        // набранное диспетчером — голосом диспетчера
        let voice = if who == "peer" { peer_voice.as_str() } else { "dispatcher" };
        match state.ml.tts.wav(&text, voice).await {
            Ok(wav) => parts.extend(phone_audio::parse(&wav)),
            Err(e) => tracing::warn!(error = %e, "реплика не озвучена для записи разговора"),
        }
    }
    if parts.is_empty() {
        return Err(AppError::Conflict("записывать нечего: голос не сохранён, синтез не подключён".into()));
    }

    let wav = phone_audio::encode(&phone_audio::join(&parts), phone_audio::RATE);
    let name = format!("vyzov-{number}-{}.wav", started_at.format("%Y%m%d-%H%M%S"));
    Ok((
        [
            (header::CONTENT_TYPE, "audio/wav".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        wav,
    )
        .into_response())
}
