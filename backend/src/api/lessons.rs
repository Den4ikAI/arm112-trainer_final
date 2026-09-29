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
    /// Режим ДДС: через сколько секунд службе приходит следующая
    /// карточка. Не задан — 60 секунд, наше допущение
    #[serde(default)]
    pub card_interval_sec: Option<i32>,
    /// Вид занятия: `training` — обычное, `marathon` — экзамен на
    /// 5–10 вызовов подряд, `blitz` — серия заданий на один сегмент
    #[serde(default)]
    pub kind: Option<String>,
    /// Сегмент блица: `address`, `type`, `services` у приёма вызова,
    /// `statuses` у ДДС
    #[serde(default)]
    pub segment: Option<String>,
    /// Сколько вызовов раздать, если они не выбраны поимённо
    #[serde(default)]
    pub call_count: Option<i32>,
    /// Вызовы, выбранные преподавателем, в порядке прохождения
    #[serde(default)]
    pub scenario_ids: Vec<Uuid>,
    /// Режим ДДС: учебные происшествия, выбранные преподавателем поимённо,
    /// в порядке выдачи. Пусто — подбираются случайно из созданных
    /// преподавателем по категориям занятия
    #[serde(default)]
    pub card_ids: Vec<Uuid>,
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
    /// Режим ДДС: интервал поступления карточек службе, секунды
    pub card_interval_sec: i32,
    /// `training`, `marathon` или `blitz`
    pub kind: String,
    /// Сегмент блица
    pub segment: Option<String>,
    /// Сколько вызовов раздаётся, если не выбраны поимённо
    pub call_count: Option<i32>,
    /// Вызовы, выбранные преподавателем, по порядку
    pub scenario_ids: Vec<Uuid>,
    /// Учебные происшествия ДДС, выбранные преподавателем, по порядку
    pub card_ids: Vec<Uuid>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Сколько человек в группе
    pub members: i64,
    /// Сколько материала роздано: вызовов или карточек
    pub items: i64,
}

/// Ход занятия по каждому курсанту.
///
/// Смысл полей зависит от режима. В приёме вызова это вызовы и их
/// разбор. В режиме ДДС разбора нет — там карточки, за которые курсант
/// взялся, и те, где его служба довела работу до конца.
#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub user_id: Uuid,
    pub fio: String,
    /// Вызовов начато / карточек взято в работу
    pub started: i64,
    /// Вызовов доведено до разбора / карточек, где служба закончила
    pub graded: i64,
    /// Средний балл: в приёме вызова — по разобранным вызовам, в режиме
    /// ДДС — по разбору работы с бригадой (`domain::brigade`)
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

/// Интервал поступления карточек в занятии ДДС, секунды.
///
/// Поток карточек — ответ заказчика (чат задачи, 23.09.2026, сообщение
/// 709), а темп он не задал. 60 секунд — наше допущение: при нормативах
/// 30 секунд на открытие и 3 минуты на первую запись у диспетчера
/// одновременно копится очередь из нескольких карточек, как в ответе и
/// описано. Пределы — та же проверка в базе.
const ИНТЕРВАЛ_ПО_УМОЛЧАНИЮ: i32 = 60;
const ИНТЕРВАЛ_ПРЕДЕЛЫ: (i32, i32) = (10, 900);
const ИСТОЧНИКИ: [&str; 3] = ["generated", "by_students", "mixed"];
const ВИДЫ: [&str; 3] = ["training", "marathon", "blitz"];

