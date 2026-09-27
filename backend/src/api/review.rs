//! Разбор попытки преподавателем: автоматический балл, нейросетевой
//! анализ текста и экспертная оценка.
//!
//! ТЗ, роль преподавателя: «просматривать результаты выполнения заданий
//! (автоматическая оценка И + экспертная оценка)», «предоставлять
//! обратную связь (комментарии к результатам)». Роль администратора:
//! «не может менять оценки … во время активного занятия». Мы строже:
//! экспертную оценку ставит только преподаватель, администратор её
//! только видит. Учебный процесс — не его зона.
//!
//! Попытка адресуется карточкой: у оператора 112 это его учебный вызов,
//! у диспетчера ДДС — служба на карточке. Карточку открывают из журнала
//! все роли, поэтому и разбор висит на ней.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
    domain::text_analysis::{self, AnalysisView, Kind},
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
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/cards/{id}/review", get(view))
        .route("/api/cards/{id}/review/expert", post(set_expert))
        .route("/api/cards/{id}/review/verdict", post(set_verdict))
        .route("/api/cards/{id}/review/analyse", post(analyse))
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudentRef {
    pub id: Uuid,
    pub fio: String,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpertView {
    pub score: i32,
    pub comment: String,
    pub teacher: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Одна попытка: вызов 112 или работа службы ДДС по карточке.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    /// `card112` или `dds`
    pub kind: String,
    pub service_id: Option<String>,
    pub student: Option<StudentRef>,
    /// Балл правил. Экспертная оценка его не переписывает
    pub auto_score: Option<i32>,
    pub analysis: Option<AnalysisView>,
    pub expert: Option<ExpertView>,
    /// Доклад начальнику — только у попыток ДДС, последний звонок
    /// этого курсанта. Балл правил он не меняет: разбор идёт рядом
    pub chief: Option<crate::domain::chief::ChiefView>,
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CardReview {
    pub attempts: Vec<Attempt>,
    /// Может ли смотрящий ставить экспертную оценку и вердикты
    pub can_grade: bool,
}

/// Кто вёл вызов 112 по карточке, если он разобран.
async fn card112_attempt(pool: &PgPool, card_id: Uuid) -> AppResult<Option<(StudentRef, i32)>> {
    let row: Option<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT u.id, u.fio, gr.score
           FROM cards c
           JOIN sessions s ON s.id = c.session_id
           JOIN users u ON u.id = s.user_id
           JOIN grades gr ON gr.session_id = s.id
          WHERE c.id = $1",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id, fio, score)| (StudentRef { id, fio }, score)))
}

/// Службы, где курсант что-то сделал по карточке, и кто именно.
/// Последний действовавший — тот, чья это попытка.
async fn dds_attempts(pool: &PgPool, card_id: Uuid) -> AppResult<Vec<(String, StudentRef)>> {
    let rows: Vec<(String, Uuid, String)> = sqlx::query_as(
        "SELECT DISTINCT ON (r.service_id) r.service_id, u.id, u.fio
           FROM response_entries r
           JOIN users u ON u.id = r.user_id
          WHERE r.card_id = $1 AND u.role = 'student'
            AND r.status::text NOT IN ('Добавлена', 'Получена службой')
          ORDER BY r.service_id, r.at DESC, r.id DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(sv, id, fio)| (sv, StudentRef { id, fio })).collect())
}

async fn expert(pool: &PgPool, kind: Kind, card_id: Uuid, service_id: Option<&str>) -> AppResult<Option<ExpertView>> {
    let row: Option<(i32, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT e.score, e.comment, u.fio, e.updated_at
           FROM expert_grades e JOIN users u ON u.id = e.teacher_id
          WHERE e.kind = $1 AND e.card_id = $2 AND e.service_id IS NOT DISTINCT FROM $3",
    )
    .bind(kind.as_str())
    .bind(card_id)
    .bind(service_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(score, comment, teacher, updated_at)| ExpertView { score, comment, teacher, updated_at }))
}

/// Экспертная оценка вызова 112 — для отчётов по занятию.
pub async fn expert_for_session(pool: &PgPool, session_id: Uuid) -> AppResult<Option<ExpertView>> {
    let card_id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM cards WHERE session_id = $1")
        .bind(session_id)
        .fetch_optional(pool)
        .await?;
    match card_id {
        Some(card_id) => expert(pool, Kind::Card112, card_id, None).await,
        None => Ok(None),
    }
}

