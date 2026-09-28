//! Сверка описания курсанта кросс-энкодером и утверждения фактов вызова.
//!
//! Настоящий кросс-энкодер в проверках не нужен: его заменяет подставной,
//! который отвечает по заранее заданным правилам. Проверяется то, что
//! делает код вокруг модели: какие факты сверяются, как вердикты
//! превращаются в замечания с цитатами, что пишется в разбор, кто может
//! править утверждения и как очередь выбирает, чем разбирать.
//!
//! Запуск: `cargo test --test fact_statements`

use arm112::{
    api,
    config::Config,
    domain::{fact_statements, scenario_edit, text_analysis},
    jobs::worker,
    ml::{
        nli::{Entailment, HttpNli, Nli},
        Llm, MlSuite,
    },
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
use std::collections::HashMap;
use tower::ServiceExt;
use uuid::Uuid;

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app_with(nli: Option<Box<dyn Nli>>) -> Option<(Router, PgPool, AppState)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let mut ml = MlSuite::build(config.ml_provider).ok()?;
    if let Some(nli) = nli {
        ml.nli = nli;
    }
    let state = AppState::new(pool.clone(), config, ml);
    Some((api::routes().with_state(state.clone()), pool, state))
}

async fn app() -> Option<(Router, PgPool, AppState)> {
    app_with(None).await
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

/// Своя копия учебного вызова «горит крыша частного дома» (билет 3,
/// вызов 1): проверки правят утверждения и ответы, а на оригинал
/// опираются соседние проверки.
async fn копия_вызова(pool: &PgPool) -> Option<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO scenarios (legend, applicant_name, applicant_phone, applicant_status,
                                norm_sec, expected, version_id, status)
         SELECT legend, applicant_name, applicant_phone, applicant_status,
                norm_sec, expected, version_id, 'approved'
           FROM scenarios WHERE ticket = 3 AND call_no = 1
         RETURNING id",
    )
    .fetch_optional(pool)
    .await
    .ok()??;
    sqlx::query(
        "INSERT INTO scenario_hidden_facts (scenario_id, key, question, answer, unlocks, sort_order)
         SELECT $1, key, question, answer, unlocks, sort_order FROM scenario_hidden_facts
          WHERE scenario_id = (SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1)
            AND NOT retired",
    )
    .bind(id)
    .execute(pool)
    .await
    .ok()?;
    Some(id)
}

async fn убрать_задачи_вызова(pool: &PgPool, scenario: Uuid) {
    sqlx::query("DELETE FROM ml_jobs WHERE input ->> 'scenarioId' = $1")
        .bind(scenario.to_string()).execute(pool).await.ok();
}

/// Утверждения, как их подготовила бы модель, — по фактам вызова.
fn утверждения() -> HashMap<String, String> {
    HashMap::from([
        ("city".into(), "Происшествие в городе Королёв Московской области.".into()),
        ("victims".into(), "Пострадавших нет, все вышли.".into()),
        ("gas".into(), "Дом не газифицирован.".into()),
        ("house".into(), "Дом частный, одноэтажный, горит крыша.".into()),
        ("flame".into(), "Видно открытое пламя.".into()),
    ])
}

/// Подставная языковая модель: отвечает заданным JSON.
struct Подставная(Value);

#[async_trait]
impl Llm for Подставная {
    async fn generate_scenario(&self, _: Value) -> anyhow::Result<Value> { unreachable!() }
    async fn refine_scenario(&self, _: Value) -> anyhow::Result<Value> { unreachable!() }
    async fn group_insights(&self, _: Value) -> anyhow::Result<String> { unreachable!() }
    async fn chat_json(&self, _: Value, _: u32, schema: Option<Value>) -> anyhow::Result<Value> {
        assert!(schema.is_some(), "утверждения обязаны идти по схеме");
        Ok(self.0.clone())
    }
}

/// Подставной кросс-энкодер. Противоречие — если в посылке упомянут
/// пострадавший, а гипотеза говорит, что их нет, или в посылке баня, а
/// гипотеза про крышу дома. Подтверждение — если гипотеза дословно
/// начинается с того, что есть в посылке. Иначе — молчит.
struct Подставной;

fn вероятности(premise: &str, h: &str) -> Entailment {
    let p = premise.to_lowercase();
    let h = h.to_lowercase();
    let (c, e, n) = if (p.contains("есть пострадавш") && h.contains("пострадавших нет"))
        || (p.contains("баня") && h.contains("крыша"))
    {
        (0.9, 0.05, 0.05)
    } else if p.contains(h.trim_end_matches('.')) {
        (0.05, 0.9, 0.05)
    } else {
        (0.05, 0.05, 0.9)
    };
    Entailment { contradiction: c, entailment: e, neutral: n }
}

