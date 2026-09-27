//! Занятие целиком: создание, вопросы заявителю, заполнение, сохранение.
//!
//! Запуск: `cargo test --test sessions_api`

use arm112::{api, config::Config, ml::MlSuite, state::AppState};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// Куда ходят проверки.
///
/// Отдельная база, если она задана: иначе прогон копит мусор в той же,
/// на которой стоит стенд, и это видно в администрировании и в
/// переписке сценариев.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, sqlx::PgPool)> {
    let config = Config::from_env().ok()?;
    let pool = sqlx::PgPool::connect(&адрес_базы(&config)).await.ok()?;
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

async fn token(app: &Router, login: &str) -> String {
    let (s, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    b["token"].as_str().unwrap().to_string()
}

/// Билет 3, вызов 1: горит крыша частного дома в Королёве.
async fn scenario_id(pool: &sqlx::PgPool) -> Option<String> {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|id| id.to_string())
}

async fn start(app: &Router, pool: &sqlx::PgPool, login: &str) -> Option<(String, Value)> {
    let tok = token(app, login).await;
    let sid = scenario_id(pool).await?;
    let (status, body) = call(app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": sid, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Some((tok, body))
}

/// Главное требование к этому эндпоинту: эталон не должен уехать
/// на клиент. Иначе метрика «выяснил или угадал» ничего не стоит.
#[tokio::test]
async fn создание_занятия_не_отдаёт_ни_эталона_ни_ответов() {
    let Some((app, pool)) = app().await else { return };
    let Some((_, body)) = start(&app, &pool, "student1").await else { return };

    let raw = body.to_string();
    assert!(!raw.contains("expected"), "эталон уехал на клиент");
    assert!(!raw.contains("Королёв"), "ответ про город виден до вопроса");
    assert!(!raw.contains("services"), "список оповещения виден заранее");

    // Вопросы отдаются — без них курсант не знает, что можно спросить
    let questions = body["scenario"]["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 5);
    assert!(questions.iter().all(|q| q["answer"].is_null()));

    // АОН известен до ответа на звонок
    assert_eq!(body["card"]["phoneAon"], "916-126-34-71");
}

#[tokio::test]
async fn вопрос_заявителю_возвращает_ответ_и_попадает_в_стенограмму() {
    let Some((app, pool)) = app().await else { return };
    let Some((tok, body)) = start(&app, &pool, "student1").await else { return };

    let session = body["session"]["id"].as_str().unwrap();
    let fact = body["scenario"]["questions"][0]["factId"].as_str().unwrap();

    let (status, ask) = call(&app, Method::POST, &format!("/api/sessions/{session}/ask"),
        Some(&tok), Some(json!({ "factId": fact }))).await;
    assert_eq!(status, StatusCode::OK, "{ask}");
    assert!(ask["answer"].as_str().unwrap().contains("Королёв"));
    // Ответ открывает поля адреса: теперь они «выяснены», а не угаданы
    assert!(ask["unlocks"].as_array().unwrap()
        .iter().any(|u| u == "address.settlement"));

    let (_, lines) = call(&app, Method::GET,
        &format!("/api/sessions/{session}/transcript"), Some(&tok), None).await;
    let lines = lines.as_array().unwrap();
    // легенда + вопрос + ответ
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["who"], "заявитель");
    assert_eq!(lines[1]["who"], "оператор");
}

/// Признаки уходят в классификатор на сервере: фронтенд не считает
/// список оповещения сам.
#[tokio::test]
async fn заполнение_признаков_подбирает_службы() {
    let Some((app, pool)) = app().await else { return };
    let Some((tok, body)) = start(&app, &pool, "student1").await else { return };
    let session = body["session"]["id"].as_str().unwrap();

    let (status, patched) = call(&app, Method::PATCH,
        &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({
            "address": { "settlement": "Королёв", "street": "Станционная", "house": "28" },
            "typeIds": ["101"],
            "answers": {
                "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                "inhouse": ["Крыша"], "gasified": ["Нет"]
            }
        }))).await;

    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["classify"]["outcome"], "пожар: частный дом");

    let services = patched["card"]["serviceIds"].as_array().unwrap();
    assert!(services.iter().any(|s| s == "mchs"), "пожарные не оповещены: {services:?}");
    assert_eq!(patched["card"]["outcomeType"], "пожар: частный дом");
}

