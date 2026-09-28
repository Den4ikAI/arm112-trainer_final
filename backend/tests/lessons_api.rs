//! Занятие, которым управляет преподаватель.
//!
//! ТЗ §10 описывает два пользовательских пути, и оба начинаются
//! с того, что преподаватель выбирает тему и запускает обучение.
//! Здесь проверяется, что этот слой действительно управляет
//! занятием, а не просто заводит запись в базе.
//!
//! Запуск: `cargo test --test lessons_api`
//!
//! Каждому тесту достаётся свой курсант: прогон идёт параллельно
//! по одной базе, и общий `student1` ловил чужое идущее занятие.

use arm112::{api, config::Config, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// Куда ходят проверки.
///
/// Отдельная база, если она задана: иначе прогон копит мусор в той же,
/// на которой стоит стенд, и это видно в администрировании и в
/// переписке сценариев.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    подмести(&pool).await;
    Some((router, pool))
}

async fn call(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Value)
{
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn токен(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().expect("вход не удался").to_string()
}

/// Группа с одним курсантом внутри.
async fn группа(pool: &PgPool, курсант: Uuid) -> Uuid {
    let teacher: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(pool).await.unwrap();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id")
        .bind(format!("занятие-{}", Uuid::new_v4().simple()))
        .bind(teacher)
        .fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(id).bind(курсант).execute(pool).await.unwrap();
    id
}

/// Свой оператор 112 на проверку. Общий student1 не годится: пока
/// идёт занятие, оно ограничивает ему список вызовов, а соседние
/// проверки принимают вызовы им же параллельно.
async fn оператор(pool: &PgPool) -> (Uuid, String) {
    let login = format!("оператор-{}", &Uuid::new_v4().simple().to_string()[..12]);
    // Без службы — значит оператор 112 (профиль выводится из службы)
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role)
         SELECT $1, password_hash, 'Проверочный О.О.', 'student'
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).fetch_one(pool).await.unwrap();
    (id, login)
}

async fn убрать_оператора(pool: &PgPool, id: Uuid) {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(pool).await.ok();
    sqlx::query("DELETE FROM cards WHERE created_by = $1").bind(id).execute(pool).await.ok();
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(id).execute(pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(pool).await.ok();
}

/// Убрать группу со всем, что на неё ссылается.
///
/// Порядок важен: у заданий внешний ключ на группу без каскада, и
/// раньше удаление группы молча падало — ошибку глотал `.ok()`, а в
/// тестовой базе копились десятки групп «занятие-…».
async fn прибрать(pool: &PgPool, group_id: Uuid) {
    for sql in [
        "UPDATE sessions SET assignment_id = NULL WHERE assignment_id IN (
             SELECT id FROM assignments WHERE group_id = $1
                OR lesson_id IN (SELECT id FROM lessons WHERE group_id = $1))",
        "DELETE FROM assignments WHERE group_id = $1
             OR lesson_id IN (SELECT id FROM lessons WHERE group_id = $1)",
        "DELETE FROM training_groups WHERE id = $1",
    ] {
        sqlx::query(sql).bind(group_id).execute(pool).await.ok();
    }
}

/// Подмести то, что оставили упавшие прогоны.
///
/// Упавший тест до уборки не доходит. Опаснее всего оставленное
/// идущим занятие: оно ограничивает ленту и список вызовов своей
/// группы, и следующие прогоны начинают падать по чужой причине.
/// Трогаем только то, что старше десяти минут, — свежее может
/// принадлежать проверке, которая идёт прямо сейчас.
async fn подмести(pool: &PgPool) {
    let старые: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM training_groups
          WHERE title LIKE 'занятие-%' AND created_at < now() - interval '10 minutes'",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for g in старые {
        прибрать(pool, g).await;
    }

    for sql in [
        "DELETE FROM cards WHERE id IN (
             SELECT cs.card_id FROM card_services cs
              JOIN services s ON s.id = cs.service_id
             WHERE s.id LIKE 'проверка-%' AND s.created_at < now() - interval '10 minutes')",
        "UPDATE audit_log SET user_id = NULL WHERE user_id IN (
             SELECT id FROM users WHERE login LIKE 'диспетчер-%'
                AND created_at < now() - interval '10 minutes')",
        "DELETE FROM response_entries WHERE user_id IN (
             SELECT id FROM users WHERE login LIKE 'диспетчер-%'
                AND created_at < now() - interval '10 minutes')",
        "DELETE FROM users WHERE login LIKE 'диспетчер-%'
             AND created_at < now() - interval '10 minutes'",
        "DELETE FROM services WHERE id LIKE 'проверка-%'
             AND created_at < now() - interval '10 minutes'",
    ] {
        sqlx::query(sql).execute(pool).await.ok();
    }
}

