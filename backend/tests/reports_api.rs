//! Отчёты по занятию и по группе.
//!
//! ТЗ перечисляет, что должно быть в отчёте преподавателя: действия
//! обучающегося, замечания, время заполнения, отличие от норматива и
//! результат проверки грамматики. Тесты проверяют наличие всех пяти.
//!
//! Запуск: `cargo test --test reports_api`

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

/// Возвращает и тело, и сырые байты: CSV JSON-ом не разобрать.
async fn raw(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Vec<(String, String)>, Vec<u8>)
{
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, headers, bytes)
}

async fn call(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Value)
{
    let (status, _, bytes) = raw(app, m, path, token, body).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn token(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().unwrap().to_string()
}

/// Занятие с одной ошибкой и нарочно кривым описанием: нужно, чтобы
/// в отчёте было что показать и в замечаниях, и в грамматике.
async fn finished_session(app: &Router, pool: &PgPool) -> Option<String> {
    let tok = token(app, "student1").await;
    let scenario: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;

    let (_, s) = call(app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    let session = s["session"]["id"].as_str().unwrap().to_string();

    // Спрашиваем один факт: в отчёте должны появиться и вопрос, и ответ
    let fact = s["scenario"]["questions"][0]["factId"].as_str().unwrap();
    call(app, Method::POST, &format!("/api/sessions/{session}/ask"), Some(&tok),
        Some(json!({ "factId": fact }))).await;

    call(app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({
            // Двойной пробел и строчная буква — на них сработает
            // проверка грамматики
            "description": "горит крыша  частного дома",
            "address": { "settlement": "Королёв", "street": "Станционная", "house": "28" },
            "typeIds": ["101"],
            "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                         "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                         "inhouse": ["Крыша"], "gasified": ["Да"] }
        }))).await;

    call(app, Method::POST, &format!("/api/sessions/{session}/save"), Some(&tok), None).await;
    Some(session)
}

/// Все пять блоков, которые перечисляет ТЗ.
#[tokio::test]
async fn отчёт_по_занятию_содержит_всё_что_требует_тз() {
    let Some((app, pool)) = app().await else { return };
    let Some(session) = finished_session(&app, &pool).await else { return };
    let tok = token(&app, "student1").await;

    let (status, r) = call(&app, Method::GET,
        &format!("/api/reports/session/{session}"), Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK, "{r}");

    // 1. Действия обучающегося
    let actions = r["actions"].as_array().unwrap();
    assert!(actions.iter().any(|a| a["kind"] == "вопрос"));
    assert!(actions.iter().any(|a| a["kind"] == "ответ заявителя"));
    assert!(actions.iter().any(|a| a["kind"] == "сохранение"));

    // 2. Замечания
    assert!(r["findings"].as_array().unwrap().iter().any(|f| f["kind"] == "wrong"),
        "ошибка в газификации должна попасть в замечания");

    // 3. Время заполнения и 4. отличие от норматива
    assert!(r["timing"]["elapsedSec"].is_number());
    assert_eq!(r["timing"]["normSec"], 60);
    assert_eq!(
        r["timing"]["diffSec"].as_i64().unwrap(),
        r["timing"]["elapsedSec"].as_i64().unwrap() - 60,
    );

    // 5. Результат проверки грамматики
    assert!(r["grammar"]["total"].as_u64().unwrap() > 0,
        "в описании двойной пробел и строчная буква");
    assert!(r["grammar"]["checkedFields"].as_array().unwrap()
        .iter().any(|f| f == "Описание со слов заявителя"));
}

