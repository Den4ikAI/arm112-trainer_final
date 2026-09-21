//! Занятие, которым управляет преподаватель.
//!
//! Порядок взят из ТЗ, §10, оба сценария «Проведение практического
//! занятия»: преподаватель выбирает категории происшествий под тему,
//! запускает обучение, следит за ходом и завершает в любой момент.
//!
//! Осторожно с терминами. В ТЗ «занятие» — это то, что ведёт
//! преподаватель. В нашем коде `sessions` — один вызов, отработанный
//! курсантом. Поэтому здесь `lessons` — занятие, `sessions` — вызов.

use crate::{
    api::auth::{AuthUser, Role},
    audit,
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
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/lessons", get(list).post(create))
        .route("/api/lessons/{id}", get(one).delete(remove))
        .route("/api/lessons/{id}/start", post(start))
        .route("/api/lessons/{id}/finish", post(finish))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewLesson {
    pub group_id: Uuid,
    pub title: String,
    /// «card112» — приём вызова, «dds» — работа со статусами реагирования
    pub mode: String,
    /// Категории происшествий под тему занятия. Пустой список — любые.
    #[serde(default)]
    pub type_ids: Vec<String>,
    /// Откуда брать карточки в режиме ДДС: `generated`, `by_students`, `mixed`
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LessonBrief {
    pub id: Uuid,
    pub title: String,
    pub group_id: Uuid,
    pub group_title: String,
    pub mode: String,
    pub source: String,
    pub type_ids: Vec<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Сколько человек в группе
    pub members: i64,
    /// Сколько материала роздано: вызовов или карточек
    pub items: i64,
}

/// Ход занятия по каждому курсанту.
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub user_id: Uuid,
    pub fio: String,
    /// Сколько вызовов начато
    pub started: i64,
    /// Сколько доведено до разбора
    pub graded: i64,
    /// Средний балл по завершённым, если они есть
    pub avg_score: Option<f64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LessonFull {
    #[serde(flatten)]
    pub lesson: LessonBrief,
    pub progress: Vec<Progress>,
}

const РЕЖИМЫ: [&str; 2] = ["card112", "dds"];
const ИСТОЧНИКИ: [&str; 3] = ["generated", "by_students", "mixed"];

/// Занятие ведёт тот, кто его создал. Администратор видит любое:
/// ему положены системные журналы, но не чужая учебная работа.
async fn своё(pool: &sqlx::PgPool, id: Uuid, user: &AuthUser) -> AppResult<()> {
    let teacher: Option<Uuid> = sqlx::query_scalar("SELECT teacher_id FROM lessons WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    match teacher {
        None => Err(AppError::NotFound),
        Some(t) if t == user.id || user.role == Role::Admin => Ok(()),
        Some(_) => Err(AppError::Forbidden),
    }
}

#[utoipa::path(
    post,
    path = "/api/lessons",
    tag = "Занятия",
    request_body = NewLesson,
    responses(
        (status = 201, description = "Занятие собрано", body = LessonBrief),
        (status = 400, description = "Негодные параметры"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<NewLesson>,
) -> AppResult<(StatusCode, Json<LessonBrief>)> {
    user.require(&[Role::Teacher, Role::Admin])?;

    if req.title.trim().is_empty() {
        return Err(AppError::Validation("у занятия должно быть название".into()));
    }
    if !РЕЖИМЫ.contains(&req.mode.as_str()) {
        return Err(AppError::Validation(
            "режим занятия — «card112» или «dds»".into(),
        ));
    }
    let source = req.source.clone().unwrap_or_else(|| "mixed".into());
    if !ИСТОЧНИКИ.contains(&source.as_str()) {
        return Err(AppError::Validation(
            "источник карточек — «generated», «by_students» или «mixed»".into(),
        ));
    }

    let есть_группа: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM training_groups WHERE id = $1)")
            .bind(req.group_id)
            .fetch_one(&state.db)
            .await?;
    if !есть_группа {
        return Err(AppError::Validation("такой учебной группы нет".into()));
    }

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO lessons (group_id, teacher_id, title, mode, type_ids, source)
         VALUES ($1, $2, $3, $4::session_mode, $5, $6::lesson_source)
         RETURNING id",
    )
    .bind(req.group_id)
    .bind(user.id)
    .bind(req.title.trim())
    .bind(&req.mode)
    .bind(&req.type_ids)
    .bind(&source)
    .fetch_one(&state.db)
    .await?;

    audit::log(
        &state.db,
        audit::Event::new("lesson.create")
            .from_user(&user)
            .about("lesson", id)
            .with(json!({ "title": req.title, "mode": req.mode, "typeIds": req.type_ids })),
    )
    .await;

    Ok((StatusCode::CREATED, Json(карточка(&state.db, id).await?)))
}

#[utoipa::path(
    post,
    path = "/api/lessons/{id}/start",
    tag = "Занятия",
    params(("id" = String, Path, description = "Идентификатор занятия")),
    responses(
        (status = 200, description = "Занятие начато, материал роздан", body = LessonFull),
        (status = 400, description = "Уже начато или материала не нашлось"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<LessonFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    своё(&state.db, id, &user).await?;

    let (mode, source, type_ids, group_id, начато): (String, String, Vec<String>, Uuid, bool) =
        sqlx::query_as(
            "SELECT mode::text, source::text, type_ids, group_id, started_at IS NOT NULL
               FROM lessons WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    if начато {
        return Err(AppError::Validation("занятие уже начато".into()));
    }

    let роздано = if mode == "card112" {
        раздать_вызовы(&state.db, id, group_id, &type_ids).await?
    } else {
        раздать_карточки(&state.db, id, &type_ids, &source).await?
    };

    // Пустое занятие запускать бессмысленно: курсанты увидят
    // пустой экран и решат, что сломалось
    if роздано == 0 {
        return Err(AppError::Validation(
            "по выбранным категориям не нашлось ни одного утверждённого вызова: \
             подготовьте их или выберите другие категории"
                .into(),
        ));
    }

    sqlx::query("UPDATE lessons SET started_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    audit::log(
        &state.db,
        audit::Event::new("lesson.start")
            .from_user(&user)
            .about("lesson", id)
            .with(json!({ "роздано": роздано })),
    )
    .await;

    // Курсанты узнают о старте, не перезагружая страницу
    state.ws.publish(
        crate::ws::Event::new("lesson.started", json!({ "lessonId": id }))
            .to(format!("group:{group_id}")),
    );

    Ok(Json(полностью(&state.db, id).await?))
}

/// Раздать группе вызовы для приёма звонка.
///
/// Курсанту достаётся весь набор занятия, а не один вызов: ТЗ
/// (§10) требует выдавать следующую карточку, пока занятие не
/// завершено. Порядок случайный — у соседей он будет свой.
async fn раздать_вызовы(
    pool: &sqlx::PgPool,
    lesson_id: Uuid,
    group_id: Uuid,
    type_ids: &[String],
) -> AppResult<i64> {
    // Больше десяти вызовов за занятие не отработать, а раздача
    // растёт как произведение на число курсантов
    let вызовы: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM scenarios
          WHERE status = 'approved'
            AND ($1::text[] = '{}' OR expected->'typeIds' ?| $1)
          ORDER BY random() LIMIT 10",
    )
    .bind(type_ids)
    .fetch_all(pool)
    .await?;

    if вызовы.is_empty() {
        return Ok(0);
    }

    let участники: Vec<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(pool)
            .await?;

    let teacher: Uuid = sqlx::query_scalar("SELECT teacher_id FROM lessons WHERE id = $1")
        .bind(lesson_id)
        .fetch_one(pool)
        .await?;

    let mut роздано = 0i64;
    for участник in &участники {
        for вызов in &вызовы {
            sqlx::query(
                "INSERT INTO assignments (lesson_id, group_id, user_id, scenario_id, created_by)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(lesson_id)
            .bind(group_id)
            .bind(участник)
            .bind(вызов)
            .bind(teacher)
            .execute(pool)
            .await?;
            роздано += 1;
        }
    }

    // Группа без людей — это не повод считать занятие пустым:
    // материал подобран, просто некому его отдать
    Ok(if участники.is_empty() { вызовы.len() as i64 } else { роздано })
}

/// Набрать карточки для работы со статусами реагирования.
async fn раздать_карточки(
    pool: &sqlx::PgPool,
    lesson_id: Uuid,
    type_ids: &[String],
    source: &str,
) -> AppResult<i64> {
    // «Сформированные обучающимися» — те, что заполнены в занятиях
    // по карточке 112; «созданные системой» — посеянные без занятия
    let условие = match source {
        "by_students" => "c.session_id IS NOT NULL",
        "generated" => "c.session_id IS NULL",
        _ => "TRUE",
    };

    let запрос = format!(
        "INSERT INTO lesson_cards (lesson_id, card_id)
         SELECT $1, c.id FROM cards c
          WHERE c.saved_at IS NOT NULL
            AND {условие}
            AND ($2::text[] = '{{}}' OR c.type_ids && $2)
          ORDER BY c.created_at DESC LIMIT 20
         ON CONFLICT DO NOTHING"
    );

    let роздано = sqlx::query(&запрос)
        .bind(lesson_id)
        .bind(type_ids)
        .execute(pool)
        .await?
        .rows_affected();

    Ok(роздано as i64)
}

#[utoipa::path(
    post,
    path = "/api/lessons/{id}/finish",
    tag = "Занятия",
    params(("id" = String, Path, description = "Идентификатор занятия")),
    responses(
        (status = 200, description = "Занятие завершено", body = LessonFull),
        (status = 400, description = "Занятие не начиналось или уже завершено"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn finish(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<LessonFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    своё(&state.db, id, &user).await?;

    // Завершаем условно, по прежнему состоянию: два преподавателя
    // могли нажать одновременно, и второй не должен переписать время
    let закрыто = sqlx::query(
        "UPDATE lessons SET finished_at = now()
          WHERE id = $1 AND started_at IS NOT NULL AND finished_at IS NULL",
    )
    .bind(id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if закрыто == 0 {
        return Err(AppError::Validation(
            "завершить можно только идущее занятие".into(),
        ));
    }

    // Недоработанные вызовы закрываем: иначе сторож нормативов будет
    // до утра переводить их в «Не завершено», а курсанта уже нет
    let брошено = sqlx::query(
        "UPDATE sessions SET finished_at = now()
          WHERE finished_at IS NULL
            AND assignment_id IN (SELECT id FROM assignments WHERE lesson_id = $1)",
    )
    .bind(id)
    .execute(&state.db)
    .await?
    .rows_affected();

    let group_id: Uuid = sqlx::query_scalar("SELECT group_id FROM lessons WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    audit::log(
        &state.db,
        audit::Event::new("lesson.finish")
            .from_user(&user)
            .about("lesson", id)
            .with(json!({ "брошенных_вызовов": брошено })),
    )
    .await;

    state.ws.publish(
        crate::ws::Event::new("lesson.finished", json!({ "lessonId": id }))
            .to(format!("group:{group_id}")),
    );

    Ok(Json(полностью(&state.db, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/lessons",
    tag = "Занятия",
    responses(
        (status = 200, description = "Занятия: преподавателю свои, курсанту идущие", body = Vec<LessonBrief>),
        (status = 401, description = "Требуется вход"),
    ),
)]
pub async fn list(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<LessonBrief>>> {
    // Курсанту показываем только идущие занятия его групп: прошлые
    // ему ничего не дают, а будущие ещё не его дело
    let строки: Vec<LessonBrief> = if user.role == Role::Student {
        sqlx::query_as(&format!(
            "{ОСНОВА}
              WHERE l.started_at IS NOT NULL AND l.finished_at IS NULL
                AND l.group_id IN (SELECT group_id FROM group_members WHERE user_id = $1)
              ORDER BY l.started_at DESC"
        ))
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(&format!(
            "{ОСНОВА}
              WHERE l.teacher_id = $1 OR $2
              ORDER BY l.created_at DESC LIMIT 50"
        ))
        .bind(user.id)
        .bind(user.role == Role::Admin)
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(строки))
}

/// Общая часть выборки занятия: счётчики считаются подзапросами,
/// чтобы не плодить соединений с повторами строк.
const ОСНОВА: &str = "
    SELECT l.id, l.title, l.group_id, g.title AS group_title,
           l.mode::text AS mode, l.source::text AS source, l.type_ids,
           l.started_at, l.finished_at,
           (SELECT count(*) FROM group_members m WHERE m.group_id = l.group_id) AS members,
           (SELECT count(*) FROM assignments a WHERE a.lesson_id = l.id)
         + (SELECT count(*) FROM lesson_cards c WHERE c.lesson_id = l.id) AS items
      FROM lessons l
      JOIN training_groups g ON g.id = l.group_id";

async fn карточка(pool: &sqlx::PgPool, id: Uuid) -> AppResult<LessonBrief> {
    sqlx::query_as(&format!("{ОСНОВА} WHERE l.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

async fn полностью(pool: &sqlx::PgPool, id: Uuid) -> AppResult<LessonFull> {
    let lesson = карточка(pool, id).await?;

    // Ход занятия: кто сколько начал и довёл до разбора. Считаем по
    // заданиям занятия, а не по всем вызовам курсанта
    let progress: Vec<Progress> = sqlx::query_as(
        "SELECT u.id AS user_id, u.fio,
                count(s.id) AS started,
                count(gr.id) AS graded,
                avg(gr.score)::float8 AS avg_score
           FROM group_members m
           JOIN users u ON u.id = m.user_id
           LEFT JOIN assignments a ON a.lesson_id = $1 AND a.user_id = u.id
           LEFT JOIN sessions s ON s.assignment_id = a.id
           LEFT JOIN grades gr ON gr.session_id = s.id
          WHERE m.group_id = (SELECT group_id FROM lessons WHERE id = $1)
          GROUP BY u.id, u.fio
          ORDER BY u.fio",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    Ok(LessonFull { lesson, progress })
}

#[utoipa::path(
    get,
    path = "/api/lessons/{id}",
    tag = "Занятия",
    params(("id" = String, Path, description = "Идентификатор занятия")),
    responses(
        (status = 200, description = "Занятие и ход по каждому курсанту", body = LessonFull),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
        (status = 404, description = "Занятие не найдено"),
    ),
)]
pub async fn one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<LessonFull>> {
    user.require(&[Role::Teacher, Role::Admin])?;
    своё(&state.db, id, &user).await?;
    Ok(Json(полностью(&state.db, id).await?))
}

#[utoipa::path(
    delete,
    path = "/api/lessons/{id}",
    tag = "Занятия",
    params(("id" = String, Path, description = "Идентификатор занятия")),
    responses(
        (status = 204, description = "Занятие удалено"),
        (status = 400, description = "Занятие уже шло"),
        (status = 401, description = "Требуется вход"),
        (status = 403, description = "Недостаточно прав"),
    ),
)]
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    user.require(&[Role::Teacher, Role::Admin])?;
    своё(&state.db, id, &user).await?;

    // Начатое занятие не удаляем: на его задания ссылаются вызовы
    // курсантов, а это уже история обучения
    let начато: bool =
        sqlx::query_scalar("SELECT started_at IS NOT NULL FROM lessons WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    if начато {
        return Err(AppError::Validation(
            "занятие уже шло: его можно завершить, но не удалить".into(),
        ));
    }

    sqlx::query("DELETE FROM lessons WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    audit::log(
        &state.db,
        audit::Event::new("lesson.delete").from_user(&user).about("lesson", id),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}