/// Полный путь из ТЗ: собрать, запустить, завершить.
#[tokio::test]
async fn преподаватель_ведёт_занятие_от_старта_до_завершения() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let курсант: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student1'")
        .fetch_one(&pool).await.unwrap();
    let group_id = группа(&pool, курсант).await;

    let (status, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Пожары в жилом секторе",
                     "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{занятие}");
    let id = занятие["id"].as_str().unwrap().to_string();
    assert!(занятие["startedAt"].is_null(), "новое занятие ещё не идёт");

    // До старта курсанту занятие не показывается
    let студент = токен(&app, "student1").await;
    let (_, идущие) = call(&app, Method::GET, "/api/lessons", Some(&студент), None).await;
    assert_eq!(идущие.as_array().unwrap().len(), 0);

    let (status, начато) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");
    assert!(начато["startedAt"].is_string());
    assert!(начато["items"].as_i64().unwrap() > 0, "материал должен быть роздан");
    assert_eq!(начато["progress"].as_array().unwrap().len(), 1, "один курсант в группе");

    // Теперь курсант его видит
    let (_, идущие) = call(&app, Method::GET, "/api/lessons", Some(&студент), None).await;
    assert_eq!(идущие.as_array().unwrap().len(), 1, "идущее занятие курсанту видно");

    let (status, _) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);

    // Завершённое из списка курсанта пропадает
    let (_, идущие) = call(&app, Method::GET, "/api/lessons", Some(&студент), None).await;
    assert_eq!(идущие.as_array().unwrap().len(), 0);

    прибрать(&pool, group_id).await;
}

/// Пока идёт занятие, курсант видит только его вызовы.
///
/// Тему выбирает преподаватель — иначе управление занятием
/// бессмысленно: курсант просто возьмёт что хочет из общего списка.
#[tokio::test]
async fn идущее_занятие_ограничивает_список_вызовов() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (курсант, логин) = оператор(&pool).await;
    let студент = токен(&app, &логин).await;
    let group_id = группа(&pool, курсант).await;

    let (_, до) = call(&app, Method::GET, "/api/scenarios", Some(&студент), None).await;
    let было = до.as_array().unwrap().len();
    assert!(было > 0, "без занятия список открыт");

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Тема", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;

    let (_, во_время) = call(&app, Method::GET, "/api/scenarios", Some(&студент), None).await;
    let роздано: Vec<String> = sqlx::query_scalar(
        "SELECT scenario_id::text FROM assignments WHERE lesson_id = $1::uuid AND user_id = $2")
        .bind(&id).bind(курсант).fetch_all(&pool).await.unwrap();
    let видно: Vec<String> = во_время.as_array().unwrap().iter()
        .map(|s| s["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(видно.len(), роздано.len(), "видно ровно то, что роздано");
    for s in &видно {
        assert!(роздано.contains(s), "вызов {s} не из этого занятия");
    }

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;

    // После завершения список снова открыт: тренажёр остаётся
    // пригодным для самостоятельной отработки
    let (_, после) = call(&app, Method::GET, "/api/scenarios", Some(&студент), None).await;
    assert_eq!(после.as_array().unwrap().len(), было);

    прибрать(&pool, group_id).await;
    убрать_оператора(&pool, курсант).await;
}

/// Пустое занятие не запускается.
#[tokio::test]
async fn занятие_без_материала_не_стартует() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (курсант, _) = оператор(&pool).await;
    let group_id = группа(&pool, курсант).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Нет такой темы",
                     "mode": "card112", "typeIds": ["такого-типа-нет"] }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();

    let (status, ответ) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");
    assert!(ответ["detail"].as_str().unwrap().contains("не нашлось"),
            "отказ должен объяснять причину: {ответ}");

    прибрать(&pool, group_id).await;
    убрать_оператора(&pool, курсант).await;
}

