//! Рекомендации курсанту и проверка того, сработали ли они.
//!
//! Подпункт критерия финальной экспертизы: «проверка рекомендаций
//! сервиса и реальных рекомендаций в соответствии с реальными
//! условиями». Проверяется не сама рекомендация, а то, что она
//! сверяется со следующим разбором и что вердикт не переписывается.
//!
//! У каждой проверки свой курсант: история разборов у рекомендаций
//! личная, и общая учётка смешала бы параллельные прогоны.
//!
//! Запуск: `cargo test --test advice`

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

/// Куда ходят проверки: отдельная база, если она задана.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
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

/// Свой курсант на проверку: пароль как у учебных учёток.
async fn курсант(pool: &PgPool) -> (Uuid, String) {
    let login = format!("рекомендации-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Проверочный Р.Р.', 'student', service_id
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login)
    .fetch_one(pool)
    .await
    .unwrap();
    (id, login)
}

async fn убрать(pool: &PgPool, user_id: Uuid, group: Option<Uuid>) {
    if let Some(g) = group {
        sqlx::query("DELETE FROM training_groups WHERE id = $1").bind(g).execute(pool).await.ok();
    }
    for sql in [
        "DELETE FROM advice WHERE user_id = $1",
        "DELETE FROM predictions WHERE user_id = $1",
        "DELETE FROM sessions WHERE user_id = $1",
        "UPDATE audit_log SET user_id = NULL WHERE user_id = $1",
        "UPDATE cards SET created_by = NULL WHERE created_by = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(sql).bind(user_id).execute(pool).await.ok();
    }
}

/// Эталон билета 3: горит крыша частного дома в Королёве.
fn верная_карточка() -> Value {
    json!({
        "address": { "region": "Московская область", "settlement": "Королёв",
                     "street": "Станционная", "house": "28" },
        "typeIds": ["101"],
        "answers": {
            "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
            "house-kind": ["Дом частный"], "people-threat": ["Нет"],
            "inhouse": ["Крыша"], "gasified": ["Нет"]
        }
    })
}