/// Проверить вид занятия и вернуть сегмент и число вызовов.
///
/// Марафон — только приём вызова: у ДДС карточки и так идут потоком,
/// а «экзамен на 5–10 карточек» в пожелании команды — про карточку 112.
fn правила_вида(req: &NewLesson, kind: &str) -> AppResult<(Option<String>, Option<i32>)> {
    use crate::domain::drill::{BLITZ_CALLS, DEFAULT_CALLS, MARATHON_CALLS, SEGMENTS_112, SEGMENT_DDS};

    // Экзамен бывает у обоих рабочих мест: у оператора 112 — вызовы
    // подряд, у диспетчера ДДС — учебные происшествия подряд

    let сегмент = match kind {
        "blitz" => {
            let s = req.segment.clone().unwrap_or_default();
            let годен = if req.mode == "dds" {
                s == SEGMENT_DDS
            } else {
                SEGMENTS_112.contains(&s.as_str())
            };
            if !годен {
                return Err(AppError::Validation(if req.mode == "dds" {
                    "сегмент блица в режиме ДДС — «statuses» (статусы по докладам)".into()
                } else {
                    "сегмент блица — «address», «type» или «services»".into()
                }));
            }
            Some(s)
        }
        _ if req.segment.is_some() => {
            return Err(AppError::Validation("сегмент задаётся только блицу".into()));
        }
        _ => None,
    };

    if req.mode == "card112" && !req.card_ids.is_empty() {
        return Err(AppError::Validation(
            "учебные происшествия выдаются диспетчерам ДДС: режим «dds»".into(),
        ));
    }

    // У ДДС материал — учебные происшествия, вызовы не раздаются
    if req.mode == "dds" {
        if !req.scenario_ids.is_empty() || req.call_count.is_some() {
            return Err(AppError::Validation(
                "в режиме ДДС материал — карточки служб группы, вызовы не выбираются".into(),
            ));
        }
        return Ok((сегмент, None));
    }

    let пределы = match kind {
        "marathon" => MARATHON_CALLS,
        "blitz" => BLITZ_CALLS,
        // Обычное занятие — как раньше: до десяти вызовов
        _ => (1, 10),
    };
    let n = if !req.scenario_ids.is_empty() {
        req.scenario_ids.len()
    } else if let Some(n) = req.call_count {
        n.max(0) as usize
    } else if kind == "training" {
        return Ok((None, None));
    } else {
        DEFAULT_CALLS
    };
    if !(пределы.0..=пределы.1).contains(&n) {
        let что = match kind {
            "marathon" => "В марафоне",
            "blitz" => "В блице",
            _ => "В занятии",
        };
        return Err(AppError::Validation(format!(
            "{что} от {} до {} вызовов, а задано {n}",
            пределы.0, пределы.1
        )));
    }

    // Обычному занятию с выбранными вызовами число задаёт сам список:
    // записывать его в call_count нельзя — у поля нижняя граница 3,
    // и занятие на один-два вызова падало с 500
    if kind == "training" && !req.scenario_ids.is_empty() {
        return Ok((сегмент, None));
    }
    Ok((сегмент, Some(n as i32)))
}

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
    let source = req.source.clone().unwrap_or_else(|| "generated".into());
    // Карточки, заполненные курсантами 112, диспетчерам не выдаются
    // (Денис, 29.09): материал ДДС — только учебные происшествия,
    // созданные преподавателем
    if source == "by_students" {
        return Err(AppError::Validation(
            "карточки курсантов 112 диспетчерам ДДС не выдаются: материал — \
             учебные происшествия, созданные преподавателем".into(),
        ));
    }
    if !ИСТОЧНИКИ.contains(&source.as_str()) {
        return Err(AppError::Validation(
            "источник карточек — «generated», «by_students» или «mixed»".into(),
        ));
    }

    let интервал = req.card_interval_sec.unwrap_or(ИНТЕРВАЛ_ПО_УМОЛЧАНИЮ);
    if !(ИНТЕРВАЛ_ПРЕДЕЛЫ.0..=ИНТЕРВАЛ_ПРЕДЕЛЫ.1).contains(&интервал) {
        return Err(AppError::Validation(format!(
            "интервал поступления карточек — от {} до {} секунд",
            ИНТЕРВАЛ_ПРЕДЕЛЫ.0, ИНТЕРВАЛ_ПРЕДЕЛЫ.1
        )));
    }

    let kind = req.kind.clone().unwrap_or_else(|| "training".into());
    if !ВИДЫ.contains(&kind.as_str()) {
        return Err(AppError::Validation(
            "вид занятия — «training», «marathon» или «blitz»".into(),
        ));
    }
    let (segment, call_count) = правила_вида(&req, &kind)?;

    // Выбранные поимённо вызовы должны существовать и быть выверены:
    // черновик курсанту не выдаётся
    if !req.scenario_ids.is_empty() {
        let годных: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT id) FROM scenarios WHERE id = ANY($1) AND status = 'approved'",
        )
        .bind(&req.scenario_ids)
        .fetch_one(&state.db)
        .await?;
        if годных != req.scenario_ids.len() as i64 {
            return Err(AppError::Validation(
                "среди выбранных вызовов есть неутверждённые, несуществующие или повторы".into(),
            ));
        }
    }

    if !req.card_ids.is_empty() {
        let годных: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT id) FROM cards
              WHERE id = ANY($1) AND origin = 'teacher' AND saved_at IS NOT NULL
                AND approved_at IS NOT NULL AND source_card_id IS NULL",
        )
        .bind(&req.card_ids)
        .fetch_one(&state.db)
        .await?;
        if годных != req.card_ids.len() as i64 {
            return Err(AppError::Validation(
                "среди выбранных происшествий есть несуществующие, повторы или \
                 карточки курсантов — выдаются только утверждённые учебные происшествия \
                 преподавателя".into(),
            ));
        }
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
        "INSERT INTO lessons (group_id, teacher_id, title, mode, type_ids, source,
                              card_interval_sec, kind, segment, call_count, scenario_ids,
                              card_ids)
         VALUES ($1, $2, $3, $4::session_mode, $5, $6::lesson_source, $7,
                 $8::lesson_kind, $9, $10, $11, $12)
         RETURNING id",
    )
    .bind(req.group_id)
    .bind(user.id)
    .bind(req.title.trim())
    .bind(&req.mode)
    .bind(&req.type_ids)
    .bind(&source)
    .bind(интервал)
    .bind(&kind)
    .bind(&segment)
    .bind(call_count)
    .bind(&req.scenario_ids)
    .bind(&req.card_ids)
    .fetch_one(&state.db)
    .await?;

    audit::log(
        &state.db,
        audit::Event::new("lesson.create")
            .from_user(&user)
            .about("lesson", id)
            .with(json!({ "title": req.title, "mode": req.mode, "typeIds": req.type_ids,
                          "kind": kind, "segment": segment, "callCount": call_count })),
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

    type Занятие = (String, String, Vec<String>, Uuid, bool, i32, String, Option<i32>, Vec<Uuid>,
                    Vec<Uuid>);
    let (mode, _source, type_ids, group_id, начато, интервал, kind, call_count, выбраны,
         происшествия): Занятие =
        sqlx::query_as(
            "SELECT mode::text, source::text, type_ids, group_id, started_at IS NOT NULL,
                    card_interval_sec, kind::text, call_count, scenario_ids, card_ids
               FROM lessons WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await?;

    if начато {
        return Err(AppError::Validation("занятие уже начато".into()));
    }

    // Занятие ведётся на одном рабочем месте. Нет в группе курсантов
    // с нужным профилем — запускать некому, и об этом надо сказать прямо,
    // а не отвечать «не нашлось вызовов»
    let профиль = if mode == "card112" { "op112" } else { "dds" };
    let подходящих: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM group_members m
           JOIN users u ON u.id = m.user_id
          WHERE m.group_id = $1 AND u.duty::text = $2 AND u.is_active",
    )
    .bind(group_id)
    .bind(профиль)
    .fetch_one(&state.db)
    .await?;
    if подходящих == 0 {
        return Err(AppError::Validation(if mode == "card112" {
            "в группе нет курсантов с профилем «оператор 112»: назначьте его \
             в администрировании или выберите режим «работа ДДС»"
                .into()
        } else {
            "в группе нет курсантов с профилем «диспетчер ДДС»: назначьте его \
             в администрировании или выберите режим «приём вызова 112»"
                .into()
        }));
    }

    let роздано = if mode == "card112" {
        // Обычному занятию — до десяти вызовов, как и раньше; марафону
        // и блицу — сколько задал преподаватель
        let сколько = call_count.map(|n| n as i64).unwrap_or(10);
        let вызовы = подобрать_вызовы(&state.db, &type_ids, &выбраны, сколько).await?;

        // Марафон — экзамен на 5–10 вызовов. Подобралось меньше —
        // запускать нечестно: это уже другой экзамен
        let нужно = match kind.as_str() {
            "marathon" => Some(crate::domain::drill::MARATHON_CALLS.0),
            "blitz" => Some(crate::domain::drill::BLITZ_CALLS.0),
            _ => None,
        };
        if let Some(нужно) = нужно {
            if !вызовы.is_empty() && вызовы.len() < нужно {
                return Err(AppError::Validation(format!(
                    "утверждённых вызовов нашлось {}, а нужно не меньше {нужно}: \
                     подготовьте ещё или выберите другие категории",
                    вызовы.len()
                )));
            }
        }
        раздать_вызовы(&state.db, id, group_id, &вызовы, kind == "marathon").await?
    } else {
        раздать_карточки(&state.db, id, group_id, &type_ids, &происшествия, интервал).await?
    };

    // Пустое занятие запускать бессмысленно: курсанты увидят
    // пустой экран и решат, что сломалось
    if роздано == 0 {
        let причина = if mode == "card112" {
            "по выбранным категориям не нашлось ни одного утверждённого вызова: \
             подготовьте их или выберите другие категории"
        } else {
            "не нашлось учебных происшествий для служб этой группы: создайте \
             их в разделе вызовов или выберите другие категории"
        };
        return Err(AppError::Validation(причина.into()));
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
    вызовы: &[Uuid],
    по_порядку: bool,
) -> AppResult<i64> {
    if вызовы.is_empty() {
        return Ok(0);
    }

    // Вызовы 112 принимает оператор 112: диспетчеру ДДС в группе
    // заданий на приём вызова не достаётся
    let участники: Vec<Uuid> = sqlx::query_scalar(
        "SELECT m.user_id FROM group_members m
           JOIN users u ON u.id = m.user_id
          WHERE m.group_id = $1 AND u.duty = 'op112' AND u.is_active",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;

    let teacher: Uuid = sqlx::query_scalar("SELECT teacher_id FROM lessons WHERE id = $1")
        .bind(lesson_id)
        .fetch_one(pool)
        .await?;

    let mut роздано = 0i64;
    for участник in &участники {
        // Марафон у всех в одном порядке — как экзаменационный билет;
        // в остальных видах порядок у каждого свой
        let mut свои: Vec<Uuid> = вызовы.to_vec();
        if !по_порядку {
            перемешать(&mut свои);
        }
        for (n, вызов) in свои.iter().enumerate() {
            sqlx::query(
                "INSERT INTO assignments (lesson_id, group_id, user_id, scenario_id, created_by, seq)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(lesson_id)
            .bind(group_id)
            .bind(участник)
            .bind(вызов)
            .bind(teacher)
            .bind(n as i32 + 1)
            .execute(pool)
            .await?;
            роздано += 1;
        }
    }

    // Группа без людей — это не повод считать занятие пустым:
    // материал подобран, просто некому его отдать
    Ok(if участники.is_empty() { вызовы.len() as i64 } else { роздано })
}

/// Перемешать без отдельной зависимости: криптостойкость здесь не
/// нужна, нужен только разный порядок у соседей.
fn перемешать(v: &mut [Uuid]) {
    for i in (1..v.len()).rev() {
        let j = (Uuid::new_v4().as_u128() % (i as u128 + 1)) as usize;
        v.swap(i, j);
    }
}

/// Подобрать вызовы занятия.
///
/// Выбранные преподавателем — ровно они и в его порядке. Иначе —
/// случайные утверждённые по категориям занятия: больше десяти за
/// занятие не отработать, а раздача растёт как произведение на число
/// курсантов.
async fn подобрать_вызовы(
    pool: &sqlx::PgPool,
    type_ids: &[String],
    выбраны: &[Uuid],
    сколько: i64,
) -> AppResult<Vec<Uuid>> {
    if !выбраны.is_empty() {
        // Выверенность проверена при создании, но между созданием и
        // запуском вызов могли снять в архив — такой не раздаём
        let годные: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM scenarios WHERE id = ANY($1) AND status = 'approved'",
        )
        .bind(выбраны)
        .fetch_all(pool)
        .await?;
        return Ok(выбраны.iter().copied().filter(|v| годные.contains(v)).collect());
    }

    Ok(sqlx::query_scalar(
        "SELECT id FROM scenarios
          WHERE status = 'approved'
            AND ($1::text[] = '{}' OR expected->'typeIds' ?| $1)
          ORDER BY random() LIMIT $2",
    )
    .bind(type_ids)
    .bind(сколько.clamp(1, 10))
    .fetch_all(pool)
    .await?)
}

/// Статусы, которыми служба заканчивает работу по карточке.
/// Карточка, где служба уже отработала, как материал занятия бесполезна.
const ЗАКРЫВАЮЩИЕ: &str = "'Работы завершены', 'Отказ от выполнения работ', 'Не принята'";

/// Сколько карточек достаётся одной службе группы.
const НА_СЛУЖБУ: i64 = 5;

/// Набрать карточки для работы со статусами реагирования.
///
/// Берём только карточки, которые касаются служб самой группы и где
/// эта служба ещё не отработала. Раньше брались двадцать последних
/// карточек вообще: диспетчер Мослифта получал пожары, которых не
/// увидит в своей ленте, а закрытые карточки — работу, которой нет.
///
/// Каждой службе — до пяти карточек, в случайном порядке: ТЗ (§10)
/// говорит о «формировании системой случайной карточки события».
///
/// Карточки приходят потоком: первая — сразу, следующие — через
/// `интервал` секунд каждая (ответ заказчика 23.09, сообщение 709).
/// Очередь своя у каждой службы: n-я карточка службы приходит через
/// (n−1)·интервал. Карточка нескольких служб приходит по самой ранней
/// из своих очередей — строка в `lesson_cards` у неё одна.
async fn раздать_карточки(
    pool: &sqlx::PgPool,
    lesson_id: Uuid,
    group_id: Uuid,
    type_ids: &[String],
    выбраны: &[Uuid],
    интервал: i32,
) -> AppResult<i64> {
    // Выбранные преподавателем — ровно они, в его порядке: первое сразу,
    // следующие через интервал. Только созданные преподавателем
    if !выбраны.is_empty() {
        let выдача: Vec<(Uuid, f64)> = sqlx::query_as(
            "SELECT t.id, ((t.ord - 1) * $2::float8)::float8
               FROM unnest($1::uuid[]) WITH ORDINALITY AS t(id, ord)
               JOIN cards c ON c.id = t.id AND c.origin = 'teacher'
                                 AND c.approved_at IS NOT NULL
                                 AND c.source_card_id IS NULL
              ORDER BY t.ord",
        )
        .bind(выбраны)
        .bind(интервал as f64)
        .fetch_all(pool)
        .await?;
        return выдать_копии(pool, lesson_id, &выдача).await;
    }

    // Иначе — случайные учебные происшествия служб группы. Карточки,
    // заполненные курсантами 112, диспетчерам не выдаются никогда, а
    // черновики преподавателя — пока он их не утвердил
    let условие = "c.origin = 'teacher' AND c.approved_at IS NOT NULL";

    let запрос = format!(
        "SELECT card_id, ((min(n) - 1) * $4::float8)::float8
           FROM (
             SELECT cs.card_id,
                    row_number() OVER (PARTITION BY cs.service_id ORDER BY random()) AS n
               FROM card_services cs
               JOIN cards c ON c.id = cs.card_id
              WHERE c.saved_at IS NOT NULL
                AND {условие}
                AND c.source_card_id IS NULL
                AND ($1::text[] = '{{}}' OR c.type_ids && $1)
                AND cs.service_id IN (
                      SELECT u.service_id FROM users u
                        JOIN group_members m ON m.user_id = u.id
                       WHERE m.group_id = $2 AND u.duty = 'dds' AND u.is_active)
                AND NOT EXISTS (
                      SELECT 1 FROM response_entries r
                       WHERE r.card_id = c.id AND r.service_id = cs.service_id
                         AND r.status::text IN ({ЗАКРЫВАЮЩИЕ}))
         ) t
         WHERE n <= $3
         GROUP BY card_id
         ORDER BY 2"
    );

    let выдача: Vec<(Uuid, f64)> = sqlx::query_as(&запрос)
        .bind(type_ids)
        .bind(group_id)
        .bind(НА_СЛУЖБУ)
        .bind(интервал as f64)
        .fetch_all(pool)
        .await?;

    выдать_копии(pool, lesson_id, &выдача).await
}

/// Выдать занятию копии учебных происшествий: у каждой выдачи своя
/// карточка со своими статусами, бригадой и разбором. Иначе одно
/// происшествие в двух занятиях делило бы всю работу по нему
/// (UX-проход 2, проблема A). `выдача` — исходник и через сколько
/// секунд от начала занятия он приходит.
async fn выдать_копии(pool: &sqlx::PgPool, lesson_id: Uuid, выдача: &[(Uuid, f64)]) -> AppResult<i64> {
    let mut tx = pool.begin().await?;
    // Два запуска разом (двойной щелчок) не должны выдать копии дважды:
    // раньше повтор гасил ON CONFLICT по (занятие, карточка), у копий
    // карточки каждый раз новые
    sqlx::query("SELECT 1 FROM lessons WHERE id = $1 FOR UPDATE")
        .bind(lesson_id)
        .execute(&mut *tx)
        .await?;
    let уже: i64 = sqlx::query_scalar("SELECT count(*) FROM lesson_cards WHERE lesson_id = $1")
        .bind(lesson_id)
        .fetch_one(&mut *tx)
        .await?;
    if уже > 0 {
        return Ok(уже);
    }
    for (исходник, через) in выдача {
        let копия: Uuid = sqlx::query_scalar(
            "INSERT INTO cards (created_by, workstation, phone_aon, phone_provided, phone_on_site,
                                applicant_name, applicant_status, address, description, victims,
                                no_contact, call_dropped, type_ids, answers, outcome_type,
                                saved_at, origin, title, approved_at, approved_by, source_card_id)
             SELECT created_by, workstation, phone_aon, phone_provided, phone_on_site,
                    applicant_name, applicant_status, address, description, victims,
                    no_contact, call_dropped, type_ids, answers, outcome_type,
                    saved_at, origin, title, approved_at, approved_by, id
               FROM cards WHERE id = $1
             RETURNING id",
        )
        .bind(исходник)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO card_services (card_id, service_id, source, added_at)
             SELECT $2, service_id, source, added_at FROM card_services WHERE card_id = $1",
        )
        .bind(исходник)
        .bind(копия)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO lesson_cards (lesson_id, card_id, release_at)
             VALUES ($1, $2, now() + make_interval(secs => $3::float8))",
        )
        .bind(lesson_id)
        .bind(копия)
        .bind(через)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(выдача.len() as i64)
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
        // …и только своего рабочего места: занятие по приёму вызовов
        // диспетчеру ДДС не касается, и наоборот
        let режим = match user.duty {
            Some(crate::api::auth::Duty::Dds) => "dds",
            _ => "card112",
        };
        sqlx::query_as(&format!(
            "{ОСНОВА}
              WHERE l.started_at IS NOT NULL AND l.finished_at IS NULL
                AND l.group_id IN (SELECT group_id FROM group_members WHERE user_id = $1)
                AND l.mode::text = $2
              ORDER BY l.started_at DESC"
        ))
        .bind(user.id)
        .bind(режим)
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
           l.card_interval_sec, l.kind::text AS kind, l.segment, l.call_count,
           l.scenario_ids, l.card_ids, l.started_at, l.finished_at,
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

    if lesson.mode == "dds" {
        return Ok(LessonFull { progress: ход_ддс(pool, id).await?, lesson });
    }

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
            AND u.duty = 'op112'
          GROUP BY u.id, u.fio
          ORDER BY u.fio",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    Ok(LessonFull { lesson, progress })
}