/// Занятие ведёт тот, кто его создал.
#[tokio::test]
async fn чужое_занятие_курсанту_не_подчиняется() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let студент = токен(&app, "student4").await;
    let курсант: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student4'")
        .fetch_one(&pool).await.unwrap();
    let group_id = группа(&pool, курсант).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Тема", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();

    for (метод, путь) in [
        (Method::POST, format!("/api/lessons/{id}/start")),
        (Method::POST, format!("/api/lessons/{id}/finish")),
        (Method::DELETE, format!("/api/lessons/{id}")),
        (Method::GET, format!("/api/lessons/{id}")),
    ] {
        let (status, _) = call(&app, метод.clone(), &путь, Some(&студент), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "курсанту доступен {метод} {путь}");
    }

    // И собрать занятие он тоже не может
    let (status, _) = call(&app, Method::POST, "/api/lessons", Some(&студент),
        Some(json!({ "groupId": group_id, "title": "Моё", "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    прибрать(&pool, group_id).await;
}

/// Начатое занятие — уже история, его не удаляют.
#[tokio::test]
async fn начатое_занятие_не_удаляется() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (курсант, _) = оператор(&pool).await;
    let group_id = группа(&pool, курсант).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Тема", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();

    // Несобранное удалить можно
    let (status, _) = call(&app, Method::DELETE, &format!("/api/lessons/{id}"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, второе) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Тема", "mode": "card112" }))).await;
    let id2 = второе["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/lessons/{id2}/start"), Some(&teacher), None).await;

    let (status, ответ) = call(&app, Method::DELETE, &format!("/api/lessons/{id2}"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(ответ["detail"].as_str().unwrap().contains("завершить"));

    call(&app, Method::POST, &format!("/api/lessons/{id2}/finish"), Some(&teacher), None).await;
    прибрать(&pool, group_id).await;
    убрать_оператора(&pool, курсант).await;
}

/// Дважды начать или завершить незапущенное нельзя.
#[tokio::test]
async fn переходы_занятия_защищены_от_повтора() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let курсант: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student5'")
        .fetch_one(&pool).await.unwrap();
    let group_id = группа(&pool, курсант).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Тема", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();

    // Завершить то, что не начиналось
    let (status, _) = call(&app, Method::POST, &format!("/api/lessons/{id}/finish"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;

    let (status, ответ) = call(&app, Method::POST, &format!("/api/lessons/{id}/start"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    let (status, _) = call(&app, Method::POST, &format!("/api/lessons/{id}/finish"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "дважды завершать нечего");

    прибрать(&pool, group_id).await;
}

/// Всё, что нужно для изолированной проверки режима ДДС: своя служба,
/// свой курсант и своя группа. Идущее занятие ограничивает ленту всей
/// группы, и на общих учётках оно задело бы соседние проверки,
/// которые гоняются параллельно.
struct Отдельно {
    service: String,
    user_id: Uuid,
    login: String,
    group_id: Uuid,
}

async fn отдельно(pool: &PgPool) -> Отдельно {
    let метка = Uuid::new_v4().simple().to_string();
    let service = format!("проверка-{}", &метка[..12]);
    let login = format!("диспетчер-{}", &метка[..12]);

    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&service).execute(pool).await.unwrap();

    // Пароль тот же, что у остальных учебных учёток
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Проверочный Д.Д.', 'student', $2
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).bind(&service).fetch_one(pool).await.unwrap();

    let group_id = группа(pool, user_id).await;
    Отдельно { service, user_id, login, group_id }
}

async fn карточка_для(pool: &PgPool, service: &str, закрыта: bool) -> Uuid {
    let номер = format!("П{}", &Uuid::new_v4().simple().to_string()[..10]);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description)
         VALUES ($1, now(), '{101}', 'проверочная карточка') RETURNING id",
    )
    .bind(&номер).fetch_one(pool).await.unwrap();

    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
        .bind(id).bind(service).execute(pool).await.unwrap();

    if закрыта {
        // Служба по этой карточке уже отработала: как материал
        // занятия она бесполезна
        for статус in ["Принята", "Работы завершены"] {
            sqlx::query(
                "INSERT INTO response_entries (card_id, service_id, status, operator)
                 VALUES ($1, $2, $3::response_status, 'оп. 0')",
            )
            .bind(id).bind(service).bind(статус).execute(pool).await.unwrap();
        }
    }
    id
}

async fn прибрать_отдельно(pool: &PgPool, о: &Отдельно, карточки: &[Uuid]) {
    for к in карточки {
        sqlx::query("DELETE FROM cards WHERE id = $1").bind(к).execute(pool).await.ok();
    }
    прибрать(pool, о.group_id).await;
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1")
        .bind(о.user_id).execute(pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(о.user_id).execute(pool).await.ok();
    sqlx::query("DELETE FROM services WHERE id = $1").bind(&о.service).execute(pool).await.ok();
}

fn номера(лента: &Value) -> Vec<String> {
    лента["items"].as_array().unwrap().iter()
        .map(|к| к["id"].as_str().unwrap().to_string())
        .collect()
}

/// Занятие ДДС ограничивает ленту своими карточками, а после
/// завершения лента снова открыта.
///
/// Раньше карточки занятия раскладывались, но лента их не учитывала:
/// занятие ДДС собиралось и стартовало, а на экран курсанта не влияло.
#[tokio::test]
async fn занятие_ддс_ограничивает_ленту_своими_карточками() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let о = отдельно(&pool).await;

    let открытая = карточка_для(&pool, &о.service, false).await;
    let закрытая = карточка_для(&pool, &о.service, true).await;

    let курсант = токен(&app, &о.login).await;
    let лента = "/api/cards?scope=service";

    // Без занятия видны обе: лента службы как есть
    let (_, до) = call(&app, Method::GET, лента, Some(&курсант), None).await;
    let до = номера(&до);
    assert!(до.contains(&открытая.to_string()) && до.contains(&закрытая.to_string()));

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Статусы реагирования",
                     "mode": "dds" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();

    let (status, начато) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");

    // Во время занятия — только его материал. Закрытая карточка
    // в материал не попала: работы по ней для службы уже нет
    let (_, во_время) = call(&app, Method::GET, лента, Some(&курсант), None).await;
    assert_eq!(номера(&во_время), vec![открытая.to_string()],
               "в ленте должны быть только карточки занятия");
    assert_eq!(во_время["total"], json!(1), "счётчик тоже считает по занятию");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;

    let (_, после) = call(&app, Method::GET, лента, Some(&курсант), None).await;
    assert_eq!(номера(&после).len(), до.len(), "после занятия лента снова открыта");

    прибрать_отдельно(&pool, &о, &[открытая, закрытая]).await;
}

/// Ход занятия ДДС считается по проставленным статусам.
///
/// Раньше он считался по заданиям, а у занятия ДДС заданий нет —
/// преподаватель видел нули при любой работе группы.
#[tokio::test]
async fn ход_занятия_ддс_виден_по_статусам() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let о = отдельно(&pool).await;
    let карточка = карточка_для(&pool, &о.service, false).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Статусы", "mode": "dds" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;

    let курсант = токен(&app, &о.login).await;

    let ход = |ответ: &Value| -> (i64, i64) {
        let строка = &ответ["progress"][0];
        (строка["started"].as_i64().unwrap(), строка["graded"].as_i64().unwrap())
    };

    let (_, пока_ничего) = call(&app, Method::GET, &format!("/api/lessons/{id}"),
        Some(&teacher), None).await;
    assert_eq!(ход(&пока_ничего), (0, 0));

    // Взялся — карточка в работе
    let (status, ответ) = call(&app, Method::POST,
        &format!("/api/cards/{карточка}/responses"), Some(&курсант),
        Some(json!({ "serviceId": о.service, "status": "Принята" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");

    let (_, взялся) = call(&app, Method::GET, &format!("/api/lessons/{id}"),
        Some(&teacher), None).await;
    assert_eq!(ход(&взялся), (1, 0), "карточка взята, но не закончена");

    // Довёл до конца
    call(&app, Method::POST, &format!("/api/cards/{карточка}/responses"), Some(&курсант),
        Some(json!({ "serviceId": о.service, "status": "Работы завершены" }))).await;

    let (_, закончил) = call(&app, Method::GET, &format!("/api/lessons/{id}"),
        Some(&teacher), None).await;
    assert_eq!(ход(&закончил), (1, 1));
    assert!(закончил["progress"][0]["avgScore"].is_null(),
            "в режиме ДДС разбора нет, и среднего балла тоже");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    прибрать_отдельно(&pool, &о, &[карточка]).await;
}

/// Занятие ДДС не стартует, если работы для служб группы нет.
#[tokio::test]
async fn занятие_ддс_без_работы_не_стартует() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let о = отдельно(&pool).await;
    // Единственная карточка службы уже закрыта
    let закрытая = карточка_для(&pool, &о.service, true).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Пусто", "mode": "dds" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();

    let (status, ответ) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");
    let причина = ответ["detail"].as_str().unwrap();
    assert!(причина.contains("не нашлось") && причина.contains("служб"),
            "отказ должен объяснять, чего не хватает: {причина}");

    прибрать_отдельно(&pool, &о, &[закрытая]).await;
}

/// Карточки занятия ДДС приходят потоком (ответ заказчика 23.09,
/// сообщение 709): первая — сразу, следующая — через интервал,
/// заданный преподавателем. До выпуска карточки в ленте нет, после —
/// есть, и о её появлении приходит событие.
#[tokio::test]
async fn карточки_занятия_ддс_приходят_потоком() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let о = отдельно(&pool).await;
    let первая = карточка_для(&pool, &о.service, false).await;
    let вторая = карточка_для(&pool, &о.service, false).await;
    let курсант = токен(&app, &о.login).await;
    let лента = "/api/cards?scope=service";

    // Интервал вне пределов не принимается
    let (status, _) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Поток", "mode": "dds",
                     "cardIntervalSec": 5 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Поток", "mode": "dds",
                     "cardIntervalSec": 600 }))).await;
    assert_eq!(занятие["cardIntervalSec"], 600);
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, начато) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");
    assert_eq!(начато["items"], 2, "в материал попали обе карточки");

    // Сразу видна одна: вторая придёт через десять минут
    let (_, сейчас) = call(&app, Method::GET, лента, Some(&курсант), None).await;
    assert_eq!(номера(&сейчас).len(), 1, "{сейчас}");
    assert_eq!(сейчас["total"], 1);
    let видна = номера(&сейчас)[0].clone();
    assert!(видна == первая.to_string() || видна == вторая.to_string());
    // Ждёт она с момента выпуска, а не с сохранения
    assert!(сейчас["items"][0]["waitingSec"].as_i64().unwrap() < 30);
    assert_eq!(сейчас["items"][0]["opened"], false);

    // И по прямой ссылке невыпущенную не открыть: иначе курсант
    // увидел бы, что его ждёт, раньше срока
    let другая = if видна == первая.to_string() { вторая } else { первая };
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{другая}"),
        Some(&курсант), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Время второй пришло: она в ленте, и о ней разослано событие
    let hub = arm112::ws::Hub::new();
    let mut слушатель = hub.subscribe();
    sqlx::query("UPDATE lesson_cards SET release_at = now() WHERE lesson_id = $1::uuid")
        .bind(&id).execute(&pool).await.unwrap();
    arm112::jobs::lesson_feed::announce(&pool, &hub).await.unwrap();

    let (_, потом) = call(&app, Method::GET, лента, Some(&курсант), None).await;
    let mut все = номера(&потом);
    все.sort();
    let mut ожидали = vec![первая.to_string(), вторая.to_string()];
    ожидали.sort();
    assert_eq!(все, ожидали);

    let mut объявлена = false;
    while let Ok(e) = слушатель.try_recv() {
        if e.kind == "card.dispatched" && e.payload["cardId"] == другая.to_string() {
            объявлена = true;
        }
    }
    assert!(объявлена, "о выпущенной карточке должно прийти событие");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    прибрать_отдельно(&pool, &о, &[первая, вторая]).await;
}