#[async_trait]
impl Nli for Подставной {
    async fn classify(&self, premise: &str, hypotheses: &[String]) -> anyhow::Result<Vec<Entailment>> {
        Ok(hypotheses.iter().map(|h| вероятности(premise, h)).collect())
    }
    fn available(&self) -> bool {
        true
    }
}

/// Курсант student1 ведёт вызов по копии: задаёт один вопрос и
/// сохраняет карточку с описанием. Возвращает карточку и строку анализа.
async fn вызов_с_описанием(app: &Router, pool: &PgPool, scenario: Uuid, вопрос: &str, описание: &str)
    -> (Uuid, Uuid)
{
    let token = токен(app, "student1").await;
    let (status, body) = call(app, Method::POST, "/api/sessions", Some(&token),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let session = body["session"]["id"].as_str().unwrap().to_string();
    let fact = body["scenario"]["questions"].as_array().unwrap().iter()
        .find(|q| q["key"] == вопрос).unwrap()["factId"].as_str().unwrap().to_string();
    call(app, Method::POST, &format!("/api/sessions/{session}/ask"), Some(&token),
        Some(json!({ "factId": fact }))).await;
    call(app, Method::PATCH, &format!("/api/sessions/{session}/card"), Some(&token),
        Some(json!({ "description": описание, "typeIds": ["101"] }))).await;
    let (status, body) = call(app, Method::POST, &format!("/api/sessions/{session}/save"),
        Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let card: Uuid = body["card"]["id"].as_str().unwrap().parse().unwrap();
    let analysis: Uuid = sqlx::query_scalar(
        "SELECT id FROM text_analyses WHERE card_id = $1 AND kind = 'card112'")
        .bind(card).fetch_one(pool).await.unwrap();
    sqlx::query("DELETE FROM ml_jobs WHERE input ->> 'analysisId' = $1")
        .bind(analysis.to_string()).execute(pool).await.ok();
    (card, analysis)
}

/// Модель готовит утверждения, код пишет их к своим фактам и легенде.
#[tokio::test]
async fn утверждения_готовит_модель_и_они_ложатся_к_фактам() {
    let Some((_, pool, _)) = app().await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };

    let модель = Подставная(json!({
        "legend": ["Горит крыша частного дома.", "Адрес: Станционная улица, дом 28."],
        "facts": утверждения().into_iter()
            .map(|(k, v)| json!({ "key": k, "statement": v })).collect::<Vec<_>>(),
    }));
    let итог = fact_statements::run(&pool, &модель, scenario).await.unwrap();
    assert_eq!(итог["facts"], 5, "{итог}");

    let s = scenario_edit::load(&pool, scenario).await.unwrap();
    assert_eq!(s.legend_statements.as_deref().map(|v| v.len()), Some(2));
    let газ = s.hidden.iter().find(|f| f.key == "gas").unwrap();
    assert_eq!(газ.statement.as_deref(), Some("Дом не газифицирован."));
    assert_eq!(газ.answer, "Нет, дом не газифицирован.", "ответ заявителя не тронут");

    убрать_задачи_вызова(&pool, scenario).await;
}

/// Утверждение готовилось к ответу. Поправили ответ — старое
/// утверждение ему уже не соответствует и сбрасывается, а подготовка
/// встаёт в очередь. Остальные факты не трогаются.
#[tokio::test]
async fn правка_ответа_сбрасывает_его_утверждение() {
    let Some((_, pool, _)) = app().await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };
    let легенда = vec!["Горит крыша частного дома.".to_string()];
    fact_statements::save_manual(&pool, scenario, Some(легенда.as_slice()), &утверждения())
        .await.unwrap();

    let mut hidden = scenario_edit::load(&pool, scenario).await.unwrap().hidden;
    for f in hidden.iter_mut().filter(|f| f.key == "gas") {
        f.answer = "Да, газ подведён.".into();
    }
    let version: Uuid = sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
        .fetch_one(&pool).await.unwrap();
    let patch: scenario_edit::Patch = serde_json::from_value(json!({
        "legend": "Горит баня на участке!",
        "hidden": hidden,
    })).unwrap();
    let s = scenario_edit::apply(&pool, version, scenario, &patch).await.unwrap();

    let газ = s.hidden.iter().find(|f| f.key == "gas").unwrap();
    assert_eq!(газ.statement, None, "утверждение к прежнему ответу снято");
    let люди = s.hidden.iter().find(|f| f.key == "victims").unwrap();
    assert_eq!(люди.statement.as_deref(), Some("Пострадавших нет, все вышли."));
    assert_eq!(s.legend_statements, None, "легенду поправили — её утверждения сняты");

    let в_очереди: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ml_jobs WHERE kind = 'scenario.statements'
            AND status = 'queued' AND input ->> 'scenarioId' = $1")
        .bind(scenario.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(в_очереди, 1, "подготовка заново — одной задачей, без повторов");

    убрать_задачи_вызова(&pool, scenario).await;
}

/// Кросс-энкодер находит противоречие и упущенное, и каждое замечание
/// цитирует и описание, и слова заявителя. Сверяются только факты,
/// которые курсант слышал: легенда и ответ на заданный вопрос.
#[tokio::test]
async fn кросс_энкодер_находит_противоречие_и_упущенное() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };
    let легенда = vec!["Горит крыша частного дома.".to_string(),
                       "Адрес: Станционная улица, дом 28.".to_string()];
    fact_statements::save_manual(&pool, scenario, Some(легенда.as_slice()), &утверждения())
        .await.unwrap();

    let (_, analysis) = вызов_с_описанием(&app, &pool, scenario, "victims",
        "Горит баня на участке. Есть пострадавший.").await;
    let status = text_analysis::run_nli(&pool, &Подставной, analysis).await.unwrap();
    assert_eq!(status, "ready");

    let (findings, engine, note): (Value, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT findings, engine, note FROM text_analyses WHERE id = $1")
        .bind(analysis).fetch_one(&pool).await.unwrap();
    assert_eq!(engine.as_deref(), Some("nli"));
    assert_eq!(note, None, "все утверждения подготовлены — оговорок нет");

    let list = findings.as_array().unwrap();
    let противоречия: Vec<&Value> = list.iter().filter(|f| f["kind"] == "contradiction").collect();
    assert_eq!(противоречия.len(), 2, "баня вместо крыши и пострадавший: {findings}");
    assert!(противоречия.iter().any(|f| f["quoteInput"] == "Есть пострадавший"
        && f["quoteSource"].as_str().unwrap().contains("пострадавших нет")), "{findings}");
    assert!(противоречия.iter().any(|f| f["quoteInput"] == "Горит баня на участке"), "{findings}");

    // Адрес из легенды в описании не записан — пропуск. Остальные факты
    // (газ, пламя, город) курсант не спрашивал — их нет среди замечаний
    let пропуски: Vec<&Value> = list.iter().filter(|f| f["kind"] == "missing").collect();
    assert!(пропуски.iter().all(|f| !f["text"].as_str().unwrap().contains("газифицирован")),
        "неуслышанное не может быть упущено: {findings}");
}