/// Ход занятия ДДС: статусы, которые курсант проставил по карточкам
/// занятия, пока оно шло.
///
/// Раньше ход считался по заданиям и в этом режиме был пуст всегда:
/// у занятия ДДС заданий нет, материал — общий набор карточек.
///
/// Средний балл — по разбору работы с бригадой: статусы по докладам,
/// а не наугад. До появления докладов разбора ДДС не было вовсе, и
/// балл здесь всегда был пуст.
async fn ход_ддс(pool: &sqlx::PgPool, id: Uuid) -> AppResult<Vec<Progress>> {
    let запрос = format!(
        "SELECT u.id AS user_id, u.fio,
                count(DISTINCT r.card_id) AS started,
                count(DISTINCT r.card_id)
                    FILTER (WHERE r.status::text IN ({ЗАКРЫВАЮЩИЕ})) AS graded,
                NULL::float8 AS avg_score
           FROM group_members m
           JOIN users u ON u.id = m.user_id
           JOIN lessons l ON l.id = $1
           LEFT JOIN response_entries r
                  ON r.user_id = u.id
                 AND r.card_id IN (SELECT card_id FROM lesson_cards WHERE lesson_id = $1)
                 AND r.at >= l.started_at
                 AND r.at <= COALESCE(l.finished_at, now())
          WHERE m.group_id = l.group_id
            AND u.duty = 'dds'
          GROUP BY u.id, u.fio
          ORDER BY u.fio"
    );
    let mut ход: Vec<Progress> = sqlx::query_as(&запрос).bind(id).fetch_all(pool).await?;

    for строка in &mut ход {
        // Бригады, которые курсант назначил по карточкам занятия, пока оно шло
        let назначено: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT a.card_id, a.service_id
               FROM brigade_assignments a
               JOIN lessons l ON l.id = $1
              WHERE a.user_id = $2
                AND a.card_id IN (SELECT card_id FROM lesson_cards WHERE lesson_id = $1)
                AND a.assigned_at >= l.started_at
                AND a.assigned_at <= COALESCE(l.finished_at, now())",
        )
        .bind(id)
        .bind(строка.user_id)
        .fetch_all(pool)
        .await?;

        let mut баллы = Vec::new();
        for (card_id, service_id) in назначено {
            if let Some(балл) = crate::api::brigade::score_for(pool, card_id, &service_id).await? {
                баллы.push(балл as f64);
            }
        }
        if !баллы.is_empty() {
            строка.avg_score = Some(баллы.iter().sum::<f64>() / баллы.len() as f64);
        }
    }

    Ok(ход)
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
