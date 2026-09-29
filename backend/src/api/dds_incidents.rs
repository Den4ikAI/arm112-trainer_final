//! Учебные происшествия для диспетчера ДДС.
//!
//! Учебное происшествие — это готовая карточка 112, которую собрал
//! преподаватель: адрес, заявитель, описание, тип с ответами опросной
//! карты и службы. Диспетчер получает её так же, как боевую карточку.
//!
//! Замечания команды к сдаче, п. 6 и 10: преподаватель создаёт такие
//! карточки сам — по описанию для модели (потом правит и утверждает)
//! или вручную; карточки, заполненные операторами 112 в занятиях, к
//! диспетчерам не попадают, всё выдаёт преподаватель. Поэтому у
//! карточки есть `origin`: `teacher` — учебный материал, `session` —
//! работа курсанта.
//!
//! Службы, как и у оператора 112, подбирает классификатор по типу и
//! признакам, а не модель: модель только предлагает тип и признаки из
//! закрытых словарей. Преподаватель может добавить службы вручную.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    domain::{applicant::STATUSES, classify::classify},
    error::{AppError, AppResult},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/dds-incidents", get(list).post(create))
        .route("/api/dds-incidents/from-prompt", post(create_from_prompt))
        .route("/api/dds-incidents/{id}", get(one).put(update).delete(remove))
        .route("/api/dds-incidents/{id}/approve", post(approve))
}

/// Учебное происшествие целиком — как его видит преподаватель.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: Uuid,
    pub number: String,
    /// Тема — как происшествие называется в списке и в составлении занятия
    pub title: Option<String>,
    /// `draft` — черновик, `approved` — утверждено и доступно занятиям
    pub status: String,
    pub applicant_name: String,
    pub applicant_status: String,
    pub phone_aon: String,
    pub address: Value,
    pub description: String,
    pub victims: Option<i32>,
    pub type_ids: Vec<String>,
    pub answers: Value,
    /// Итоговый тип по классификатору
    pub outcome_type: Option<String>,
    /// Все службы карточки: подобранные классификатором и добавленные
    pub services: Vec<String>,
    /// Из них добавленные преподавателем вручную
    pub manual_services: Vec<String>,
    /// Почему классификатор не подобрал службы, если не подобрал
    pub classify_reason: Option<String>,
    /// В скольких занятиях уже выдано
    pub lessons: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub approved_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Что преподаватель задаёт при сборке вручную.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IncidentInput {
    pub title: String,
    #[serde(default)]
    pub applicant_name: String,
    #[serde(default)]
    pub applicant_status: String,
    #[serde(default)]
    pub phone_aon: String,
    #[serde(default)]
    pub address: Option<Value>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub victims: Option<i32>,
    #[serde(default)]
    pub type_ids: Vec<String>,
    #[serde(default)]
    pub answers: HashMap<String, Vec<String>>,
    /// Службы сверх подобранных классификатором
    #[serde(default)]
    pub extra_services: Vec<String>,
}

type Row = (
    Uuid, String, Option<String>, Option<chrono::DateTime<chrono::Utc>>, String, String, String,
    Value, String, Option<i32>, Vec<String>, Value, Option<String>,
    chrono::DateTime<chrono::Utc>, i64,
);

const SELECT: &str = "SELECT c.id, c.number, c.title, c.approved_at, c.applicant_name,
        c.applicant_status, c.phone_aon, c.address, c.description, c.victims,
        c.type_ids, c.answers, c.outcome_type, c.created_at,
        (SELECT count(DISTINCT lc.lesson_id) FROM lesson_cards lc
           JOIN cards k ON k.id = lc.card_id
          WHERE k.id = c.id OR k.source_card_id = c.id)
   FROM cards c WHERE c.origin = 'teacher'
    -- Копии, выданные занятиями, — не учебный материал: их не видно,
    -- не правят и не удаляют (404)
    AND c.source_card_id IS NULL";