/// Адрес приходит по кусочкам (дебаунс 500 мс), и каждый PATCH
/// должен дописывать поля, а не перетирать объект целиком.
#[tokio::test]
async fn частичный_patch_адреса_дописывает_а_не_затирает() {
    let Some((app, pool)) = app().await else { return };
    let Some((tok, body)) = start(&app, &pool, "student1").await else { return };
    let session = body["session"]["id"].as_str().unwrap();
    let url = format!("/api/sessions/{session}/card");

    call(&app, Method::PATCH, &url, Some(&tok),
        Some(json!({ "address": { "street": "Станционная" } }))).await;
    let (_, second) = call(&app, Method::PATCH, &url, Some(&tok),
        Some(json!({ "address": { "house": "28" } }))).await;

    assert_eq!(second["card"]["address"]["street"], "Станционная");
    assert_eq!(second["card"]["address"]["house"], "28");
}

/// Отметку о пострадавших надо уметь и ставить, и снимать.
/// Отсутствие ключа и `null` — разные вещи.
#[tokio::test]
async fn отметку_о_пострадавших_можно_снять() {
    let Some((app, pool)) = app().await else { return };
    let Some((tok, body)) = start(&app, &pool, "student1").await else { return };
    let session = body["session"]["id"].as_str().unwrap();
    let url = format!("/api/sessions/{session}/card");

    let (_, a) = call(&app, Method::PATCH, &url, Some(&tok),
        Some(json!({ "victims": 2 }))).await;
    assert_eq!(a["card"]["victims"], 2);

    // Ключа нет — значение не трогаем
    let (_, b) = call(&app, Method::PATCH, &url, Some(&tok),
        Some(json!({ "description": "горит крыша" }))).await;
    assert_eq!(b["card"]["victims"], 2);

    // Явный null — снимаем
    let (_, c) = call(&app, Method::PATCH, &url, Some(&tok),
        Some(json!({ "victims": null }))).await;
    assert!(c["card"]["victims"].is_null());
}

#[tokio::test]
async fn сохранение_закрывает_занятие_и_повторно_не_проходит() {
    let Some((app, pool)) = app().await else { return };
    let Some((tok, body)) = start(&app, &pool, "student1").await else { return };
    let session = body["session"]["id"].as_str().unwrap();

    let (status, saved) = call(&app, Method::POST,
        &format!("/api/sessions/{session}/save"), Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved["card"]["savedAt"].is_string());
    // Сохранение сразу отдаёт разбор: отдельно за ним ходить не нужно
    assert_eq!(saved["grade"]["normSec"], 60);
    assert!(saved["grade"]["score"].is_number());

    let (again, _) = call(&app, Method::POST,
        &format!("/api/sessions/{session}/save"), Some(&tok), None).await;
    assert_eq!(again, StatusCode::CONFLICT);

    // После сохранения карточка правится только через режим ДДС
    let (late, _) = call(&app, Method::PATCH,
        &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({ "description": "поздно" }))).await;
    assert_eq!(late, StatusCode::CONFLICT);
}