/// Отработать вызов билета 3. `спросить` — задать все вопросы
/// заявителю: тогда верные значения выяснены, а не угаданы.
async fn отработать(app: &Router, pool: &PgPool, token: &str, спросить: bool) -> Option<String> {
    let scenario: Uuid = sqlx::query_scalar("SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;
    let (status, body) = call(app, Method::POST, "/api/sessions", Some(token),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["session"]["id"].as_str()?.to_string();

    if спросить {
        for q in body["scenario"]["questions"].as_array()? {
            call(app, Method::POST, &format!("/api/sessions/{id}/ask"), Some(token),
                Some(json!({ "factId": q["factId"] }))).await;
        }
    }
    call(app, Method::PATCH, &format!("/api/sessions/{id}/card"), Some(token),
        Some(верная_карточка())).await;
    let (status, body) = call(app, Method::POST, &format!("/api/sessions/{id}/save"),
        Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Some(id)
}

async fn мои(app: &Router, token: &str) -> Vec<Value> {
    let (status, body) = call(app, Method::GET, "/api/advice/me", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

/// Одна ошибка — случайность: после первого разбора рекомендаций нет.
/// Повторилась — после второго появляется рекомендация.
#[tokio::test]
async fn рекомендация_появляется_когда_ошибка_повторилась() {
    let Some((app, pool)) = app().await else { return };
    let (user_id, login) = курсант(&pool).await;
    let t = токен(&app, &login).await;

    let Some(_) = отработать(&app, &pool, &t, false).await else { return };
    assert!(мои(&app, &t).await.is_empty(), "одна ошибка — ещё не привычка");

    let второй = отработать(&app, &pool, &t, false).await.unwrap();
    let выдано = мои(&app, &t).await;
    assert!(!выдано.is_empty(), "угаданное повторилось — должна быть рекомендация");
    assert!(выдано.len() <= 3, "больше трёх открытых курсант не удержит");
    for р in &выдано {
        assert_eq!(р["kind"], "guessed", "{р}");
        assert_eq!(р["sourceSessionId"], второй.as_str());
        assert!(р["checkedAt"].is_null(), "проверять ещё нечем");
        let текст = р["text"].as_str().unwrap();
        assert!(текст.contains(р["label"].as_str().unwrap()),
                "формулировка строится из подписи поля: {текст}");
    }

    убрать(&pool, user_id, None).await;
}

/// Следующий разбор ставит вердикт: ошибка ушла — рекомендация сработала.
#[tokio::test]
async fn выполненная_рекомендация_засчитывается_следующим_разбором() {
    let Some((app, pool)) = app().await else { return };
    let (user_id, login) = курсант(&pool).await;
    let t = токен(&app, &login).await;

    if отработать(&app, &pool, &t, false).await.is_none() { return }
    отработать(&app, &pool, &t, false).await.unwrap();
    let выдано = мои(&app, &t).await.len();
    assert!(выдано > 0);

    // На этот раз всё выяснено у заявителя — угаданного нет
    let третий = отработать(&app, &pool, &t, true).await.unwrap();
    let после = мои(&app, &t).await;
    let проверенные: Vec<&Value> = после.iter().filter(|р| !р["checkedAt"].is_null()).collect();
    assert_eq!(проверенные.len(), выдано, "каждая рекомендация проверена следующим разбором");
    for р in проверенные {
        assert_eq!(р["worked"], true, "ошибка ушла: {р}");
        assert_eq!(р["checkSessionId"], третий.as_str());
        assert!(р["checkedAt"].as_str().unwrap() > р["madeAt"].as_str().unwrap(),
                "вердикт позже рекомендации");
    }

    убрать(&pool, user_id, None).await;
}

/// Ошибка повторилась — рекомендация не сработала, и вердикт потом
/// не переписывается, сколько бы разборов ни прошло.
#[tokio::test]
async fn невыполненная_остаётся_невыполненной() {
    let Some((app, pool)) = app().await else { return };
    let (user_id, login) = курсант(&pool).await;
    let t = токен(&app, &login).await;

    if отработать(&app, &pool, &t, false).await.is_none() { return }
    отработать(&app, &pool, &t, false).await.unwrap();
    let первые: Vec<String> = мои(&app, &t).await.iter()
        .map(|р| р["id"].as_str().unwrap().to_string()).collect();

    // Снова угадал
    отработать(&app, &pool, &t, false).await.unwrap();
    let вердикт = |все: &[Value], id: &str| все.iter()
        .find(|р| р["id"] == id).map(|р| р["worked"].clone()).unwrap();
    let после = мои(&app, &t).await;
    for id in &первые {
        assert_eq!(вердикт(&после, id), json!(false), "ошибка повторилась — не сработала");
    }
    // Ошибка всё ещё повторяется — на неё выдана свежая рекомендация
    assert!(после.iter().any(|р| р["checkedAt"].is_null()));

    // Теперь исправился — прежний вердикт от этого не меняется
    отработать(&app, &pool, &t, true).await.unwrap();
    let позже = мои(&app, &t).await;
    for id in &первые {
        assert_eq!(вердикт(&позже, id), json!(false), "вердикт задним числом не переписывается");
    }

    убрать(&pool, user_id, None).await;
}

/// Преподаватель видит долю сработавших по группе рядом с базовой линией.
#[tokio::test]
async fn доля_по_группе_считается_по_проверенным() {
    let Some((app, pool)) = app().await else { return };
    let (user_id, login) = курсант(&pool).await;
    let t = токен(&app, &login).await;

    let teacher: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(&pool).await.unwrap();
    let group: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id")
        .bind(format!("рекомендации-{}", Uuid::new_v4().simple()))
        .bind(teacher)
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(group).bind(user_id).execute(&pool).await.unwrap();

    let учитель = токен(&app, "teacher1").await;
    let (_, пусто) = call(&app, Method::GET, &format!("/api/advice/group/{group}"),
        Some(&учитель), None).await;
    assert_eq!(пусто["efficacy"]["checked"], 0);
    assert!(пусто["efficacy"]["rate"].is_null(), "не из чего считать — нечего показывать");

    if отработать(&app, &pool, &t, false).await.is_none() { return }
    отработать(&app, &pool, &t, false).await.unwrap();
    отработать(&app, &pool, &t, true).await.unwrap();

    let (status, ответ) = call(&app, Method::GET, &format!("/api/advice/group/{group}"),
        Some(&учитель), None).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    let э = &ответ["efficacy"];
    let проверено = э["checked"].as_u64().unwrap();
    assert!(проверено > 0);
    assert_eq!(э["worked"].as_u64().unwrap(), проверено);
    assert_eq!(э["rate"], json!(1.0));
    assert_eq!(э["tooFew"], true, "по нескольким случаям судить рано");
    assert!(э["baselineChecked"].is_number(), "базовая линия считается всегда");
    assert_eq!(ответ["recent"].as_array().unwrap().len() as u64, проверено);

    // Курсанту чужая аналитика недоступна
    let (status, _) = call(&app, Method::GET, &format!("/api/advice/group/{group}"),
        Some(&t), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    убрать(&pool, user_id, Some(group)).await;
}

/// Окно рекомендаций — настраиваемый параметр, в пределах.
#[tokio::test]
async fn окно_рекомендаций_настраивается() {
    let Some((app, _)) = app().await else { return };
    let admin = токен(&app, "admin").await;

    let (_, список) = call(&app, Method::GET, "/api/settings", Some(&admin), None).await;
    assert!(список.as_array().unwrap().iter().any(|s| s["key"] == "advice.window"));

    let (status, _) = call(&app, Method::PATCH, "/api/settings/advice.window",
        Some(&admin), Some(json!({ "value": 1 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "в окне из одного разбора повторов не бывает");
}
