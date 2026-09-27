//! Нейросетевой анализ ручного ввода и экспертная оценка преподавателя.
//!
//! Модель в проверках не нужна: где важен её ответ, её заменяет
//! подставная, которая отвечает заранее заданным JSON. Проверяется то,
//! что делает код вокруг модели: очередь, проверка цитат, хранение,
//! права, аудит — и что автоматический балл никто не переписывает.
//!
//! Запуск: `cargo test --test text_analysis`

use arm112::{
    api,
    config::Config,
    domain::text_analysis::{self, Kind},
    jobs::worker,
    ml::{Llm, MlSuite},
    state::AppState,
};
use async_trait::async_trait;
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

async fn app() -> Option<(Router, PgPool, AppState)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let state = AppState::new(pool.clone(), config, ml);
    Some((api::routes().with_state(state.clone()), pool, state))
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

/// Сохранённый вызов 112 курсанта student1 по билету 3 с описанием.
struct Вызов {
    token: String,
    session: String,
    card: Uuid,
    score: i64,
}

async fn сохранённый_вызов(app: &Router, pool: &PgPool, описание: &str) -> Option<Вызов> {
    let token = токен(app, "student1").await;
    let scenario: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;

    let (status, body) = call(app, Method::POST, "/api/sessions", Some(&token),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let session = body["session"]["id"].as_str().unwrap().to_string();

    call(app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&token),
        Some(json!({ "description": описание, "typeIds": ["101"] }))).await;
    let (status, body) = call(app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    Some(Вызов {
        token,
        session,
        card: body["card"]["id"].as_str().unwrap().parse().unwrap(),
        score: body["grade"]["score"].as_i64().unwrap(),
    })
}

async fn анализ(pool: &PgPool, card: Uuid, kind: &str) -> Option<(Uuid, String)> {
    sqlx::query_as("SELECT id, status FROM text_analyses WHERE card_id = $1 AND kind = $2")
        .bind(card).bind(kind).fetch_optional(pool).await.unwrap()
}

async fn задача(pool: &PgPool, analysis: Uuid) -> Option<(Uuid, String, Value)> {
    sqlx::query_as(
        "SELECT id, kind, input FROM ml_jobs
          WHERE kind = 'analysis.text' AND input ->> 'analysisId' = $1
          ORDER BY created_at DESC LIMIT 1")
        .bind(analysis.to_string()).fetch_optional(pool).await.unwrap()
}

/// Прибрать за собой задачи очереди: в проверочной базе воркера нет,
/// и они копились бы вечно.
async fn убрать_задачи(pool: &PgPool, analysis: Uuid) {
    sqlx::query("DELETE FROM ml_jobs WHERE input ->> 'analysisId' = $1")
        .bind(analysis.to_string()).execute(pool).await.ok();
}

/// Подставная модель: отвечает тем, что ей дали, и запоминает запрос.
struct Подставная {
    ответ: Value,
    запрос: std::sync::Mutex<Option<Value>>,
}

#[async_trait]
impl Llm for Подставная {
    async fn generate_scenario(&self, _: Value) -> anyhow::Result<Value> { unreachable!() }
    async fn refine_scenario(&self, _: Value) -> anyhow::Result<Value> { unreachable!() }
    async fn group_insights(&self, _: Value) -> anyhow::Result<String> { unreachable!() }
    async fn chat_json(&self, messages: Value, _: u32, schema: Option<Value>) -> anyhow::Result<Value> {
        assert!(schema.is_some(), "анализ обязан ограничивать ответ схемой");
        *self.запрос.lock().unwrap() = Some(messages);
        Ok(self.ответ.clone())
    }
}

/// Сохранение карточки ставит разбор описания в очередь — сразу,
/// не дожидаясь модели.
#[tokio::test]
async fn сохранение_карточки_ставит_анализ_в_очередь() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит крыша частного дома").await else { return };

    let (id, status) = анализ(&pool, в.card, "card112").await.expect("анализа нет");
    assert_eq!(status, "queued");
    let (_, kind, input) = задача(&pool, id).await.expect("задачи в очереди нет");
    assert_eq!(kind, text_analysis::JOB_KIND);
    assert_eq!(input["analysisId"], id.to_string());
    убрать_задачи(&pool, id).await;
}

/// На заглушке модель не зовётся, и это видно: «модель недоступна»,
/// а не пустой список, который выглядел бы как «замечаний нет».
#[tokio::test]
async fn без_модели_анализ_честно_помечен_недоступным() {
    let Some((app, pool, state)) = app().await else { return };
    if state.config.ml_provider != arm112::config::MlProvider::Mock { return }
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит крыша").await else { return };

    let (id, _) = анализ(&pool, в.card, "card112").await.unwrap();
    let (job, kind, input) = задача(&pool, id).await.unwrap();
    worker::execute(&state, &worker::Job { id: job, kind, input }).await.unwrap();
    убрать_задачи(&pool, id).await;

    let (status, body) = call(&app, Method::GET, &format!("/api/cards/{}/review", в.card),
        Some(&в.token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let попытка = &body["attempts"][0];
    assert_eq!(попытка["kind"], "card112");
    assert_eq!(попытка["analysis"]["status"], "unavailable");
    assert!(попытка["analysis"]["note"].as_str().unwrap().contains("модель"));
    assert_eq!(body["canGrade"], false, "курсант экспертную оценку не ставит");
}

/// Замечание с цитатой, которой нет ни в описании, ни в разговоре,
/// отбрасывается — и это учитывается.
#[tokio::test]
async fn ответ_модели_проверяется_по_цитатам_и_сохраняется() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит сарай, пострадал сосед").await else { return };
    let (id, _) = анализ(&pool, в.card, "card112").await.unwrap();
    убрать_задачи(&pool, id).await;

    let первая_реплика: String = sqlx::query_scalar(
        "SELECT text FROM transcript_lines t JOIN sessions s ON s.id = t.session_id
          WHERE s.id = $1::uuid AND t.who = 'заявитель' ORDER BY t.at_sec, t.id LIMIT 1")
        .bind(&в.session).fetch_one(&pool).await.unwrap();
    // Кусок настоящей реплики — цитата, которая найдётся
    let кусок: String = первая_реплика.split_whitespace().take(3).collect::<Vec<_>>().join(" ");

    let модель = Подставная {
        ответ: json!({ "findings": [
            { "kind": "contradiction", "text": "Горит не сарай.",
              "quoteInput": "Горит сарай", "quoteSource": кусок },
            { "kind": "invented", "text": "Про соседа заявитель не говорил.",
              "quoteInput": "пострадал сосед", "quoteSource": "" },
            { "kind": "missing", "text": "Выдумано моделью.",
              "quoteInput": "", "quoteSource": "этого заявитель не говорил никогда" },
        ]}),
        запрос: Default::default(),
    };
    assert_eq!(text_analysis::run(&pool, &модель, id).await.unwrap(), "ready");

    // Модель видела и описание, и слова заявителя
    let запрос = модель.запрос.lock().unwrap().clone().unwrap().to_string();
    assert!(запрос.contains("Горит сарай"));

    let v = text_analysis::view(&pool, Kind::Card112, в.card, None).await.unwrap().unwrap();
    assert_eq!(v.status, "ready");
    assert_eq!(v.findings.len(), 2, "{:?}", v.findings);
    assert_eq!(v.dropped, 1);
    assert!(v.model_ms.is_some());

    // Балл правил анализ не трогает
    let score: i32 = sqlx::query_scalar("SELECT score FROM grades WHERE session_id = $1::uuid")
        .bind(&в.session).fetch_one(&pool).await.unwrap();
    assert_eq!(score as i64, в.score);
}

/// Экспертная оценка ложится рядом с автоматической и пишется в журнал
/// действий с обоими баллами.
#[tokio::test]
async fn экспертная_оценка_не_переписывает_автоматическую_и_попадает_в_аудит() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит крыша").await else { return };
    if let Some((id, _)) = анализ(&pool, в.card, "card112").await { убрать_задачи(&pool, id).await }
    let учитель = токен(&app, "teacher1").await;
    let путь = format!("/api/cards/{}/review/expert", в.card);

    let (status, body) = call(&app, Method::POST, &путь, Some(&учитель),
        Some(json!({ "kind": "card112", "score": 55, "comment": "Не уточнил газификацию" }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["score"], 55);

    let score: i32 = sqlx::query_scalar("SELECT score FROM grades WHERE session_id = $1::uuid")
        .bind(&в.session).fetch_one(&pool).await.unwrap();
    assert_eq!(score as i64, в.score, "автоматический балл переписан");

    let (auto, expert): (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT (payload ->> 'autoScore')::bigint, (payload ->> 'score')::bigint
           FROM audit_log WHERE action = 'grade.override' AND entity_id = $1::text
          ORDER BY at DESC LIMIT 1")
        .bind(в.card).fetch_one(&pool).await.unwrap();
    assert_eq!(auto, Some(в.score));
    assert_eq!(expert, Some(55));

    // Курсант видит оба балла и комментарий
    let (_, body) = call(&app, Method::GET, &format!("/api/cards/{}/review", в.card),
        Some(&в.token), None).await;
    let попытка = &body["attempts"][0];
    assert_eq!(попытка["autoScore"].as_i64(), Some(в.score));
    assert_eq!(попытка["expert"]["score"], 55);
    assert_eq!(попытка["expert"]["comment"], "Не уточнил газификацию");

    // В отчёте по занятию — оба балла
    let (status, отчёт) = call(&app, Method::GET, &format!("/api/reports/session/{}", в.session),
        Some(&учитель), None).await;
    assert_eq!(status, StatusCode::OK, "{отчёт}");
    assert_eq!(отчёт["score"].as_i64(), Some(в.score));
    assert_eq!(отчёт["expert"]["score"], 55);
}

/// Ставит оценку только преподаватель. Администратору ТЗ запрещает
/// вмешиваться в оценки — у нас он не ставит их вовсе.
#[tokio::test]
async fn экспертную_оценку_не_ставят_курсант_и_администратор() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит крыша").await else { return };
    if let Some((id, _)) = анализ(&pool, в.card, "card112").await { убрать_задачи(&pool, id).await }
    let путь = format!("/api/cards/{}/review/expert", в.card);
    let тело = json!({ "kind": "card112", "score": 100 });

    let (status, _) = call(&app, Method::POST, &путь, Some(&в.token), Some(тело.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let админ = токен(&app, "admin").await;
    let (status, _) = call(&app, Method::POST, &путь, Some(&админ), Some(тело)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let учитель = токен(&app, "teacher1").await;
    let (status, _) = call(&app, Method::POST, &путь, Some(&учитель),
        Some(json!({ "kind": "card112", "score": 101 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "балл вне 0..100");
}

/// Преподаватель подтверждает или отклоняет замечание модели — это и
/// есть экспертная проверка нейросетевой оценки.
#[tokio::test]
async fn вердикт_преподавателя_по_замечанию_сохраняется() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит сарай").await else { return };
    let (id, _) = анализ(&pool, в.card, "card112").await.unwrap();
    убрать_задачи(&pool, id).await;
    let модель = Подставная {
        ответ: json!({ "findings": [{ "kind": "invented", "text": "Сарая не было.",
                                      "quoteInput": "Горит сарай", "quoteSource": "" }] }),
        запрос: Default::default(),
    };
    text_analysis::run(&pool, &модель, id).await.unwrap();

    let учитель = токен(&app, "teacher1").await;
    let путь = format!("/api/cards/{}/review/verdict", в.card);
    let (status, _) = call(&app, Method::POST, &путь, Some(&учитель),
        Some(json!({ "analysisId": id, "index": 0, "verdict": "confirmed" }))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, Method::POST, &путь, Some(&учитель),
        Some(json!({ "analysisId": id, "index": 5, "verdict": "confirmed" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "замечания с таким номером нет");
    let (status, _) = call(&app, Method::POST, &путь, Some(&в.token),
        Some(json!({ "analysisId": id, "index": 0, "verdict": "rejected" }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, body) = call(&app, Method::GET, &format!("/api/cards/{}/review", в.card),
        Some(&учитель), None).await;
    assert_eq!(body["canGrade"], true);
    assert_eq!(body["attempts"][0]["analysis"]["findings"][0]["verdict"], "confirmed");
}

/// Чужой разбор курсанту не показывается, даже если карточка ему видна.
#[tokio::test]
async fn чужой_разбор_курсанту_не_виден() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(в) = сохранённый_вызов(&app, &pool, "Горит крыша").await else { return };
    if let Some((id, _)) = анализ(&pool, в.card, "card112").await { убрать_задачи(&pool, id).await }

    // Сделать карточку видимой второму курсанту через его службу
    let (другой, служба): (Uuid, Option<String>) = sqlx::query_as(
        "SELECT id, service_id FROM users WHERE login = 'student2'")
        .fetch_one(&pool).await.unwrap();
    let _ = другой;
    let Some(служба) = служба else { return };
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(в.card).bind(&служба).execute(&pool).await.unwrap();

    let чужой = токен(&app, "student2").await;
    let (status, body) = call(&app, Method::GET, &format!("/api/cards/{}/review", в.card),
        Some(&чужой), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["attempts"].as_array().unwrap().is_empty(), "{body}");
}

/// Настоящая модель на настоящем вызове. Нужен поднятый сервер модели
/// (`ML_BASE_URL`, по умолчанию 127.0.0.1:8090), поэтому по умолчанию
/// пропускается:
///
/// ```text
/// cargo test --test text_analysis -- --ignored --nocapture
/// ```
///
/// Описание нарочно испорчено: заявитель сказал «горит крыша частного
/// дома», «пламя видно», «пострадавших нет», а про газ его не спрашивали.
#[tokio::test]
#[ignore = "нужен запущенный сервер модели"]
async fn настоящая_модель_находит_расхождения_в_описании() {
    let Some((app, pool, _)) = app().await else { return };
    let token = токен(&app, "student1").await;
    let scenario: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_one(&pool).await.unwrap();
    let (_, body) = call(&app, Method::POST, "/api/sessions", Some(&token),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    let session = body["session"]["id"].as_str().unwrap().to_string();
    for q in body["scenario"]["questions"].as_array().unwrap() {
        if ["victims", "flame"].contains(&q["key"].as_str().unwrap()) {
            call(&app, Method::POST, &format!("/api/sessions/{session}/ask"), Some(&token),
                Some(json!({ "factId": q["factId"] }))).await;
        }
    }
    call(&app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&token),
        Some(json!({ "description": "Горит баня на участке, есть пострадавший. Дом газифицирован.",
                     "typeIds": ["101"] }))).await;
    let (_, body) = call(&app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&token), None).await;
    let card: Uuid = body["card"]["id"].as_str().unwrap().parse().unwrap();
    let (id, _) = анализ(&pool, card, "card112").await.unwrap();
    убрать_задачи(&pool, id).await;

    let ml = MlSuite::build(arm112::config::MlProvider::Http).unwrap();
    let started = std::time::Instant::now();
    let status = text_analysis::run(&pool, ml.llm.as_ref(), id).await.unwrap();
    let v = text_analysis::view(&pool, Kind::Card112, card, None).await.unwrap().unwrap();
    println!("статус {status}, {:.1} с, отброшено {}", started.elapsed().as_secs_f64(), v.dropped);
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(status, "ready", "{:?}", v.note);
    assert!(!v.findings.is_empty(), "испорченное описание прошло без замечаний");
}

/// Диспетчер ДДС закрыл работу службы — комментарии уходят на разбор.
#[tokio::test]
async fn отказ_диспетчера_ставит_анализ_комментариев() {
    let Some((app, pool, _)) = app().await else { return };
    let метка = Uuid::new_v4().simple().to_string();
    let service = format!("проверка-ан-{}", &метка[..10]);
    let login = format!("аналитик-{}", &метка[..10]);
    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&service).execute(&pool).await.unwrap();
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Проверочный А.А.', 'student', $2
           FROM users WHERE login = 'student1' RETURNING id")
        .bind(&login).bind(&service).fetch_one(&pool).await.unwrap();
    let card: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description)
         VALUES ($1, now(), '{101}', 'проверочная карточка') RETURNING id")
        .bind(format!("А{}", &метка[..10])).fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
        .bind(card).bind(&service).execute(&pool).await.unwrap();

    let курсант = токен(&app, &login).await;
    let (status, body) = call(&app, Method::POST, &format!("/api/cards/{card}/responses"),
        Some(&курсант),
        Some(json!({ "serviceId": service, "status": "Не принята", "comment": "нет" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (id, status) = анализ(&pool, card, "dds").await.expect("анализа комментариев нет");
    assert_eq!(status, "queued");
    assert!(задача(&pool, id).await.is_some());

    // Разбор ДДС виден и курсанту, и преподавателю — как отдельная попытка
    let (_, body) = call(&app, Method::GET, &format!("/api/cards/{card}/review"),
        Some(&курсант), None).await;
    assert_eq!(body["attempts"][0]["kind"], "dds");
    assert_eq!(body["attempts"][0]["serviceId"], service.as_str());

    // Прибрать
    убрать_задачи(&pool, id).await;
    sqlx::query("DELETE FROM cards WHERE id = $1").bind(card).execute(&pool).await.ok();
    sqlx::query("UPDATE audit_log SET user_id = NULL WHERE user_id = $1").bind(user).execute(&pool).await.ok();
    sqlx::query("DELETE FROM users WHERE id = $1").bind(user).execute(&pool).await.ok();
    sqlx::query("DELETE FROM services WHERE id = $1").bind(&service).execute(&pool).await.ok();
}
