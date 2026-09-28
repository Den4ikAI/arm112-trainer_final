//! Профиль рабочего места: оператор 112 и диспетчер ДДС.
//!
//! Организатор, чат 18.09: «один интерфейс, одна учётка, две мини-роли».
//! Интерфейс общий, а функционал разный, и ограничение держит сервер:
//! кнопку можно спрятать, а запрос всё равно придёт.
//!
//! Запуск: `cargo test --test duty`

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
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn token(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().expect("вход не удался").to_string()
}

/// Отказ именно по профилю, а не какой-то другой.
fn не_по_профилю(status: StatusCode, body: &Value, что: &str) {
    assert_eq!(status, StatusCode::FORBIDDEN, "{что}: {body}");
    assert_eq!(body["error"], "wrong_duty", "{что}: {body}");
    assert!(body["detail"].as_str().is_some_and(|d| d.contains("профиль")), "{что}: {body}");
}

/// Сохранённая карточка, ушедшая в МЧС (student3).
async fn карточка_мчс(pool: &PgPool) -> Uuid {
    let номер = format!("Р{}", &Uuid::new_v4().simple().to_string()[..10]);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description)
         VALUES ($1, now(), '{101}', 'проверка профилей') RETURNING id",
    )
    .bind(&номер).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, 'mchs')")
        .bind(id).execute(pool).await.unwrap();
    id
}

async fn убрать_карточку(pool: &PgPool, id: Uuid) {
    sqlx::query("DELETE FROM cards WHERE id = $1").bind(id).execute(pool).await.ok();
}

