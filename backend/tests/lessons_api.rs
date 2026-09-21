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

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&config.database_url).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
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

async fn прибрать(pool: &PgPool, group_id: Uuid) {
    sqlx::query("DELETE FROM training_groups WHERE id = $1")
        .bind(group_id).execute(pool).await.ok();
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
    let студент = токен(&app, "student2").await;
    let курсант: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student2'")
        .fetch_one(&pool).await.unwrap();
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
}

/// Пустое занятие не запускается.
#[tokio::test]
async fn занятие_без_материала_не_стартует() {
    let Some((app, pool)) = app().await else { return };
    let teacher = токен(&app, "teacher1").await;
    let курсант: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student3'")
        .fetch_one(&pool).await.unwrap();
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
    let курсант: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'student5'")
        .fetch_one(&pool).await.unwrap();
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