/// Преподаватель видит занятие любого курсанта: ему по нему и учить.
#[tokio::test]
async fn преподаватель_видит_чужое_занятие_а_курсант_нет() {
    let Some((app, pool)) = app().await else { return };
    let Some(session) = finished_session(&app, &pool).await else { return };
    let url = format!("/api/reports/session/{session}");

    let teacher = token(&app, "teacher1").await;
    let (status, _) = call(&app, Method::GET, &url, Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);

    // Чужое занятие для курсанта не существует — 404, а не 403
    let other = token(&app, "student2").await;
    let (status, _) = call(&app, Method::GET, &url, Some(&other), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Выгрузку открывают в Excel на русской локали: без BOM кириллица
/// читается как «Ð¿Ð¾», с запятой вместо точки с запятой строка
/// сваливается в одну ячейку.
#[tokio::test]
async fn csv_открывается_в_excel() {
    let Some((app, pool)) = app().await else { return };
    let Some(session) = finished_session(&app, &pool).await else { return };
    let tok = token(&app, "student1").await;

    let (status, headers, bytes) = raw(&app, Method::GET,
        &format!("/api/reports/session/{session}?format=csv"), Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK);

    let header = |name: &str| headers.iter()
        .find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap_or_default();
    assert!(header("content-type").contains("text/csv"));
    assert!(header("content-disposition").contains("attachment"));

    assert_eq!(&bytes[..3], b"\xef\xbb\xbf", "нет BOM — Excel сломает кириллицу");
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("Отличие от норматива, с;"));
    assert!(text.contains("\r\n"), "Excel ждёт CRLF");
    assert!(text.contains("угадал") || text.contains("ошибка"));
}

/// Отчёт по группе считается в очереди: норматив ТЗ на подготовку —
/// 30 секунд, держать соединение всё это время незачем.
#[tokio::test]
async fn отчёт_по_группе_считается_в_очереди() {
    let Some((app, pool)) = app().await else { return };
    let Some(_) = finished_session(&app, &pool).await else { return };

    let teacher = token(&app, "teacher1").await;
    let (_, groups) = call(&app, Method::GET, "/api/groups", Some(&teacher), None).await;
    let Some(group) = groups.as_array().and_then(|g| g.first()) else { return };
    let group_id = group["id"].as_str().unwrap();

    let (status, accepted) = call(&app, Method::POST,
        &format!("/api/reports/group/{group_id}"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    let job = accepted["jobId"].as_str().unwrap().to_string();

    // Воркер поднят отдельным процессом, в тестах его нет — считаем
    // ту же статистику напрямую и проверяем, что она осмысленна
    let stats = arm112::reports::group::stats(
        &pool, &json!({ "groupId": group_id })).await.unwrap();
    assert_eq!(stats.members, 5);
    assert!(stats.sessions > 0);
    assert!(stats.guessed_share >= 0.0 && stats.guessed_share <= 1.0);
    assert_eq!(stats.students.len(), 5, "в списке должны быть все, даже не занимавшиеся");

    // Задача видна автору
    let (status, view) = call(&app, Method::GET, &format!("/api/jobs/{job}"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["kind"], "report.group");
}

/// Отчёт по группе — дело преподавателя.
#[tokio::test]
async fn курсант_не_заказывает_отчёт_по_группе() {
    let Some((app, _)) = app().await else { return };
    let teacher = token(&app, "teacher1").await;
    let (_, groups) = call(&app, Method::GET, "/api/groups", Some(&teacher), None).await;
    let Some(group) = groups.as_array().and_then(|g| g.first()) else { return };
    let group_id = group["id"].as_str().unwrap();

    let student = token(&app, "student1").await;
    let (status, _) = call(&app, Method::POST,
        &format!("/api/reports/group/{group_id}"), Some(&student), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Чужую задачу не видно: в её результате лежит статистика по группе.
#[tokio::test]
async fn чужая_задача_не_видна() {
    let Some((app, _)) = app().await else { return };
    let teacher = token(&app, "teacher1").await;
    let (_, groups) = call(&app, Method::GET, "/api/groups", Some(&teacher), None).await;
    let Some(group) = groups.as_array().and_then(|g| g.first()) else { return };

    let (_, accepted) = call(&app, Method::POST,
        &format!("/api/reports/group/{}", group["id"].as_str().unwrap()),
        Some(&teacher), None).await;
    let job = accepted["jobId"].as_str().unwrap();

    let student = token(&app, "student1").await;
    let (status, _) = call(&app, Method::GET, &format!("/api/jobs/{job}"),
        Some(&student), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Курсант в своей группе состоит и должен её видеть; чужих — нет.
///
/// Сверяем с членством в базе, а не с названием группы: раньше
/// проверка требовала, чтобы все названия содержали «УМЦ», и ломалась
/// от любой группы, заведённой соседним тестом.
#[tokio::test]
async fn курсант_видит_только_свои_группы() {
    let Some((app, pool)) = app().await else { return };

    let student = token(&app, "student1").await;
    let (status, groups) = call(&app, Method::GET, "/api/groups", Some(&student), None).await;
    assert_eq!(status, StatusCode::OK);

    let свои: Vec<String> = sqlx::query_scalar(
        "SELECT g.id::text FROM training_groups g
           JOIN group_members m ON m.group_id = g.id
           JOIN users u ON u.id = m.user_id
          WHERE u.login = 'student1'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert!(!свои.is_empty(), "курсант должен состоять хотя бы в одной группе");
    for g in groups.as_array().unwrap() {
        let id = g["id"].as_str().unwrap();
        assert!(свои.contains(&id.to_string()),
                "показана чужая группа {}", g["title"]);
    }
}