/// Утверждений ещё нет — сверка идёт по сырым ответам заявителя, и
/// разбор прямо говорит, что точность ниже.
#[tokio::test]
async fn без_утверждений_сверка_идёт_по_ответам_и_это_написано() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };

    let (_, analysis) = вызов_с_описанием(&app, &pool, scenario, "victims",
        "Горит крыша частного дома.").await;
    text_analysis::run_nli(&pool, &Подставной, analysis).await.unwrap();

    let note: Option<String> = sqlx::query_scalar("SELECT note FROM text_analyses WHERE id = $1")
        .bind(analysis).fetch_one(&pool).await.unwrap();
    let note = note.expect("оговорка о сырых ответах обязательна");
    assert!(note.contains("2 из 2"), "{note}");
    assert!(note.contains("точность ниже"), "{note}");
}

/// Очередь выбирает кросс-энкодер, если он подключён и настройка
/// «авто»; без него и без языковой модели честно пишет, чего нет.
#[tokio::test]
async fn очередь_выбирает_кросс_энкодер_когда_он_подключён() {
    let Some((router, pool, state)) = app_with(Some(Box::new(Подставной))).await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };
    let (_, analysis) = вызов_с_описанием(&router, &pool, scenario, "victims",
        "Горит крыша частного дома.").await;

    let job = worker::Job {
        id: Uuid::new_v4(),
        kind: text_analysis::JOB_KIND.into(),
        input: json!({ "analysisId": analysis }),
    };
    let out = worker::execute(&state, &job).await.unwrap();
    assert_eq!(out["status"], "ready", "{out}");
    let engine: Option<String> = sqlx::query_scalar("SELECT engine FROM text_analyses WHERE id = $1")
        .bind(analysis).fetch_one(&pool).await.unwrap();
    assert_eq!(engine.as_deref(), Some("nli"));

    // Тот же анализ без кросс-энкодера на заглушке языковой модели.
    // Кросс-энкодер снят явно: NLI_BASE_URL в окружении подключил бы его
    let Some((_, _, без)) = app_with(Some(Box::new(arm112::ml::nli::NoNli))).await else { return };
    if без.config.ml_provider == arm112::config::MlProvider::Mock {
        let out = worker::execute(&без, &job).await.unwrap();
        assert_eq!(out["status"], "unavailable", "{out}");
    }
}

