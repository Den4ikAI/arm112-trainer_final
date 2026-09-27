//! Журнал карточек и режим ДДС.
//!
//! Курсант в роли диспетчера видит только карточки своей службы —
//! требование ТЗ «чтобы в их ленту попадали только профильные
//! события». Фильтр стоит в SQL, а не в интерфейсе: иначе чужую
//! карточку можно достать прямым запросом.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    domain::response,
    error::{AppError, AppResult},
    state::AppState,
    ws,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards", get(list))
        .route("/api/cards/{id}", get(one))
        .route("/api/cards/{id}/responses", post(add_response))
        .route("/api/cards/{id}/opened", post(opened))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// `service` — только профильные для своей ДДС; `own` — свои;
    /// `all` — все (преподавателю и администратору)
    #[serde(default = "default_scope")]
    pub scope: String,
    pub status: Option<String>,
    /// Поиск по номеру, адресу и описанию
    pub q: Option<String>,
    #[serde(default)]
    pub page: i64,
    #[serde(default = "default_size")]
    pub size: i64,
}

fn default_scope() -> String {
    "service".into()
}
fn default_size() -> i64 {
    50
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CardBrief {
    pub id: Uuid,
    pub number: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub saved_at: Option<chrono::DateTime<chrono::Utc>>,
    pub address: Value,
    pub description: String,
    pub outcome_type: Option<String>,
    pub status: String,
    pub operator: String,
    /// Когда карточка поступила в ленту: в занятии ДДС — когда её
    /// выпустил поток занятия, иначе — когда оператор 112 её сохранил.
    /// От этого момента считаются нормативы ДДС
    pub arrived_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Секунды с поступления. Нормативы (ответ заказчика в чате 27.09):
    /// 30 секунд на открытие, 3 минуты на первую запись
    pub waiting_sec: Option<i64>,
    /// Своя служба курсанта открыла карточку. `None` — у пользователя
    /// нет службы или она не в списке оповещения
    pub opened: Option<bool>,
    /// Своя служба сделала первую запись — содержательный статус
    pub recorded: Option<bool>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListResponse {
    pub items: Vec<CardBrief>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

#[utoipa::path(
    get,
    path = "/api/cards",
    tag = "Журнал",
    responses(
        (status = 200, description = "Лента карточек", body = ListResponse),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<ListResponse>> {
    // Чужую ленту курсанту не показываем, даже если он попросит
    let scope = match (q.scope.as_str(), user.role) {
        ("all", Role::Student) => return Err(AppError::Forbidden),
        (s, _) => s,
    };
    if scope == "service" && user.service_id.is_none() {
        return Err(AppError::Validation(
            "у пользователя не задана служба, лента по службе недоступна".into(),
        ));
    }

    let size = q.size.clamp(1, 200);
    let offset = q.page.max(0) * size;
    let pattern = q.q.as_ref().map(|s| format!("%{}%", s.trim().to_lowercase()));

    // Пока по группе курсанта идёт занятие в режиме ДДС, в его ленте
    // только карточки этого занятия: материал выбирает преподаватель.
    // Раньше карточки раскладывались в `lesson_cards`, но лента их не
    // учитывала, и занятие на экран курсанта никак не влияло
    let lesson: Option<Uuid> = if user.role == Role::Student {
        sqlx::query_scalar(
            "SELECT l.id FROM lessons l
               JOIN group_members m ON m.group_id = l.group_id
              WHERE m.user_id = $1 AND l.mode = 'dds'
                AND l.started_at IS NOT NULL AND l.finished_at IS NULL
              ORDER BY l.started_at DESC
              LIMIT 1",
        )
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
    } else {
        None
    };

    // Незаполненная карточка в журнале не нужна: она ещё пишется
    //
    // Поступление — момент, от которого считаются нормативы. В занятии
    // старая карточка выходит в поток заново, и 30 секунд должны идти
    // от её появления в ленте, а не от давнего сохранения
    let sql = "
        SELECT c.id, c.number, c.created_at, c.saved_at, c.address, c.description,
               c.outcome_type, c.status::text AS status,
               COALESCE(u.fio, '') AS operator,
               arr.arrived_at,
               EXTRACT(EPOCH FROM (now() - arr.arrived_at))::bigint AS waiting_sec,
               own.opened, own.recorded
          FROM cards c
          LEFT JOIN users u ON u.id = c.created_by
          CROSS JOIN LATERAL (
                SELECT COALESCE(
                         (SELECT lc.release_at FROM lesson_cards lc
                           WHERE lc.lesson_id = $7 AND lc.card_id = c.id),
                         c.saved_at) AS arrived_at) arr
          LEFT JOIN LATERAL (
                SELECT COALESCE(bool_or(re.status <> 'Добавлена'), false) AS opened,
                       COALESCE(bool_or(re.status NOT IN ('Добавлена', 'Получена службой')),
                                false) AS recorded
                  FROM response_entries re
                 WHERE re.card_id = c.id AND re.service_id = $8) own
                 ON EXISTS (SELECT 1 FROM card_services cs
                             WHERE cs.card_id = c.id AND cs.service_id = $8)
         WHERE c.saved_at IS NOT NULL
           AND ($1::text IS NULL OR EXISTS (
                 SELECT 1 FROM card_services cs
                  WHERE cs.card_id = c.id AND cs.service_id = $1))
           AND ($2::uuid IS NULL OR c.created_by = $2)
           AND ($3::text IS NULL OR c.status::text = $3)
           AND ($4::text IS NULL OR lower(c.number) LIKE $4
                OR lower(c.description) LIKE $4
                OR lower(c.address::text) LIKE $4)
           AND ($7::uuid IS NULL OR c.id IN (
                 SELECT card_id FROM lesson_cards
                  WHERE lesson_id = $7 AND release_at <= now()))
         ORDER BY arr.arrived_at DESC
         LIMIT $5 OFFSET $6";

    let service_filter = (scope == "service").then(|| user.service_id.clone()).flatten();
    let own_filter = (scope == "own").then_some(user.id);

    let items: Vec<CardBrief> = sqlx::query_as(sql)
        .bind(&service_filter)
        .bind(own_filter)
        .bind(&q.status)
        .bind(&pattern)
        .bind(size)
        .bind(offset)
        .bind(lesson)
        .bind(user.service_id.as_deref().filter(|_| user.role == Role::Student))
        .fetch_all(&state.db)
        .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cards c
          WHERE c.saved_at IS NOT NULL
            AND ($1::text IS NULL OR EXISTS (
                  SELECT 1 FROM card_services cs
                   WHERE cs.card_id = c.id AND cs.service_id = $1))
            AND ($2::uuid IS NULL OR c.created_by = $2)
            AND ($3::text IS NULL OR c.status::text = $3)
            AND ($4::text IS NULL OR lower(c.number) LIKE $4
                 OR lower(c.description) LIKE $4
                 OR lower(c.address::text) LIKE $4)
            AND ($5::uuid IS NULL OR c.id IN (
                  SELECT card_id FROM lesson_cards
                   WHERE lesson_id = $5 AND release_at <= now()))",
    )
    .bind(&service_filter)
    .bind(own_filter)
    .bind(&q.status)
    .bind(&pattern)
    .bind(lesson)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(ListResponse { items, total, page: q.page.max(0), size }))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseEntry {
    pub id: Uuid,
    pub service_id: String,
    pub status: String,
    pub at: chrono::DateTime<chrono::Utc>,
    pub operator: String,
    pub order_no: Option<String>,
    pub comment: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceResponses {
    pub service_id: String,
    pub title: String,
    pub entries: Vec<ResponseEntry>,
    /// Что диспетчер может выбрать следующим. Считает сервер —
    /// интерфейс только рисует.
    pub allowed_next: Vec<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CardDetail {
    #[serde(flatten)]
    pub card: Value,
    pub services: Vec<ServiceResponses>,
}

#[utoipa::path(
    get,
    path = "/api/cards/{id}",
    tag = "Журнал",
    params(
        ("id" = String, Path, description = "Идентификатор карточки"),
    ),
    responses(
        (status = 200, description = "Карточка со статусами служб", body = CardDetail),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CardDetail>> {
    visible_card(&state.db, id, &user).await?;

    let card: Value = sqlx::query_scalar(
        "SELECT to_jsonb(x) FROM (
            SELECT c.id, c.number, c.created_at AS \"createdAt\", c.saved_at AS \"savedAt\",
                   c.workstation, c.phone_aon AS \"phoneAon\",
                   c.phone_provided AS \"phoneProvided\", c.phone_on_site AS \"phoneOnSite\",
                   c.applicant_name AS \"applicantName\",
                   c.applicant_status AS \"applicantStatus\",
                   c.address, c.description, c.victims,
                   c.no_contact AS \"noContact\", c.call_dropped AS \"callDropped\",
                   c.type_ids AS \"typeIds\", c.answers,
                   c.outcome_type AS \"outcomeType\", c.status::text AS status,
                   COALESCE(u.fio, '') AS operator
              FROM cards c LEFT JOIN users u ON u.id = c.created_by
             WHERE c.id = $1
         ) x",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let services: Vec<(String, String)> = sqlx::query_as(
        "SELECT cs.service_id, s.title
           FROM card_services cs JOIN services s ON s.id = cs.service_id
          WHERE cs.card_id = $1 ORDER BY cs.source, cs.service_id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(services.len());
    for (service_id, title) in services {
        let entries: Vec<ResponseEntry> = sqlx::query_as(
            "SELECT id, service_id, status::text AS status, at, operator, order_no, comment
               FROM response_entries
              WHERE card_id = $1 AND service_id = $2 ORDER BY at, id",
        )
        .bind(id)
        .bind(&service_id)
        .fetch_all(&state.db)
        .await?;

        let chain: Vec<String> = entries.iter().map(|e| e.status.clone()).collect();
        out.push(ServiceResponses {
            allowed_next: response::next_statuses(&chain),
            service_id,
            title,
            entries,
        });
    }

    Ok(Json(CardDetail { card, services: out }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewResponse {
    pub service_id: String,
    pub status: String,
    pub order_no: Option<String>,
    pub comment: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/responses",
    tag = "Журнал",
    params(
        ("id" = String, Path, description = "Идентификатор карточки"),
    ),
    responses(
        (status = 201, description = "Статус проставлен", body = ResponseEntry),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn add_response(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<NewResponse>,
) -> AppResult<(StatusCode, Json<ResponseEntry>)> {
    visible_card(&state.db, id, &user).await?;

    // Диспетчер отвечает за свою службу и ни за чью больше
    if user.role == Role::Student && user.service_id.as_deref() != Some(req.service_id.as_str()) {
        return Err(AppError::Forbidden);
    }

    let in_list: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM card_services WHERE card_id = $1 AND service_id = $2)",
    )
    .bind(id)
    .bind(&req.service_id)
    .fetch_one(&state.db)
    .await?;
    if !in_list {
        return Err(AppError::Validation(
            "служба не входит в список оповещения по этой карточке".into(),
        ));
    }

    let chain: Vec<String> = sqlx::query_scalar(
        "SELECT status::text FROM response_entries
          WHERE card_id = $1 AND service_id = $2 ORDER BY at, id",
    )
    .bind(id)
    .bind(&req.service_id)
    .fetch_all(&state.db)
    .await?;

    let allowed = response::next_statuses(&chain);
    if !allowed.contains(&req.status) {
        return Err(AppError::StatusNotAllowed { allowed });
    }

    // «Не принята» и «Отказ от выполнения работ» без объяснения
    // оставляют карточку без движения — памятка требует комментарий
    if response::requires_comment(&req.status)
        && req.comment.as_ref().is_none_or(|c| c.trim().is_empty())
    {
        return Err(AppError::CommentRequired);
    }

    let operator: String = sqlx::query_scalar("SELECT fio FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_default();

    let entry: ResponseEntry = sqlx::query_as(
        "INSERT INTO response_entries
             (card_id, service_id, status, operator, user_id, order_no, comment)
         VALUES ($1, $2, $3::response_status, $4, $5, $6, $7)
         RETURNING id, service_id, status::text AS status, at, operator, order_no, comment",
    )
    .bind(id)
    .bind(&req.service_id)
    .bind(&req.status)
    .bind(&operator)
    .bind(user.id)
    .bind(&req.order_no)
    .bind(req.comment.as_ref().map(|c| c.trim()))
    .fetch_one(&state.db)
    .await?;

    // Кто и когда проставил статус — то, что спрашивают при разборе
    // происшествия в первую очередь
    audit::log(
        &state.db,
        audit::Event::new("response.add")
            .from_user(&user)
            .about("card", id)
            .with(serde_json::json!({
                "serviceId": entry.service_id,
                "status": entry.status,
                "orderNo": entry.order_no,
                "comment": entry.comment,
            })),
    )
    .await;

    // Другие диспетчеры той же карточки и преподаватель видят
    // движение сразу, без обновления страницы
    let (session_id, services): (Option<Uuid>, Vec<String>) = sqlx::query_as(
        "SELECT c.session_id,
                array(SELECT service_id FROM card_services WHERE card_id = c.id)
           FROM cards c WHERE c.id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    state.ws.publish(ws::routes::card_event(
        "response.recorded",
        serde_json::json!({
            "cardId": id,
            "serviceId": entry.service_id,
            "status": entry.status,
            "at": entry.at,
            "operator": entry.operator,
        }),
        ws::routes::card_topics(&services, session_id),
    ));

    // Карточка считается отработанной, когда хотя бы одна служба
    // довела работы до конца
    if response::is_terminal(&req.status) {
        sqlx::query("UPDATE cards SET status = 'Отработана' WHERE id = $1 AND status = 'Зарегистрирована'")
            .bind(id)
            .execute(&state.db)
            .await?;
    }

    Ok((StatusCode::CREATED, Json(entry)))
}

/// Диспетчер открыл карточку своей службы.
///
/// Ответ заказчика в чате задачи 27.09.2026: 30 секунд даётся с момента
/// поступления карточки в список до её открытия. Чтобы норматив можно
/// было проверить, открытие оставляет системную запись «Получена
/// службой» — статус из памятки ДДС, который раньше нигде не ставился.
///
/// Идемпотентно: запись делается только при первом открытии. Открытие
/// преподавателем или курсантом чужой службы следа не оставляет —
/// норматив касается диспетчера, которому карточка адресована.
#[utoipa::path(
    post,
    path = "/api/cards/{id}/opened",
    tag = "Журнал",
    params(
        ("id" = String, Path, description = "Идентификатор карточки"),
    ),
    responses(
        (status = 204, description = "Открытие учтено или учитывать нечего"),
        (status = 401, description = "Требуется вход"),
        (status = 404, description = "Карточка не найдена"),
    ),
)]
async fn opened(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    visible_card(&state.db, id, &user).await?;

    let Some(service_id) = user.service_id.as_deref().filter(|_| user.role == Role::Student)
    else {
        return Ok(StatusCode::NO_CONTENT);
    };

    let operator: String = sqlx::query_scalar("SELECT fio FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or_default();

    // Два открытия подряд (две вкладки, повторный рендер) не должны дать
    // две записи: блокировка на пару карточка—служба до конца транзакции
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("card.opened:{id}:{service_id}"))
        .execute(&mut *tx)
        .await?;
    let at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "INSERT INTO response_entries (card_id, service_id, status, operator, user_id)
         SELECT $1, $2, 'Получена службой', $3, $4
          WHERE EXISTS (SELECT 1 FROM card_services WHERE card_id = $1 AND service_id = $2)
            AND NOT EXISTS (SELECT 1 FROM response_entries
                             WHERE card_id = $1 AND service_id = $2
                               AND status <> 'Добавлена')
         RETURNING at",
    )
    .bind(id)
    .bind(service_id)
    .bind(&operator)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    let Some(at) = at else {
        return Ok(StatusCode::NO_CONTENT);
    };

    audit::log(
        &state.db,
        audit::Event::new("card.opened")
            .from_user(&user)
            .about("card", id)
            .with(serde_json::json!({ "serviceId": service_id })),
    )
    .await;

    // Журнал преподавателя видит открытие сразу — как и любой статус
    let (session_id, services): (Option<Uuid>, Vec<String>) = sqlx::query_as(
        "SELECT c.session_id,
                array(SELECT service_id FROM card_services WHERE card_id = c.id)
           FROM cards c WHERE c.id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    state.ws.publish(ws::routes::card_event(
        "response.recorded",
        serde_json::json!({
            "cardId": id,
            "serviceId": service_id,
            "status": "Получена службой",
            "at": at,
            "operator": operator,
        }),
        ws::routes::card_topics(&services, session_id),
    ));

    Ok(StatusCode::NO_CONTENT)
}

/// Карточку видно, если она профильная для службы пользователя,
/// своя, или пользователь — преподаватель либо администратор.
/// Ответ всегда 404: по коду не должно быть видно, что карточка есть.
pub(crate) async fn visible_card(pool: &sqlx::PgPool, id: Uuid, user: &AuthUser) -> AppResult<()> {
    if user.role != Role::Student {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM cards WHERE id = $1)")
                .bind(id)
                .fetch_one(pool)
                .await?;
        return if exists { Ok(()) } else { Err(AppError::NotFound) };
    }

    let visible: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM cards c
             WHERE c.id = $1
               AND (c.created_by = $2
                    OR EXISTS (SELECT 1 FROM card_services cs
                                WHERE cs.card_id = c.id AND cs.service_id = $3)))",
    )
    .bind(id)
    .bind(user.id)
    .bind(&user.service_id)
    .fetch_one(pool)
    .await?;

    if visible { Ok(()) } else { Err(AppError::NotFound) }
}
