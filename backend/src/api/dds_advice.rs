//! Рекомендации диспетчеру ДДС и следующее задание от ИИ под них.
//!
//! Рекомендации финалистам, п. 5 и 10: цепочка «ИИ формирует задание →
//! обучаемый ДДС выполняет → результат → рекомендации для дальнейшего
//! обучения» без преподавателя. Рекомендации считает
//! `reports::dds_advice`, задание собирает `api::dds_incidents::
//! practice_for` — то же, что по кнопке «Новое происшествие от ИИ»,
//! только с отметкой, какую рекомендацию оно отрабатывает.

use crate::{
    api::{
        auth::{AuthUser, Duty, Role},
        dds_incidents::{practice_for, Practice},
        forecast::GroupAdvice,
    },
    audit,
    error::{AppError, AppResult},
    reports::dds_advice,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/advice/dds/me", get(my_advice))
        .route("/api/advice/dds/group/{id}", get(group_advice))
        .route("/api/advice/dds/{id}/next", post(next_task))
}

#[utoipa::path(
    get,
    path = "/api/advice/dds/me",
    tag = "Прогноз",
    responses(
        (status = 200, description = "Свои рекомендации диспетчера: выданные и проверенные", body = Vec<dds_advice::DdsAdviceView>),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Только диспетчеру ДДС"),
    ),
)]
pub async fn my_advice(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<dds_advice::DdsAdviceView>>> {
    user.require_duty(Duty::Dds)?;
    // Разбор мог закрыться не статусом курсанта (сторож нормативов,
    // преподаватель): перед выдачей списка досчитываем. Повтор безвреден
    if user.role == Role::Student {
        dds_advice::after_close(&state.db, user.id).await.map_err(AppError::Other)?;
    }
    Ok(Json(dds_advice::mine(&state.db, user.id).await.map_err(AppError::Other)?))
}

#[utoipa::path(
    get,
    path = "/api/advice/dds/group/{id}",
    tag = "Прогноз",
    params(("id" = String, Path, description = "Идентификатор группы")),
    responses(
        (status = 200, description = "Доля сработавших рекомендаций ДДС и базовая линия", body = GroupAdvice),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn group_advice(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<GroupAdvice>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    let (efficacy, recent) = dds_advice::group_efficacy(&state.db, id)
        .await
        .map_err(AppError::Other)?;
    Ok(Json(GroupAdvice { efficacy, recent }))
}

#[utoipa::path(
    post,
    path = "/api/advice/dds/{id}/next",
    tag = "Прогноз",
    params(("id" = String, Path, description = "Идентификатор рекомендации")),
    responses(
        (status = 201, description = "Следующее задание от ИИ выдано курсанту под эту рекомендацию", body = Practice),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Только диспетчеру ДДС"),
        (status = 404, description = "Нет такой открытой рекомендации"),
        (status = 409, description = "Происшествие уже готовится или курсант не в группе"),
    ),
)]
pub async fn next_task(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<(StatusCode, Json<Practice>)> {
    user.require(&[Role::Student])?;
    user.require_duty(Duty::Dds)?;
    // Сначала проверяем рекомендацию, потом зовём модель: на процессоре
    // это до двух минут, и тратить их на чужой идентификатор незачем
    let own: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM dds_advice WHERE id = $1 AND user_id = $2 AND checked_at IS NULL)",
    )
    .bind(id)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if !own {
        return Err(AppError::NotFound);
    }
    let task = practice_for(&state, &user).await?;
    dds_advice::mark_next(&state.db, id, user.id, task.card_id)
        .await
        .map_err(AppError::Other)?;
    audit::log(
        &state.db,
        audit::Event::new("advice.next_task")
            .from_user(&user)
            .about("card", task.card_id)
            .with(json!({ "adviceId": id })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(task)))
}