async fn load(pool: &sqlx::PgPool, id: Uuid) -> AppResult<Incident> {
    let row: Option<Row> = sqlx::query_as(&format!("{SELECT} AND c.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let row = row.ok_or(AppError::NotFound)?;
    assemble(pool, row).await
}

async fn assemble(pool: &sqlx::PgPool, row: Row) -> AppResult<Incident> {
    let (id, number, title, approved_at, applicant_name, applicant_status, phone_aon, address,
         description, victims, type_ids, answers, outcome_type, created_at, lessons) = row;
    let svc: Vec<(String, String)> = sqlx::query_as(
        "SELECT service_id, source::text FROM card_services WHERE card_id = $1 ORDER BY added_at, service_id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let classify_reason = if svc.iter().any(|(_, src)| src == "auto") || type_ids.is_empty() {
        None
    } else {
        Some("классификатор не подобрал служб: допишите признаки опросной карты".to_string())
    };
    Ok(Incident {
        id,
        number,
        title,
        status: if approved_at.is_some() { "approved" } else { "draft" }.into(),
        applicant_name,
        applicant_status,
        phone_aon,
        address,
        description,
        victims,
        type_ids,
        answers,
        outcome_type,
        services: svc.iter().map(|(s, _)| s.clone()).collect(),
        manual_services: svc.iter().filter(|(_, src)| src == "manual").map(|(s, _)| s.clone()).collect(),
        classify_reason,
        lessons,
        created_at,
        approved_at,
    })
}

/// Записать поля карточки и пересчитать службы классификатором.
async fn write(pool: &sqlx::PgPool, id: Uuid, input: &IncidentInput) -> AppResult<()> {
    let version_id = crate::api::scenarios::active_version(pool).await?;
    let result = classify(pool, version_id, &input.type_ids, &input.answers)
        .await
        .map_err(AppError::Other)?;
    let status = if STATUSES.contains(&input.applicant_status.trim()) {
        input.applicant_status.trim().to_string()
    } else {
        String::new()
    };

    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE cards SET title = NULLIF(btrim($2), ''), applicant_name = $3,
                applicant_status = $4, phone_aon = $5, address = $6, description = $7,
                victims = $8, type_ids = $9, answers = $10, outcome_type = $11
          WHERE id = $1",
    )
    .bind(id)
    .bind(&input.title)
    .bind(input.applicant_name.trim())
    .bind(status)
    .bind(input.phone_aon.trim())
    .bind(input.address.clone().unwrap_or(json!({})))
    .bind(input.description.trim())
    .bind(input.victims)
    .bind(&input.type_ids)
    .bind(json!(input.answers))
    .bind(result.outcome.as_ref())
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM card_services WHERE card_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for s in &result.services {
        sqlx::query(
            "INSERT INTO card_services (card_id, service_id, source) VALUES ($1, $2, 'auto')
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(&s.service_id)
        .execute(&mut *tx)
        .await?;
    }
    for s in &input.extra_services {
        // Только службы из окна «Добавьте службы»: скрытые столбцы
        // классификатора вручную не добавляются
        sqlx::query(
            "INSERT INTO card_services (card_id, service_id, source)
             SELECT $1, c.service_id, 'manual'::service_source
               FROM service_catalog c WHERE c.service_id = $2
             ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(s)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Выдано ли уже в занятии: такую карточку править и удалять нельзя —
/// по ней есть работа курсантов. Занятие выдаёт копию (`source_card_id`),
/// но и по копии — работа курсантов, а удалённый исходник вывел бы свои
/// копии в «Учебный материал». Выданное до копий — в `lesson_cards` само.
async fn in_use(pool: &sqlx::PgPool, id: Uuid) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM lesson_cards WHERE card_id = $1)
             OR EXISTS (SELECT 1 FROM cards WHERE source_card_id = $1)
             OR EXISTS (SELECT 1 FROM response_entries WHERE card_id = $1)",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}

#[utoipa::path(
    get,
    path = "/api/dds-incidents",
    tag = "Происшествия ДДС",
    responses(
        (status = 200, description = "Учебные происшествия ДДС, новые сверху", body = Vec<Incident>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<Incident>>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let rows: Vec<Row> = sqlx::query_as(&format!("{SELECT} ORDER BY c.created_at DESC"))
        .fetch_all(&state.db)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(assemble(&state.db, r).await?);
    }
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/dds-incidents/{id}",
    tag = "Происшествия ДДС",
    params(("id" = String, Path, description = "Идентификатор происшествия")),
    responses(
        (status = 200, description = "Учебное происшествие", body = Incident),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Нет такого"),
    ),
)]
async fn one(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> AppResult<Json<Incident>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    load(&state.db, id).await.map(Json)
}

async fn insert_blank(pool: &sqlx::PgPool, user: &AuthUser) -> AppResult<Uuid> {
    Ok(sqlx::query_scalar(
        "INSERT INTO cards (origin, created_by, workstation) VALUES ('teacher', $1, 'УМЦ')
         RETURNING id",
    )
    .bind(user.id)
    .fetch_one(pool)
    .await?)
}

#[utoipa::path(
    post,
    path = "/api/dds-incidents",
    tag = "Происшествия ДДС",
    request_body = IncidentInput,
    responses(
        (status = 201, description = "Черновик происшествия, службы подобраны классификатором", body = Incident),
        (status = 400, description = "Нет темы"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<IncidentInput>,
) -> AppResult<(StatusCode, Json<Incident>)> {
    user.require(&[Role::Teacher, Role::Admin])?;
    if input.title.trim().is_empty() {
        return Err(AppError::Validation("нужна тема происшествия".into()));
    }
    let id = insert_blank(&state.db, &user).await?;
    write(&state.db, id, &input).await?;
    audit::log(
        &state.db,
        audit::Event::new("incident.create").from_user(&user).about("card", id)
            .with(json!({ "source": "manual" })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(load(&state.db, id).await?)))
}

#[utoipa::path(
    put,
    path = "/api/dds-incidents/{id}",
    tag = "Происшествия ДДС",
    params(("id" = String, Path, description = "Идентификатор происшествия")),
    request_body = IncidentInput,
    responses(
        (status = 200, description = "Происшествие после правки", body = Incident),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Нет такого"),
        (status = 409, description = "Уже выдано в занятии — править нельзя"),
    ),
)]
async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(input): Json<IncidentInput>,
) -> AppResult<Json<Incident>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    load(&state.db, id).await?;
    if in_use(&state.db, id).await? {
        return Err(AppError::Conflict(
            "происшествие уже выдано в занятии: по нему есть работа курсантов. \
             Соберите новое на его основе"
                .into(),
        ));
    }
    if input.title.trim().is_empty() {
        return Err(AppError::Validation("нужна тема происшествия".into()));
    }
    write(&state.db, id, &input).await?;
    // Правка возвращает в черновик: утверждали другое
    sqlx::query("UPDATE cards SET approved_at = NULL, approved_by = NULL WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state.db,
        audit::Event::new("incident.edit").from_user(&user).about("card", id),
    )
    .await;
    load(&state.db, id).await.map(Json)
}