/// Утверждения правит преподаватель; курсант — нет; факт, которого у
/// вызова нет, — ошибка, а не молча проглоченная правка.
#[tokio::test]
async fn утверждения_правит_только_преподаватель() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };
    let путь = format!("/api/scenarios/{scenario}/statements");

    let курсант = токен(&app, "student1").await;
    let (status, _) = call(&app, Method::PUT, &путь, Some(&курсант),
        Some(json!({ "facts": { "gas": "Дом не газифицирован." } }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let препод = токен(&app, "teacher1").await;
    let (status, body) = call(&app, Method::PUT, &путь, Some(&препод),
        Some(json!({ "legendStatements": ["Горит крыша частного дома."],
                     "facts": { "gas": "Дом не газифицирован." } }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["legendStatements"], json!(["Горит крыша частного дома."]));
    let газ = body["hidden"].as_array().unwrap().iter().find(|f| f["key"] == "gas").unwrap();
    assert_eq!(газ["statement"], "Дом не газифицирован.");

    let (status, _) = call(&app, Method::PUT, &путь, Some(&препод),
        Some(json!({ "facts": { "нет_такого": "что-то" } }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = call(&app, Method::POST, &format!("{путь}/prepare"), Some(&препод), None).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    убрать_задачи_вызова(&pool, scenario).await;
}

/// Адаптер разбирает ответ сервера NLI и не принимает ответ, где
/// вероятностей меньше, чем гипотез: разбор сдвинулся бы на факт.
#[tokio::test]
async fn адаптер_разбирает_ответ_сервера_nli() {
    use axum::routing::post;
    let сервер = Router::new().route("/nli", post(|axum::Json(b): axum::Json<Value>| async move {
        let n = b["hypotheses"].as_array().map(|a| a.len()).unwrap_or(0);
        // На «сломайся» — на одну вероятность меньше
        let n = if b["premise"] == "сломайся" { n - 1 } else { n };
        axum::Json(json!({ "probs": vec![[0.1, 0.8, 0.1]; n] }))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, сервер).await.unwrap() });

    let nli = HttpNli::new(format!("http://{addr}")).unwrap();
    assert!(nli.available());
    let ответ = nli.classify("Горит дом.", &["Горит дом.".into(), "Есть пламя.".into()]).await.unwrap();
    assert_eq!(ответ.len(), 2);
    assert!((ответ[0].entailment - 0.8).abs() < 1e-6);
    assert!(nli.classify("сломайся", &["а".into(), "б".into()]).await.is_err());
}

/// Сквозь тренажёр на настоящих моделях: утверждения готовит языковая
/// модель (`ML_BASE_URL`, по умолчанию 127.0.0.1:8090), описание сверяет
/// сервер `nli/server.py` (`NLI_BASE_URL`, по умолчанию 127.0.0.1:8093).
/// Нужны оба запущенных сервера, поэтому по умолчанию пропускается:
/// `cargo test --test fact_statements -- --ignored --nocapture`
#[tokio::test]
#[ignore = "нужны запущенные сервер модели и сервер NLI"]
async fn настоящие_модели_сверяют_описание_сквозь_тренажёр() {
    let Some((app, pool, _)) = app().await else { return };
    let Some(scenario) = копия_вызова(&pool).await else { return };

    let llm = arm112::ml::http::HttpLlm::new(
        std::env::var("ML_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8090".into()),
        "local".into(),
    ).unwrap();
    let started = std::time::Instant::now();
    let итог = fact_statements::run(&pool, &llm, scenario).await.unwrap();
    println!("утверждения за {:.1} с: {итог}", started.elapsed().as_secs_f64());
    let s = scenario_edit::load(&pool, scenario).await.unwrap();
    println!("легенда: {:?}", s.legend_statements);
    for f in &s.hidden {
        println!("{}: {:?}", f.key, f.statement);
    }

    let (card, analysis) = вызов_с_описанием(&app, &pool, scenario, "victims",
        "Горит баня на участке, есть пострадавший. Дом газифицирован.").await;
    let nli = HttpNli::new(
        std::env::var("NLI_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8093".into()),
    ).unwrap();
    let started = std::time::Instant::now();
    let status = text_analysis::run_nli(&pool, &nli, analysis).await.unwrap();
    let v = text_analysis::view(&pool, text_analysis::Kind::Card112, card, None).await.unwrap().unwrap();
    println!("сверка за {:.1} с, статус {status}", started.elapsed().as_secs_f64());
    println!("{}", serde_json::to_string_pretty(&v).unwrap());
    assert_eq!(status, "ready", "{:?}", v.note);
    assert!(v.findings.iter().any(|f| f.finding.kind == "contradiction"),
        "баня вместо крыши и пострадавший — противоречия");

    убрать_задачи_вызова(&pool, scenario).await;
}