/// Чужое занятие для курсанта не существует — именно 404, а не 403:
/// по коду ответа не должно быть видно, что занятие вообще есть.
#[tokio::test]
async fn курсант_не_видит_чужого_занятия() {
    let Some((app, pool)) = app().await else { return };
    let Some((_, body)) = start(&app, &pool, "student1").await else { return };
    let session = body["session"]["id"].as_str().unwrap();

    let other = token(&app, "student2").await;
    let (status, _) = call(&app, Method::GET,
        &format!("/api/sessions/{session}/transcript"), Some(&other), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Преподаватель видит любое: ему нужен разбор
    let teacher = token(&app, "teacher1").await;
    let (status, _) = call(&app, Method::GET,
        &format!("/api/sessions/{session}/transcript"), Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn занятие_без_токена_не_создаётся() {
    let Some((app, pool)) = app().await else { return };
    let Some(sid) = scenario_id(&pool).await else { return };

    let (status, _) = call(&app, Method::POST, "/api/sessions", None,
        Some(json!({ "scenarioId": sid, "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Свой курсант на проверку: «незаконченный вызов» — последний у
/// пользователя, и на общем student1 его перехватили бы соседние
/// проверки, которые идут параллельно.
async fn свой_курсант(pool: &sqlx::PgPool) -> (uuid::Uuid, String) {
    let login = format!("курсант-f5-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role)
         SELECT $1, password_hash, 'Проверочный К.К.', 'student'
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).fetch_one(pool).await.unwrap();
    (id, login)
}

async fn убрать_курсанта(pool: &sqlx::PgPool, id: uuid::Uuid) {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1").bind(id).execute(pool).await.ok();
    sqlx::query("DELETE FROM cards WHERE created_by = $1").bind(id).execute(pool).await.ok();
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(id).execute(pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(id).execute(pool).await.ok();
}

/// Замечание со стенда: после обновления страницы экран писал
/// «Занятие не запущено», а повторный выбор билета заводил новую
/// пустую карточку. Незаконченный вызов должен возвращаться целиком:
/// тот же номер, введённые поля, заданные вопросы, разговор, время.
#[tokio::test]
async fn после_обновления_страницы_вызов_продолжается_с_того_же_места() {
    let Some((app, pool)) = app().await else { return };
    let Some(sid) = scenario_id(&pool).await else { return };
    let (user_id, login) = свой_курсант(&pool).await;
    let tok = token(&app, &login).await;

    let (_, пусто) = call(&app, Method::GET, "/api/sessions/active", Some(&tok), None).await;
    assert!(пусто.is_null(), "у нового курсанта незаконченных вызовов нет: {пусто}");

    let (_, начато) = call(&app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": sid, "mode": "card112" }))).await;
    let session = начато["session"]["id"].as_str().unwrap().to_string();
    let номер = начато["card"]["number"].clone();
    let вопрос = начато["scenario"]["questions"][0]["factId"].as_str().unwrap().to_string();

    call(&app, Method::POST, &format!("/api/sessions/{session}/ask"), Some(&tok),
        Some(json!({ "factId": вопрос }))).await;
    call(&app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&tok),
        Some(json!({ "applicantName": "Проверка восстановления",
                     "typeIds": ["101"], "answers": { "where": ["Дом"] } }))).await;

    // «Обновили страницу»: у клиента ничего нет, кроме входа
    let (status, с) = call(&app, Method::GET, "/api/sessions/active", Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK, "{с}");
    assert_eq!(с["session"]["id"], session.as_str(), "вернулся не тот вызов");
    assert_eq!(с["card"]["number"], номер, "номер карточки сменился");
    assert_eq!(с["card"]["applicantName"], "Проверка восстановления");
    assert_eq!(с["card"]["answers"]["where"], json!(["Дом"]));
    assert!(с["session"]["startedAt"].is_string(), "без начала не восстановить таймер");

    let заданные: Vec<&Value> = с["scenario"]["questions"].as_array().unwrap().iter()
        .filter(|q| q["revealed"] == true).collect();
    assert_eq!(заданные.len(), 1);
    assert_eq!(заданные[0]["factId"], вопрос.as_str());
    // Легенда, вопрос и ответ
    assert_eq!(с["transcript"].as_array().unwrap().len(), 3);
    assert!(с["classify"].is_object(), "список оповещения по выбранному типу не вернулся");
    assert!(с["grade"].is_null(), "разбор до сохранения показывать нельзя");

    // Эталон и ответы на незаданные вопросы по-прежнему не уезжают
    let raw = с.to_string();
    assert!(!raw.contains("expected"), "эталон уехал на клиент");
    assert!(!raw.contains("Королёв"), "ответ про город виден до вопроса");

    // После сохранения незаконченного нет, а состояние по номеру
    // вызова отдаёт разбор — его тоже можно открыть после обновления
    call(&app, Method::POST, &format!("/api/sessions/{session}/save"), Some(&tok), None).await;
    let (status, _) = call(&app, Method::GET, "/api/sessions/active", Some(&tok), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, после) = call(&app, Method::GET, &format!("/api/sessions/{session}"), Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(после["grade"]["score"].is_number(), "{после}");

    // Чужой вызов курсанту не отдаётся
    let чужой = token(&app, "student1").await;
    let (status, _) = call(&app, Method::GET, &format!("/api/sessions/{session}"), Some(&чужой), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    убрать_курсанта(&pool, user_id).await;
}