#[utoipa::path(
    post,
    path = "/api/dds-incidents/{id}/approve",
    tag = "Происшествия ДДС",
    params(("id" = String, Path, description = "Идентификатор происшествия")),
    responses(
        (status = 200, description = "Утверждено: доступно в составлении занятия", body = Incident),
        (status = 400, description = "Карточка неполна"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Нет такого"),
    ),
)]
async fn approve(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Incident>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let inc = load(&state.db, id).await?;

    // Неполная карточка учит плохому: диспетчеру нечего принимать
    let mut нет = Vec::new();
    if inc.description.trim().is_empty() {
        нет.push("описание");
    }
    if inc.type_ids.is_empty() {
        нет.push("тип происшествия");
    }
    if inc.services.is_empty() {
        нет.push("службы");
    }
    let адрес_есть = inc
        .address
        .as_object()
        .is_some_and(|m| m.values().any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())));
    if !адрес_есть {
        нет.push("адрес");
    }
    if !нет.is_empty() {
        return Err(AppError::Validation(format!("не заполнено: {}", нет.join(", "))));
    }

    // Утверждённая карточка считается «сохранённой оператором 112»:
    // от этого момента её можно выдать диспетчеру
    sqlx::query(
        "UPDATE cards SET approved_at = now(), approved_by = $2,
                saved_at = COALESCE(saved_at, now())
          WHERE id = $1",
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    audit::log(
        &state.db,
        audit::Event::new("incident.approve").from_user(&user).about("card", id),
    )
    .await;
    load(&state.db, id).await.map(Json)
}

#[utoipa::path(
    delete,
    path = "/api/dds-incidents/{id}",
    tag = "Происшествия ДДС",
    params(("id" = String, Path, description = "Идентификатор происшествия")),
    responses(
        (status = 204, description = "Удалено"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Нет такого"),
        (status = 409, description = "Уже выдано в занятии"),
    ),
)]
async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    user.require(&[Role::Teacher, Role::Admin])?;
    load(&state.db, id).await?;
    if in_use(&state.db, id).await? {
        return Err(AppError::Conflict(
            "происшествие уже выдано в занятии — удалить нельзя, по нему есть работа курсантов".into(),
        ));
    }
    sqlx::query("DELETE FROM cards WHERE id = $1 AND origin = 'teacher'")
        .bind(id)
        .execute(&state.db)
        .await?;
    audit::log(
        &state.db,
        audit::Event::new("incident.delete").from_user(&user).about("card", id),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Описание происшествия для модели.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IncidentPrompt {
    #[serde(default)]
    pub title: String,
    /// Что произошло, словами преподавателя
    pub prompt: String,
}

/// Поля карточки, которые пишет модель. Тип и признаки она выбирает
/// отдельно, из закрытых словарей, — тем же путём, что у вызовов 112.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Drafted {
    applicant_name: String,
    applicant_status: String,
    description: String,
    victims: Option<i32>,
    region: String,
    settlement: String,
    street: String,
    house: String,
    descriptive: String,
}

