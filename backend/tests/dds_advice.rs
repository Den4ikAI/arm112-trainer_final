//! Рекомендации диспетчеру ДДС и следующее задание от ИИ под них.
//!
//! Рекомендации финалистам, п. 5 и 10: «ИИ формирует задание →
//! обучаемый ДДС выполняет → результат → рекомендации» без
//! преподавателя. Проверяется, что рекомендация появляется по
//! замечанию разбора закрытой карточки, что следующая карточка ставит
//! ей вердикт и не переписывает его, и что по рекомендации курсанту
//! выдаётся новое происшествие от ИИ с отметкой, что оно отрабатывает.
//!
//! Каждая проверка заводит своего курсанта и свои карточки: прогон
//! идёт параллельно по одной базе.
//!
//! Запуск: `cargo test --test dds_advice`

mod common;
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
    Some((api::routes().with_state(AppState::new(pool.clone(), config, ml)), pool))
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

/// Свой курсант-диспетчер и его служба.
struct Стенд {
    service: String,
    login: String,
    user_id: Uuid,
    /// Карточки, заведённые проверкой, — прибрать
    cards: Vec<Uuid>,
}

/// `настоящая_служба` — служба диспетчера student3 (её карточки собирает
/// классификатор, и ИИ может выдать по ней задание); иначе своя
/// проверочная служба, которой классификатор ничего не даёт.
async fn стенд(pool: &PgPool, настоящая_служба: bool) -> Стенд {
    let метка = Uuid::new_v4().simple().to_string();
    let login = format!("рек-ддс-{}", &метка[..10]);
    let service = if настоящая_служба {
        sqlx::query_scalar("SELECT service_id FROM users WHERE login = 'student3'")
            .fetch_one(pool).await.unwrap()
    } else {
        let s = format!("проверка-рд-{}", &метка[..10]);
        sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
            .bind(&s).execute(pool).await.unwrap();
        s
    };
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id, duty)
         SELECT $1, password_hash, 'Проверочный Р.Д.', 'student', $2, 'dds'
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).bind(&service).fetch_one(pool).await.unwrap();
    Стенд { service, login, user_id, cards: Vec::new() }
}

/// Новая сохранённая карточка, направленная в службу и выданная курсанту.
async fn карточка(pool: &PgPool, с: &mut Стенд) -> Uuid {
    let номер = format!("Р{}", &Uuid::new_v4().simple().to_string()[..10]);
    let card: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description)
         VALUES ($1, now(), '{101}', 'проверочная карточка') RETURNING id",
    )
    .bind(&номер).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
        .bind(card).bind(&с.service).execute(pool).await.unwrap();
    common::выдать(pool, card, с.user_id).await;
    с.cards.push(card);
    card
}

async fn прибрать(pool: &PgPool, с: &Стенд) {
    for card in &с.cards {
        sqlx::query("DELETE FROM cards WHERE id = $1").bind(card).execute(pool).await.ok();
    }
    for sql in [
        "DELETE FROM dds_advice WHERE user_id = $1",
        "DELETE FROM cards WHERE id IN (SELECT card_id FROM brigade_assignments WHERE user_id = $1)",
        "DELETE FROM lessons WHERE solo_user_id = $1",
        "DELETE FROM group_members WHERE user_id = $1",
        "UPDATE audit_log SET user_id = NULL WHERE user_id = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(sql).bind(с.user_id).execute(pool).await.ok();
    }
    if с.service.starts_with("проверка-") {
        sqlx::query("DELETE FROM services WHERE id = $1").bind(&с.service).execute(pool).await.ok();
    }
}

