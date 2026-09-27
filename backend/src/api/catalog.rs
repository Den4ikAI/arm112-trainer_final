//! Справочники: типы происшествия, службы, опросные карты.
//!
//! Фронтенд держит их в кэше и перезапрашивает, когда меняется активная
//! версия классификатора. Отсюда заголовок `ETag`.

use crate::{domain::applicant::STATUSES as APPLICANT_STATUSES, error::AppResult, state::AppState};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/catalog/incident-types", get(incident_types))
        .route("/api/catalog/services", get(services))
        .route("/api/catalog/survey-cards/{type_id}", get(survey_card))
        .route("/api/catalog/applicant-statuses", get(applicant_statuses))
        .route("/api/classifier/evaluate", axum::routing::post(evaluate))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct IncidentType {
    id: String,
    title: String,
    /// Показывать плашкой на пустой карточке
    frequent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_no: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/catalog/incident-types",
    tag = "Справочники",
    responses(
        (status = 200, description = "Типы происшествия", body = Vec<IncidentType>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn incident_types(State(state): State<AppState>) -> AppResult<Json<Vec<IncidentType>>> {
    // Частые идут первыми — так же, как на кадре пустой карточки
    let rows = sqlx::query_as::<_, IncidentType>(
        "SELECT id, title, frequent, group_no FROM incident_types
          ORDER BY frequent DESC, title",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct Service {
    id: String,
    /// Как в окне «Добавьте службы» боевой системы, если название снято
    /// с кадров; иначе — заголовок столбца классификатора
    title: String,
    /// Как на плашке в полосе оповещения: «Служба 101», «Деп. ЖКХ»
    short_title: String,
    /// Заголовок столбца в XLSX классификатора: «Классификатор МЧС»
    source_title: Option<String>,
    /// Слова, по которым службу тоже находит поиск: «скорая», «02»
    aliases: Vec<String>,
    is_primary: bool,
}

#[utoipa::path(
    get,
    path = "/api/catalog/services",
    tag = "Справочники",
    responses(
        (status = 200, description = "Службы", body = Vec<Service>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn services(State(state): State<AppState>) -> AppResult<Json<Vec<Service>>> {
    let rows = sqlx::query_as::<_, Service>(
        "SELECT s.id, s.title,
                COALESCE(n.short_title, s.title) AS short_title,
                s.source_title,
                COALESCE(n.aliases, '{}') AS aliases,
                s.is_primary
           FROM services s
           LEFT JOIN service_names n ON n.service_id = s.id
          ORDER BY s.title",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct Question {
    id: String,
    label: String,
    kind: String,
    /// Всегда присутствует, у свободного ввода — пустой массив.
    /// Пропускать поле нельзя: клиенту пришлось бы сторожить каждое
    /// обращение, а забытая проверка роняет весь экран карточки.
    options: Vec<String>,
    /// Вопрос, от ответа на который зависят варианты этого. У карт,
    /// выведенных из классификатора, второй признак зависит от первого:
    /// у ДТП одни варианты, у «ДТП пострадавшие» — другие
    depends_on: Option<String>,
    /// Вариант → при каких ответах на `depends_on` он показывается.
    /// Варианта нет в словаре — показывается всегда
    option_parents: std::collections::BTreeMap<String, Vec<String>>,
    /// Когда вопрос виден целиком. Достаточно одного выполненного
    /// условия; пустой список — виден всегда. Карта боевой системы
    /// растёт по мере ответов: на пустом «Где» у пожара только «Где»
    /// и «Описание», а «Описание правонарушения» появляется лишь после
    /// «Есть правонарушение»
    show_when: Vec<ShowWhen>,
}

/// Условие показа вопроса: ответ на `question` содержит одно из `values`.
#[derive(Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct ShowWhen {
    question: String,
    values: Vec<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct SurveyCard {
    type_id: String,
    title: String,
    questions: Vec<Question>,
    /// Откуда взята карта: со скриншота боевой системы или выведена
    /// из классификатора. Фронтенд может это показывать преподавателю.
    source: &'static str,
}

#[utoipa::path(
    get,
    path = "/api/catalog/survey-cards/{type_id}",
    tag = "Справочники",
    params(
        ("type_id" = String, Path, description = "Идентификатор типа происшествия"),
    ),
    responses(
        (status = 200, description = "Опросная карта типа", body = SurveyCard),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn survey_card(
    State(state): State<AppState>,
    Path(type_id): Path<String>,
) -> AppResult<Response> {
    let title: Option<String> =
        sqlx::query_scalar("SELECT title FROM incident_types WHERE id = $1")
            .bind(&type_id)
            .fetch_optional(&state.db)
            .await?;

    let Some(title) = title else {
        return Err(crate::error::AppError::NotFound);
    };

    type Row = (String, String, String, bool, Option<String>, Option<serde_json::Value>);
    let questions: Vec<Row> = sqlx::query_as(
        "SELECT id, label, kind::text, derived, depends_on, show_when FROM survey_questions
          WHERE type_id = $1 ORDER BY sort_order",
    )
    .bind(&type_id)
    .fetch_all(&state.db)
    .await?;

    // Карты нет — это не ошибка: у группы классификатора может не
    // оказаться ни одной позиции, видимой оператору. Отвечаем 204,
    // чтобы фронтенд отличал «нет карты» от «нет такого типа»
    if questions.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }

    let derived = questions.iter().any(|q| q.3);
    let mut out = Vec::with_capacity(questions.len());
    for (id, label, kind, _, depends_on, show_when) in questions {
        let rows: Vec<(String, Option<Vec<String>>)> = sqlx::query_as(
            "SELECT value, parents FROM survey_options
              WHERE type_id = $1 AND question_id = $2 ORDER BY sort_order",
        )
        .bind(&type_id)
        .bind(&id)
        .fetch_all(&state.db)
        .await?;

        let mut options = Vec::with_capacity(rows.len());
        let mut option_parents = std::collections::BTreeMap::new();
        for (value, parents) in rows {
            if let Some(p) = parents {
                option_parents.insert(value.clone(), p);
            }
            options.push(value);
        }

        // Условие хранится JSON-ом; кривое — это ошибка данных, а не
        // повод прятать вопрос: показываем его всегда и пишем в журнал
        let show_when: Vec<ShowWhen> = match show_when {
            None => Vec::new(),
            Some(v) => serde_json::from_value(v).unwrap_or_else(|e| {
                tracing::warn!(type_id = %type_id, question = %id, error = %e,
                               "условие показа вопроса не разобралось");
                Vec::new()
            }),
        };

        out.push(Question { id, label, kind, options, depends_on, option_parents, show_when });
    }

    // Откуда карта: со скриншота — формулировки дословные, из
    // классификатора — структура верная, подписи вопросов общие
    let source = if derived { "classifier" } else { "screenshot" };

    Ok(Json(SurveyCard { type_id, title, questions: out, source }).into_response())
}

#[utoipa::path(
    get,
    path = "/api/catalog/applicant-statuses",
    tag = "Справочники",
    responses(
        (status = 200, description = "Статусы заявителя", body = Vec<String>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn applicant_statuses() -> (HeaderMap, Json<[&'static str; 6]>) {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "public, max-age=86400".parse().unwrap());
    (headers, Json(APPLICANT_STATUSES))
}


// ─── Движок эталона ──────────────────────────────────────────────────

use crate::domain::classify::{classify, ClassifyResult};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateRequest {
    pub type_ids: Vec<String>,
    /// questionId → выбранные варианты
    #[serde(default)]
    pub answers: HashMap<String, Vec<String>>,
}

/// Горячий путь: дёргается после каждого нажатия плашки признака.
/// Бюджет p95 — 50 мс.
#[utoipa::path(
    post,
    path = "/api/classifier/evaluate",
    tag = "Классификатор",
    responses(
        (status = 200, description = "Подбор служб по признакам", body = ClassifyResult),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
async fn evaluate(
    State(state): State<AppState>,
    Json(req): Json<EvaluateRequest>,
) -> AppResult<Json<ClassifyResult>> {
    let version_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM classifier_versions
          ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;

    let Some(version_id) = version_id else {
        return Err(crate::error::AppError::Conflict(
            "в базе нет ни одной версии классификатора".into(),
        ));
    };

    let result = classify(&state.db, version_id, &req.type_ids, &req.answers).await?;
    Ok(Json(result))
}