/// Все попытки по карточке, которые смотрящему положено видеть.
async fn attempts(pool: &PgPool, card_id: Uuid, user: &AuthUser) -> AppResult<Vec<Attempt>> {
    let mut out = Vec::new();

    if let Some((student, score)) = card112_attempt(pool, card_id).await? {
        // Курсант видит только свой разбор: оценки соседа — не его дело
        if user.role != Role::Student || student.id == user.id {
            out.push(Attempt {
                kind: Kind::Card112.as_str().into(),
                service_id: None,
                auto_score: Some(score),
                analysis: text_analysis::view(pool, Kind::Card112, card_id, None)
                    .await
                    .map_err(AppError::Other)?,
                expert: expert(pool, Kind::Card112, card_id, None).await?,
                chief: None,
                student: Some(student),
            });
        }
    }

    for (service_id, student) in dds_attempts(pool, card_id).await? {
        if user.role == Role::Student && student.id != user.id {
            continue;
        }
        out.push(Attempt {
            kind: Kind::Dds.as_str().into(),
            auto_score: crate::api::brigade::score_for(pool, card_id, &service_id).await?,
            analysis: text_analysis::view(pool, Kind::Dds, card_id, Some(&service_id))
                .await
                .map_err(AppError::Other)?,
            expert: expert(pool, Kind::Dds, card_id, Some(&service_id)).await?,
            chief: crate::domain::chief::latest(pool, card_id, &service_id, Some(student.id))
                .await
                .map_err(AppError::Other)?,
            service_id: Some(service_id),
            student: Some(student),
        });
    }

    Ok(out)
}

#[utoipa::path(
    get,
    path = "/api/cards/{id}/review",
    tag = "Разбор",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    responses(
        (status = 200, description = "Автоматическая оценка, анализ текста и экспертная оценка", body = CardReview),
        (status = 401, description = "Требуется вход"),
        (status = 404, description = "Карточка не найдена или недоступна"),
    ),
)]
pub async fn view(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<CardReview>> {
    crate::api::cards::visible_card(&state.db, id, &user).await?;
    Ok(Json(CardReview {
        attempts: attempts(&state.db, id, &user).await?,
        can_grade: user.role == Role::Teacher,
    }))
}

/// Какую попытку трогаем.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// `card112` или `dds`
    pub kind: String,
    /// Для `dds` — служба
    pub service_id: Option<String>,
}

impl Target {
    fn parse(&self) -> AppResult<(Kind, Option<&str>)> {
        let kind = Kind::parse(&self.kind)
            .ok_or_else(|| AppError::Validation("вид попытки — «card112» или «dds»".into()))?;
        let service_id = self.service_id.as_deref().filter(|s| !s.is_empty());
        match (kind, service_id) {
            (Kind::Card112, None) | (Kind::Dds, Some(_)) => Ok((kind, service_id)),
            (Kind::Card112, Some(_)) => Err(AppError::Validation("у вызова 112 нет службы".into())),
            (Kind::Dds, None) => Err(AppError::Validation("для ДДС укажите службу".into())),
        }
    }
}