async fn статус(app: &Router, с: &Стенд, card: Uuid, токен: &str, статус: &str) {
    let (status, ответ) = call(app, Method::POST, &format!("/api/cards/{card}/responses"),
        Some(токен), Some(json!({ "serviceId": с.service, "status": статус,
                                  "comment": "запись диспетчера" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");
}

async fn назначить(app: &Router, с: &Стенд, card: Uuid, токен: &str) {
    let (status, ответ) = call(app, Method::POST, &format!("/api/cards/{card}/brigade"), Some(токен),
        Some(json!({ "serviceId": с.service, "brigadeNo": 1 }))).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");
}

/// Все этапы давно наступили — звонок старшему раскрывает их разом.
async fn всё_наступило(pool: &PgPool, card: Uuid) {
    sqlx::query(
        "UPDATE brigade_reports r SET due_at = now() - interval '1 hour'
           FROM brigade_assignments a WHERE a.id = r.assignment_id AND a.card_id = $1",
    )
    .bind(card).execute(pool).await.unwrap();
}

/// Карточка, где статус проставлен без доклада и работа закрыта отказом:
/// одно замечание «не по факту».
async fn наугад(app: &Router, с: &Стенд, card: Uuid, токен: &str) {
    статус(app, с, card, токен, "Принята").await;
    назначить(app, с, card, токен).await;
    статус(app, с, card, токен, "Прибытие").await;
    статус(app, с, card, токен, "Отказ от выполнения работ").await;
}

/// Карточка, отработанная по докладу: статус после звонка старшему.
async fn по_факту(app: &Router, pool: &PgPool, с: &Стенд, card: Uuid, токен: &str) {
    статус(app, с, card, токен, "Принята").await;
    назначить(app, с, card, токен).await;
    всё_наступило(pool, card).await;
    let (status, ответ) = call(app, Method::POST, &format!("/api/cards/{card}/brigade/call"),
        Some(токен), Some(json!({ "serviceId": с.service }))).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    assert_eq!(ответ["learned"], "Работы завершены");
    статус(app, с, card, токен, "Работы завершены").await;
}

async fn мои(app: &Router, токен: &str) -> Vec<Value> {
    let (status, body) = call(app, Method::GET, "/api/advice/dds/me", Some(токен), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

/// Замечание разбора закрытой карточки даёт рекомендацию из памятки,
/// и повторный запрос её не дублирует.
#[tokio::test]
async fn замечание_ддс_даёт_рекомендацию() {
    let Some((app, pool)) = app().await else { return };
    let mut с = стенд(&pool, false).await;
    let курсант = токен(&app, &с.login).await;
    let card = карточка(&pool, &mut с).await;

    assert!(мои(&app, &курсант).await.is_empty(), "до первой карточки рекомендаций нет");
    наугад(&app, &с, card, &курсант).await;

    let список = мои(&app, &курсант).await;
    assert_eq!(список.len(), 1, "{список:?}");
    let р = &список[0];
    assert_eq!(р["kind"], "notByFact");
    assert_eq!(р["sourceCardId"], card.to_string());
    assert!(р["checkedAt"].is_null());
    assert!(р["text"].as_str().unwrap().contains("по факту"), "{}", р["text"]);
    assert_eq!(мои(&app, &курсант).await.len(), 1, "повтор не дублирует");

    прибрать(&pool, &с).await;
}

/// Следующая карточка ставит вердикт: ошибка ушла — рекомендация
/// сработала; вердикт один и задним числом не меняется.
#[tokio::test]
async fn следующая_карточка_ставит_вердикт() {
    let Some((app, pool)) = app().await else { return };
    let mut с = стенд(&pool, false).await;
    let курсант = токен(&app, &с.login).await;

    let первая = карточка(&pool, &mut с).await;
    наугад(&app, &с, первая, &курсант).await;
    let id = мои(&app, &курсант).await[0]["id"].as_str().unwrap().to_string();

    let вторая = карточка(&pool, &mut с).await;
    по_факту(&app, &pool, &с, вторая, &курсант).await;

    let список = мои(&app, &курсант).await;
    let р = список.iter().find(|a| a["id"] == id).expect("рекомендация на месте");
    assert_eq!(р["worked"], true, "{р}");
    assert_eq!(р["checkCardId"], вторая.to_string());
    assert!(список.iter().all(|a| a["kind"] != "notByFact" || a["id"] == id),
            "новой рекомендации на ту же ошибку нет: {список:?}");

    // Снова наугад: вердикт прежней не переписывается, выдаётся новая
    let третья = карточка(&pool, &mut с).await;
    наугад(&app, &с, третья, &курсант).await;
    let список = мои(&app, &курсант).await;
    let старая = список.iter().find(|a| a["id"] == id).unwrap();
    assert_eq!(старая["worked"], true);
    assert_eq!(старая["checkCardId"], вторая.to_string());
    let новая = список.iter().find(|a| a["id"] != id && a["kind"] == "notByFact").expect("новая");
    assert_eq!(новая["sourceCardId"], третья.to_string());
    assert!(новая["checkedAt"].is_null());

    прибрать(&pool, &с).await;
}

/// Не ушла — не сработала.
#[tokio::test]
async fn повторившаяся_ошибка_не_сработала() {
    let Some((app, pool)) = app().await else { return };
    let mut с = стенд(&pool, false).await;
    let курсант = токен(&app, &с.login).await;

    let первая = карточка(&pool, &mut с).await;
    наугад(&app, &с, первая, &курсант).await;
    let вторая = карточка(&pool, &mut с).await;
    наугад(&app, &с, вторая, &курсант).await;

    let список = мои(&app, &курсант).await;
    let проверена = список.iter().find(|a| !a["checkedAt"].is_null()).expect("вердикт есть");
    assert_eq!(проверена["worked"], false);
    assert_eq!(проверена["checkCardId"], вторая.to_string());
    // И сразу новая открытая на ту же ошибку — тренировать дальше
    assert!(список.iter().any(|a| a["checkedAt"].is_null() && a["kind"] == "notByFact"));

    прибрать(&pool, &с).await;
}

/// По рекомендации курсант берёт следующее задание от ИИ: новая
/// карточка его службы у него в ленте, с отметкой, что она отрабатывает.
/// Чужую или проверенную рекомендацию отработать нельзя.
#[tokio::test]
async fn следующее_задание_от_ии_по_рекомендации() {
    let Some((app, pool)) = app().await else { return };
    let mut с = стенд(&pool, true).await;
    let курсант = токен(&app, &с.login).await;
    let card = карточка(&pool, &mut с).await;
    наугад(&app, &с, card, &курсант).await;
    let id = мои(&app, &курсант).await[0]["id"].as_str().unwrap().to_string();

    let (status, задание) = call(&app, Method::POST, &format!("/api/advice/dds/{id}/next"),
        Some(&курсант), None).await;
    assert_eq!(status, StatusCode::CREATED, "{задание}");
    let новая = Uuid::parse_str(задание["cardId"].as_str().unwrap()).unwrap();
    assert_ne!(новая, card);

    let список = мои(&app, &курсант).await;
    assert_eq!(список[0]["nextCardId"], новая.to_string(), "{список:?}");

    // Карточка в ленте курсанта и направлена в его службу
    let (status, работы) = call(&app, Method::GET, "/api/me/work?section=training",
        Some(&курсант), None).await;
    assert_eq!(status, StatusCode::OK, "{работы}");
    assert!(работы.as_array().unwrap().iter().any(|w| w["cardId"] == новая.to_string()), "{работы}");
    let в_службе: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM card_services WHERE card_id = $1 AND service_id = $2)")
        .bind(новая).bind(&с.service).fetch_one(&pool).await.unwrap();
    assert!(в_службе);

    // Чужая рекомендация — 404
    let чужой = стенд(&pool, true).await;
    let чужой_токен = токен(&app, &чужой.login).await;
    let (status, _) = call(&app, Method::POST, &format!("/api/advice/dds/{id}/next"),
        Some(&чужой_токен), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    прибрать(&pool, &чужой).await;
    прибрать(&pool, &с).await;
}

/// Преподавателю — доля сработавших по группе и лента проверенных.
#[tokio::test]
async fn преподаватель_видит_долю_сработавших_по_группе() {
    let Some((app, pool)) = app().await else { return };
    let mut с = стенд(&pool, false).await;
    let курсант = токен(&app, &с.login).await;
    let первая = карточка(&pool, &mut с).await;
    наугад(&app, &с, первая, &курсант).await;
    let вторая = карточка(&pool, &mut с).await;
    по_факту(&app, &pool, &с, вторая, &курсант).await;
    мои(&app, &курсант).await;

    let group: Uuid = sqlx::query_scalar("SELECT group_id FROM group_members WHERE user_id = $1")
        .bind(с.user_id).fetch_one(&pool).await.unwrap();
    let teacher = токен(&app, "teacher1").await;
    let (status, body) = call(&app, Method::GET, &format!("/api/advice/dds/group/{group}"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["efficacy"]["checked"], 1);
    assert_eq!(body["efficacy"]["worked"], 1);
    assert_eq!(body["efficacy"]["tooFew"], true);
    assert_eq!(body["recent"][0]["worked"], true);
    assert_eq!(body["recent"][0]["fio"], "Проверочный Р.Д.");

    let (status, _) = call(&app, Method::GET, &format!("/api/advice/dds/group/{group}"),
        Some(&курсант), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    прибрать(&pool, &с).await;
}