async fn draft_fields(state: &AppState, prompt: &str) -> anyhow::Result<Drafted> {
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["applicantName", "applicantStatus", "description", "victims",
                     "region", "settlement", "street", "house", "descriptive"],
        "properties": {
            "applicantName": { "type": "string", "maxLength": 80 },
            "applicantStatus": { "type": "string", "enum": STATUSES },
            "description": { "type": "string", "maxLength": 400 },
            "victims": { "type": ["integer", "null"] },
            "region": { "type": "string", "maxLength": 60 },
            "settlement": { "type": "string", "maxLength": 60 },
            "street": { "type": "string", "maxLength": 80 },
            "house": { "type": "string", "maxLength": 20 },
            "descriptive": { "type": "string", "maxLength": 160 },
        }
    });
    let raw = state
        .ml
        .llm
        .chat_json(
            json!([
                { "role": "system", "content":
                    "Ты помогаешь преподавателю учебного центра Системы-112 собрать учебную \
                     карточку происшествия для диспетчера ДДС. Заполни поля строго по \
                     описанию преподавателя. Не добавляй сведений, которых в описании нет: \
                     чего нет — оставь пустой строкой, число пострадавших — null. \
                     description — описание со слов заявителя так, как его записал бы \
                     оператор 112: кратко, по делу, в третьем лице. Адрес раскладывай по \
                     полям; если город не назван — Москва." },
                { "role": "user", "content": format!("Описание: {prompt}") },
            ]),
            600,
            Some(schema),
        )
        .await?;
    Ok(serde_json::from_value(raw)?)
}

#[utoipa::path(
    post,
    path = "/api/dds-incidents/from-prompt",
    tag = "Происшествия ДДС",
    request_body = IncidentPrompt,
    responses(
        (status = 201, description = "Черновик по описанию: поля пишет модель, службы — классификатор", body = Incident),
        (status = 400, description = "Пустое описание"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn create_from_prompt(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<IncidentPrompt>,
) -> AppResult<(StatusCode, Json<Incident>)> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let prompt = req.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(AppError::Validation("опишите происшествие словами".into()));
    }
    let title = if req.title.trim().is_empty() {
        prompt.chars().take(60).collect::<String>()
    } else {
        req.title.trim().to_string()
    };

    // Тип и признаки — из закрытых словарей, как у вызовов 112
    let call = crate::ingest::tickets::TicketCall {
        ticket: 0,
        call_no: 0,
        situation: prompt.clone(),
        address: String::new(),
        address_hidden: None,
        applicant_name: None,
        applicant_phone: None,
    };
    let llm = state.ml.llm.as_ref();
    let signs = async {
        let picked = crate::domain::scenario_gen::pick_signs(llm, &state.db, &call, None).await?;
        let (answers, _) =
            crate::domain::scenario_gen::keep_known_answers(&state.db, &picked.type_ids, picked.answers)
                .await?;
        anyhow::Ok((picked.type_ids, answers))
    }
    .await;
    let fields = draft_fields(&state, &prompt).await;

    let mut notes = Vec::new();
    let (type_ids, answers) = match signs {
        Ok(v) => v,
        Err(e) => {
            notes.push(format!("тип и признаки не выбраны моделью ({e})"));
            (Vec::new(), HashMap::new())
        }
    };
    let d = match fields {
        Ok(d) => d,
        Err(e) => {
            notes.push(format!("поля не заполнены моделью ({e})"));
            Drafted { description: prompt.clone(), ..Default::default() }
        }
    };
    let mut address = serde_json::Map::new();
    for (k, v) in [("region", &d.region), ("settlement", &d.settlement), ("street", &d.street),
                   ("house", &d.house), ("descriptive", &d.descriptive)] {
        if !v.trim().is_empty() {
            address.insert(k.into(), json!(v.trim()));
        }
    }
    let input = IncidentInput {
        title,
        applicant_name: d.applicant_name,
        applicant_status: d.applicant_status,
        phone_aon: String::new(),
        address: Some(Value::Object(address)),
        description: if d.description.trim().is_empty() { prompt.clone() } else { d.description },
        victims: d.victims,
        type_ids,
        answers,
        extra_services: Vec::new(),
    };
    let id = insert_blank(&state.db, &user).await?;
    write(&state.db, id, &input).await?;
    audit::log(
        &state.db,
        audit::Event::new("incident.create").from_user(&user).about("card", id)
            .with(json!({ "source": "prompt", "prompt": prompt, "notes": notes })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(load(&state.db, id).await?)))
}