/// Попытка существует и разобрана правилами: оценивать то, чего нет,
/// нельзя. Возвращает автоматический балл — он уходит в журнал рядом
/// с экспертным.
async fn attempt_score(pool: &PgPool, card_id: Uuid, kind: Kind, service_id: Option<&str>) -> AppResult<Option<i32>> {
    match (kind, service_id) {
        (Kind::Card112, _) => match card112_attempt(pool, card_id).await? {
            Some((_, score)) => Ok(Some(score)),
            None => Err(AppError::Conflict("вызов по карточке ещё не сохранён и не разобран".into())),
        },
        (Kind::Dds, Some(sv)) => {
            if !dds_attempts(pool, card_id).await?.iter().any(|(s, _)| s == sv) {
                return Err(AppError::Conflict("служба ещё не работала по карточке".into()));
            }
            crate::api::brigade::score_for(pool, card_id, sv).await
        }
        (Kind::Dds, None) => Err(AppError::Validation("для ДДС укажите службу".into())),
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpertInput {
    #[serde(flatten)]
    pub target: Target,
    /// 0..100
    pub score: i32,
    #[serde(default)]
    pub comment: String,
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/review/expert",
    tag = "Разбор",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = ExpertInput,
    responses(
        (status = 200, description = "Экспертная оценка записана", body = ExpertView),
        (status = 400, description = "Балл вне 0..100 или неверная попытка"),
        (status = 403, description = "Только преподаватель"),
        (status = 409, description = "Попытка ещё не разобрана"),
    ),
)]
pub async fn set_expert(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ExpertInput>,
) -> AppResult<Json<ExpertView>> {
    user.require(&[Role::Teacher])?;
    crate::api::cards::visible_card(&state.db, id, &user).await?;
    let (kind, service_id) = req.target.parse()?;
    if !(0..=100).contains(&req.score) {
        return Err(AppError::Validation("экспертный балл — от 0 до 100".into()));
    }
    let auto_score = attempt_score(&state.db, id, kind, service_id).await?;
    let previous = expert(&state.db, kind, id, service_id).await?;

    sqlx::query(
        "INSERT INTO expert_grades (kind, card_id, service_id, score, comment, teacher_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (kind, card_id, service_id) DO UPDATE
            SET score = EXCLUDED.score, comment = EXCLUDED.comment,
                teacher_id = EXCLUDED.teacher_id, updated_at = now()",
    )
    .bind(kind.as_str())
    .bind(id)
    .bind(service_id)
    .bind(req.score)
    .bind(req.comment.trim())
    .bind(user.id)
    .execute(&state.db)
    .await?;

    // Изменение оценки — первое, что поднимают при споре о результате.
    // Автоматический балл пишем рядом: видно, насколько эксперт
    // разошёлся с правилами
    audit::log(
        &state.db,
        audit::Event::new("grade.override")
            .from_user(&user)
            .about("card", id)
            .with(json!({
                "kind": kind.as_str(),
                "serviceId": service_id,
                "autoScore": auto_score,
                "score": req.score,
                "comment": req.comment.trim(),
                "previousScore": previous.as_ref().map(|p| p.score),
            })),
    )
    .await;

    expert(&state.db, kind, id, service_id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerdictInput {
    pub analysis_id: Uuid,
    /// Номер замечания в списке анализа
    pub index: usize,
    /// `confirmed`, `rejected`; пусто — снять вердикт
    pub verdict: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/review/verdict",
    tag = "Разбор",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = VerdictInput,
    responses(
        (status = 204, description = "Вердикт записан"),
        (status = 400, description = "Нет такого замечания или вердикта"),
        (status = 403, description = "Только преподаватель"),
    ),
)]
pub async fn set_verdict(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<VerdictInput>,
) -> AppResult<StatusCode> {
    user.require(&[Role::Teacher])?;
    if let Some(v) = req.verdict.as_deref() {
        if v != "confirmed" && v != "rejected" {
            return Err(AppError::Validation("вердикт — «confirmed» или «rejected»".into()));
        }
    }

    let count: Option<i32> = sqlx::query_scalar(
        "SELECT jsonb_array_length(findings) FROM text_analyses
          WHERE id = $1 AND card_id = $2 AND status = 'ready'",
    )
    .bind(req.analysis_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let Some(count) = count else { return Err(AppError::NotFound) };
    if req.index >= count as usize {
        return Err(AppError::Validation("нет замечания с таким номером".into()));
    }

    let key = req.index.to_string();
    match req.verdict.as_deref() {
        Some(v) => {
            sqlx::query(
                "UPDATE text_analyses
                    SET verdicts = verdicts || jsonb_build_object($2::text,
                          jsonb_build_object('verdict', $3::text, 'by', $4::uuid, 'at', now()))
                  WHERE id = $1",
            )
            .bind(req.analysis_id)
            .bind(&key)
            .bind(v)
            .bind(user.id)
            .execute(&state.db)
            .await?;
        }
        None => {
            sqlx::query("UPDATE text_analyses SET verdicts = verdicts - $2::text WHERE id = $1")
                .bind(req.analysis_id)
                .bind(&key)
                .execute(&state.db)
                .await?;
        }
    }

    audit::log(
        &state.db,
        audit::Event::new("analysis.verdict")
            .from_user(&user)
            .about("card", id)
            .with(json!({
                "analysisId": req.analysis_id,
                "index": req.index,
                "verdict": req.verdict,
            })),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/cards/{id}/review/analyse",
    tag = "Разбор",
    params(("id" = String, Path, description = "Идентификатор карточки")),
    request_body = Target,
    responses(
        (status = 202, description = "Анализ поставлен в очередь"),
        (status = 403, description = "Только преподаватель"),
        (status = 409, description = "Попытка ещё не разобрана"),
    ),
)]
pub async fn analyse(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<Target>,
) -> AppResult<StatusCode> {
    // Повторный анализ нужен, когда модель была недоступна: подняли её —
    // и разбор можно получить, не заставляя курсанта проходить заново
    user.require(&[Role::Teacher])?;
    crate::api::cards::visible_card(&state.db, id, &user).await?;
    let (kind, service_id) = req.parse()?;
    attempt_score(&state.db, id, kind, service_id).await?;
    text_analysis::enqueue(&state.db, kind, id, service_id, Some(user.id))
        .await
        .map_err(AppError::Other)?;
    Ok(StatusCode::ACCEPTED)
}