/// Оператор 112 принимает вызовы, а за ДДС не работает: ни ленты
/// службы, ни статусов, ни бригады, ни доклада начальнику.
#[tokio::test]
async fn оператор_112_не_работает_за_ддс() {
    let Some((app, pool)) = app().await else { return };
    let карточка = карточка_мчс(&pool).await;
    let оп = token(&app, "student1").await;

    let (s, b) = call(&app, Method::GET, "/api/cards?scope=service", Some(&оп), None).await;
    не_по_профилю(s, &b, "лента службы");
    let (s, b) = call(&app, Method::POST, &format!("/api/cards/{карточка}/responses"), Some(&оп),
        Some(json!({ "serviceId": "mchs", "status": "Принята" }))).await;
    не_по_профилю(s, &b, "статус реагирования");
    let (s, b) = call(&app, Method::POST, &format!("/api/cards/{карточка}/opened"),
        Some(&оп), None).await;
    не_по_профилю(s, &b, "отметка открытия");
    let (s, b) = call(&app, Method::GET, &format!("/api/cards/{карточка}/brigade"),
        Some(&оп), None).await;
    не_по_профилю(s, &b, "бригада");
    let (s, b) = call(&app, Method::POST, &format!("/api/cards/{карточка}/brigade"), Some(&оп),
        Some(json!({ "serviceId": "mchs", "brigadeNo": 1 }))).await;
    не_по_профилю(s, &b, "назначение бригады");
    let (s, b) = call(&app, Method::POST, &format!("/api/cards/{карточка}/chief/start"),
        Some(&оп), Some(json!({}))).await;
    не_по_профилю(s, &b, "доклад начальнику");

    // Своё — можно: свои вызовы и список билетов для приёма
    let (s, b) = call(&app, Method::GET, "/api/cards?scope=own", Some(&оп), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = call(&app, Method::GET, "/api/scenarios", Some(&оп), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");

    убрать_карточку(&pool, карточка).await;
}

/// Диспетчер ДДС работает с карточками своей службы, а вызовы 112 не
/// принимает и карточку 112 не заполняет.
#[tokio::test]
async fn диспетчер_ддс_не_принимает_вызовы_112() {
    let Some((app, pool)) = app().await else { return };
    let ддс = token(&app, "student3").await;
    let Some(сценарий) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM scenarios WHERE status = 'approved' ORDER BY ticket LIMIT 1")
        .fetch_optional(&pool).await.unwrap() else { return };

    let (s, b) = call(&app, Method::GET, "/api/scenarios", Some(&ддс), None).await;
    не_по_профилю(s, &b, "билеты для приёма");
    let (s, b) = call(&app, Method::POST, "/api/sessions", Some(&ддс),
        Some(json!({ "scenarioId": сценарий, "mode": "card112" }))).await;
    не_по_профилю(s, &b, "начать вызов 112");
    let (s, b) = call(&app, Method::GET, "/api/sessions/active", Some(&ддс), None).await;
    не_по_профилю(s, &b, "незаконченный вызов");
    let (s, b) = call(&app, Method::GET, "/api/cards?scope=own", Some(&ддс), None).await;
    не_по_профилю(s, &b, "свои карточки 112");
    let (s, b) = call(&app, Method::GET, "/api/forecast/me", Some(&ддс), None).await;
    не_по_профилю(s, &b, "прогноз по вызовам 112");

    // Чужой вызов оператора тоже не тронуть: ни вопрос, ни правка, ни сохранение
    let оп = token(&app, "student1").await;
    let (_, начато) = call(&app, Method::POST, "/api/sessions", Some(&оп),
        Some(json!({ "scenarioId": сценарий, "mode": "card112" }))).await;
    let session = начато["session"]["id"].as_str().unwrap().to_string();
    let факт = начато["scenario"]["questions"][0]["factId"].clone();
    let (s, b) = call(&app, Method::POST, &format!("/api/sessions/{session}/ask"), Some(&ддс),
        Some(json!({ "factId": факт }))).await;
    не_по_профилю(s, &b, "вопрос заявителю");
    let (s, b) = call(&app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&ддс),
        Some(json!({ "description": "правка диспетчера" }))).await;
    не_по_профилю(s, &b, "правка карточки 112");
    let (s, b) = call(&app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&ддс), None).await;
    не_по_профилю(s, &b, "сохранение карточки 112");

    // Своё — можно: лента своей службы
    let (s, b) = call(&app, Method::GET, "/api/cards?scope=service", Some(&ддс), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");

    // Незаконченный вызов student1 не оставляем: его подхватило бы
    // «продолжение после обновления» в соседних проверках
    for sql in ["DELETE FROM cards WHERE session_id = $1::uuid",
                "DELETE FROM sessions WHERE id = $1::uuid"] {
        sqlx::query(sql).bind(&session).execute(&pool).await.ok();
    }
}

/// Преподаватель направил курсанта на другое рабочее место — права
/// меняются сразу, без перевхода: сервер берёт профиль из базы.
#[tokio::test]
async fn смена_профиля_действует_без_перевхода() {
    let Some((app, pool)) = app().await else { return };
    let admin = token(&app, "admin").await;
    let login = format!("практикант-{}", &Uuid::new_v4().simple().to_string()[..12]);

    // Без профиля и службы — оператор 112
    let (s, заведён) = call(&app, Method::POST, "/api/admin/users", Some(&admin),
        Some(json!({ "login": login, "password": "длинныйПароль",
                     "fio": "Практикантов П.П.", "role": "student" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{заведён}");
    assert_eq!(заведён["duty"], "op112");
    let id = заведён["id"].as_str().unwrap().to_string();

    let (_, b) = call(&app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "длинныйПароль" }))).await;
    let tok = b["token"].as_str().unwrap().to_string();
    let (s, _) = call(&app, Method::GET, "/api/scenarios", Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK);

    // Направили диспетчером в МЧС
    let (s, стало) = call(&app, Method::PATCH, &format!("/api/users/{id}/duty"), Some(&admin),
        Some(json!({ "duty": "dds", "serviceId": "mchs" }))).await;
    assert_eq!(s, StatusCode::OK, "{стало}");
    assert_eq!(стало["duty"], "dds");
    assert_eq!(стало["serviceId"], "mchs");

    // Тот же токен — уже другие права
    let (s, b) = call(&app, Method::GET, "/api/scenarios", Some(&tok), None).await;
    не_по_профилю(s, &b, "после смены профиля");
    let (s, b) = call(&app, Method::GET, "/api/cards?scope=service", Some(&tok), None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, me) = call(&app, Method::GET, "/api/auth/me", Some(&tok), None).await;
    assert_eq!(me["duty"], "dds");

    // Смена — в журнале действий, с тем, что было и что стало
    let след: Option<Value> = sqlx::query_scalar(
        "SELECT payload FROM audit_log WHERE action = 'user.duty' AND entity_id = $1
          ORDER BY at DESC LIMIT 1")
        .bind(&id).fetch_optional(&pool).await.unwrap();
    let след = след.expect("смена профиля не попала в журнал");
    assert_eq!(след["было"]["duty"], "op112");
    assert_eq!(след["стало"]["duty"], "dds");

    // Преподаватель чужому курсанту профиль не меняет
    let teacher = token(&app, "teacher1").await;
    let (s, _) = call(&app, Method::PATCH, &format!("/api/users/{id}/duty"), Some(&teacher),
        Some(json!({ "duty": "op112" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1::uuid")
        .bind(&id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1::uuid").bind(&id).execute(&pool).await.ok();
}

/// Пара «профиль — служба» проверяется: диспетчеру служба обязательна,
/// оператору 112 не нужна, профиль бывает только у обучающегося.
/// Своему курсанту преподаватель профиль меняет, курсант себе — нет.
#[tokio::test]
async fn профиль_назначается_с_проверками() {
    let Some((app, pool)) = app().await else { return };
    let teacher = token(&app, "teacher1").await;
    let login = format!("курсант-{}", &Uuid::new_v4().simple().to_string()[..12]);
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role)
         SELECT $1, password_hash, 'Проверочный П.П.', 'student'
           FROM users WHERE login = 'student1' RETURNING id")
        .bind(&login).fetch_one(&pool).await.unwrap();
    // Курсант в группе преподавателя — значит, его курсант
    let группа: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id)
         SELECT $1, id FROM users WHERE login = 'teacher1' RETURNING id")
        .bind(format!("профили-{}", Uuid::new_v4().simple()))
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(группа).bind(id).execute(&pool).await.unwrap();

    let путь = format!("/api/users/{id}/duty");
    for (тело, что) in [
        (json!({ "duty": "dds" }), "диспетчер без службы"),
        (json!({ "duty": "op112", "serviceId": "mchs" }), "оператор со службой"),
        (json!({ "duty": "dds", "serviceId": "такой-службы-нет" }), "неизвестная служба"),
        (json!({ "duty": "начальник" }), "неизвестный профиль"),
    ] {
        let (s, b) = call(&app, Method::PATCH, &путь, Some(&teacher), Some(тело)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{что}: {b}");
    }

    let (s, b) = call(&app, Method::PATCH, &путь, Some(&teacher),
        Some(json!({ "duty": "dds", "serviceId": "moslift" }))).await;
    assert_eq!(s, StatusCode::OK, "своему курсанту преподаватель профиль меняет: {b}");

    // Курсант себе профиль не выбирает: на практику направляют
    let сам = token(&app, &login).await;
    let (s, _) = call(&app, Method::PATCH, &путь, Some(&сам),
        Some(json!({ "duty": "op112" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // У преподавателя профиля нет
    let teacher_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(&pool).await.unwrap();
    let admin = token(&app, "admin").await;
    let (s, _) = call(&app, Method::PATCH, &format!("/api/users/{teacher_id}/duty"), Some(&admin),
        Some(json!({ "duty": "op112" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Список группы — с профилями
    let (s, члены) = call(&app, Method::GET, &format!("/api/groups/{группа}/members"),
        Some(&teacher), None).await;
    assert_eq!(s, StatusCode::OK, "{члены}");
    assert_eq!(члены[0]["duty"], "dds");
    assert_eq!(члены[0]["serviceId"], "moslift");

    sqlx::query("DELETE FROM training_groups WHERE id = $1").bind(группа).execute(&pool).await.ok();
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1")
        .bind(id).execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(&pool).await.ok();
}