/// Попытка по вызову из занятия записывается в задание, хотя клиент
/// задание не присылает.
///
/// Раньше связь ждали от клиента, а он её не слал: ход занятия у
/// преподавателя показывал ноль начатых вызовов при любой работе.
#[tokio::test]
async fn попытка_по_вызову_занятия_попадает_в_задание() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    // Приём вызова — работа оператора 112
    let (user_id, login) = оператор(&pool).await;
    let group_id = группа(&pool, user_id).await;

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": group_id, "title": "Попытки", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;

    let (задание, сценарий): (Uuid, Uuid) = sqlx::query_as(
        "SELECT id, scenario_id FROM assignments WHERE lesson_id = $1::uuid AND user_id = $2 LIMIT 1")
        .bind(&id).bind(user_id).fetch_one(&pool).await.unwrap();

    let курсант = токен(&app, &login).await;
    let (status, начато) = call(&app, Method::POST, "/api/sessions", Some(&курсант),
        Some(json!({ "scenarioId": сценарий, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{начато}");

    let привязано: Option<Uuid> = sqlx::query_scalar(
        "SELECT assignment_id FROM sessions WHERE id = $1::uuid")
        .bind(начато["session"]["id"].as_str().unwrap())
        .fetch_one(&pool).await.unwrap();
    assert_eq!(привязано, Some(задание), "попытка не связана с заданием занятия");

    let (_, ход) = call(&app, Method::GET, &format!("/api/lessons/{id}"),
        Some(&teacher), None).await;
    assert_eq!(ход["progress"][0]["started"], 1, "преподаватель должен видеть начатый вызов: {ход}");

    // Чужое задание явно указать нельзя
    let (status, _) = call(&app, Method::POST, "/api/sessions", Some(&токен(&app, "student1").await),
        Some(json!({ "scenarioId": сценарий, "mode": "card112", "assignmentId": задание }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "попытка записалась в чужое задание");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    прибрать(&pool, group_id).await;
    убрать_оператора(&pool, user_id).await;
}

/// Занятие ведётся на одном рабочем месте: вызовы 112 раздаются только
/// операторам 112, диспетчер ДДС той же группы заданий не получает и
/// занятия по приёму вызовов у себя не видит.
#[tokio::test]
async fn вызовы_занятия_достаются_только_операторам_112() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let (оп, оп_логин) = оператор(&pool).await;
    let о = отдельно(&pool).await; // диспетчер ДДС своей службы
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(о.group_id).bind(оп).execute(&pool).await.unwrap();

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Приём", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, начато) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");

    let задания = |user: Uuid| {
        let pool = pool.clone();
        let id = id.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM assignments WHERE lesson_id = $1::uuid AND user_id = $2")
                .bind(&id).bind(user).fetch_one(&pool).await.unwrap()
        }
    };
    assert!(задания(оп).await > 0, "оператор 112 должен получить вызовы");
    assert_eq!(задания(о.user_id).await, 0, "диспетчеру ДДС вызовы 112 не положены");

    // Ход занятия — только по операторам
    let строки = начато["progress"].as_array().unwrap();
    assert!(строки.iter().all(|r| r["userId"] != о.user_id.to_string()), "{начато}");

    // Диспетчер занятия по приёму вызова у себя не видит, оператор — видит
    let (_, у_диспетчера) = call(&app, Method::GET, "/api/lessons",
        Some(&токен(&app, &о.login).await), None).await;
    assert!(у_диспетчера.as_array().unwrap().iter().all(|l| l["id"] != id.as_str()));
    let (_, у_оператора) = call(&app, Method::GET, "/api/lessons",
        Some(&токен(&app, &оп_логин).await), None).await;
    assert!(у_оператора.as_array().unwrap().iter().any(|l| l["id"] == id.as_str()));

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    прибрать_отдельно(&pool, &о, &[]).await;
    убрать_оператора(&pool, оп).await;
}

/// Нет в группе курсантов нужного профиля — занятие не запускается,
/// и отказ говорит, в чём дело, а не «не нашлось вызовов».
#[tokio::test]
async fn занятие_без_курсантов_нужного_профиля_не_стартует() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let о = отдельно(&pool).await; // в группе только диспетчер ДДС

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": о.group_id, "title": "Приём", "mode": "card112" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, ответ) = call(&app, Method::POST,
        &format!("/api/lessons/{id}/start"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");
    assert!(ответ["detail"].as_str().unwrap().contains("оператор 112"), "{ответ}");

    прибрать_отдельно(&pool, &о, &[]).await;
}
