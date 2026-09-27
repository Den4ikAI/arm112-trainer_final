//! Разбор занятия: сравнение трёх слоёв.
//!
//! Главное, что здесь проверяется, — различие между «выяснил»
//! и «угадал». Без него оценка сводится к сверке двух текстов,
//! которую умеет кто угодно.
//!
//! Запуск: `cargo test --test grade_api`

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
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn token(app: &Router) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": "student1", "password": "arm112" }))).await;
    b["token"].as_str().unwrap().to_string()
}

/// Эталон билета 3: горит крыша частного дома в Королёве.
fn correct_card() -> Value {
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

struct Session {
    token: String,
    id: String,
    facts: Vec<(String, String)>,
}

async fn start(app: &Router, pool: &sqlx::PgPool) -> Option<Session> {
    let tok = token(app).await;
    let scenario: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM scenarios WHERE ticket = 3 AND call_no = 1")
        .fetch_optional(pool).await.ok()??;

    let (status, body) = call(app, Method::POST, "/api/sessions", Some(&tok),
        Some(json!({ "scenarioId": scenario.to_string(), "mode": "card112" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let facts = body["scenario"]["questions"].as_array().unwrap().iter()
        .map(|q| (q["key"].as_str().unwrap().to_string(),
                  q["factId"].as_str().unwrap().to_string()))
        .collect();

    Some(Session { token: tok, id: body["session"]["id"].as_str().unwrap().to_string(), facts })
}

async fn ask_all(app: &Router, s: &Session) {
    for (_, id) in &s.facts {
        call(app, Method::POST, &format!("/api/sessions/{}/ask", s.id), Some(&s.token),
            Some(json!({ "factId": id }))).await;
    }
}

async fn fill_and_save(app: &Router, s: &Session, card: Value) -> Value {
    call(app, Method::PATCH, &format!("/api/sessions/{}/card", s.id), Some(&s.token),
        Some(card)).await;
    let (status, body) = call(app, Method::POST, &format!("/api/sessions/{}/save", s.id),
        Some(&s.token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["grade"].clone()
}

fn kinds<'a>(grade: &'a Value, kind: &str) -> Vec<&'a str> {
    grade["findings"].as_array().unwrap().iter()
        .filter(|f| f["kind"] == kind)
        .map(|f| f["field"].as_str().unwrap())
        .collect()
}

/// Курсант всё выяснил и всё заполнил верно — ни одного замечания.
#[tokio::test]
async fn выяснил_и_заполнил_верно_сто_баллов() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    ask_all(&app, &s).await;
    let grade = fill_and_save(&app, &s, correct_card()).await;

    assert!(kinds(&grade, "guessed").is_empty(),
        "выясненное засчитано как угаданное: {:?}", kinds(&grade, "guessed"));
    assert!(kinds(&grade, "wrong").is_empty());
    assert!(kinds(&grade, "missing").is_empty());
    assert!(kinds(&grade, "extra").is_empty());
    assert_eq!(grade["score"], 100);
}

/// То же содержимое карточки, но ни одного вопроса заявителю.
/// Значения верные — и всё равно это не работа оператора.
#[tokio::test]
async fn то_же_самое_без_вопросов_это_угадал() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    let grade = fill_and_save(&app, &s, correct_card()).await;

    let guessed = kinds(&grade, "guessed");
    assert!(guessed.contains(&"answers.gasified"),
        "газификацию нельзя было знать без вопроса: {guessed:?}");
    assert!(guessed.contains(&"address.settlement"),
        "город заявитель сам не называл: {guessed:?}");
    assert!(grade["score"].as_i64().unwrap() < 100,
        "угаданная карточка не может стоить столько же, сколько выясненная");

    // Но ошибкой это не считается: значения-то верные
    assert!(kinds(&grade, "wrong").is_empty());
}

/// Поля, которые заявитель назвал сам, угаданными не считаются.
#[tokio::test]
async fn названное_в_первой_фразе_не_считается_угаданным() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    let grade = fill_and_save(&app, &s, correct_card()).await;
    let guessed = kinds(&grade, "guessed");

    // Улицу и дом заявитель назвал в первой же фразе, вопроса не требовалось
    assert!(!guessed.contains(&"address.street"), "{guessed:?}");
    assert!(!guessed.contains(&"address.house"), "{guessed:?}");
}

/// Ошибка в признаке видна не баллом, а последствием: поехала лишняя
/// служба. Это и есть «последствие вместо оценки».
#[tokio::test]
async fn неверный_признак_приводит_к_лишней_службе() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    ask_all(&app, &s).await;
    let mut card = correct_card();
    card["answers"]["gasified"] = json!(["Да"]);
    let grade = fill_and_save(&app, &s, card).await;

    assert!(kinds(&grade, "wrong").contains(&"answers.gasified"));
    let extra = grade["services"]["extra"].as_array().unwrap();
    assert!(extra.iter().any(|s| s == "mosgaz"),
        "из-за неверной газификации должен был приехать газ: {extra:?}");

    // У находки есть человеческое последствие, а не только вес
    let f = grade["findings"].as_array().unwrap().iter()
        .find(|f| f["field"] == "service.mosgaz").unwrap();
    assert!(f["consequence"].as_str().unwrap().contains("непрофильный"));
}

/// «Королёв» и «Королев» — один город. Ввод человека нормализуем,
/// значения самого классификатора — нет.
#[tokio::test]
async fn ё_в_адресе_не_ошибка() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    ask_all(&app, &s).await;
    let mut card = correct_card();
    card["address"]["settlement"] = json!("королев");
    let grade = fill_and_save(&app, &s, card).await;

    assert!(!kinds(&grade, "wrong").contains(&"address.settlement"));
}

/// Заявитель сказал, что пострадавших нет. Отметка при этом — ошибка:
/// скорая поедет впустую.
#[tokio::test]
async fn отметка_о_пострадавших_которых_нет_это_ошибка() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    ask_all(&app, &s).await;
    let mut card = correct_card();
    card["victims"] = json!(2);
    let grade = fill_and_save(&app, &s, card).await;

    let f = grade["findings"].as_array().unwrap().iter()
        .find(|f| f["field"] == "victims").expect("пострадавшие не проверены");
    assert_eq!(f["kind"], "wrong");
    assert!(f["consequence"].as_str().unwrap().contains("скорая"));
}

/// Незаполненный адрес — не «неверно», а «не заполнено»:
/// последствия разные, и вес штрафа тоже.
#[tokio::test]
async fn незаполненные_поля_отличаются_от_неверных() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    ask_all(&app, &s).await;
    let grade = fill_and_save(&app, &s, json!({
        "typeIds": ["101"],
        "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"] }
    })).await;

    let missing = kinds(&grade, "missing");
    assert!(missing.contains(&"address.street"), "{missing:?}");
    assert!(missing.contains(&"answers.gasified"), "{missing:?}");
    // Службы не подобрались: без признаков дома позиция не определяется
    assert!(!grade["services"]["missed"].as_array().unwrap().is_empty());
    assert!(grade["score"].as_i64().unwrap() < 60);
}

/// Разбор доступен только после сохранения и считается один раз.
#[tokio::test]
async fn разбор_появляется_после_сохранения_и_не_меняется() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };
    let url = format!("/api/sessions/{}/grade", s.id);

    let (before, _) = call(&app, Method::GET, &url, Some(&s.token), None).await;
    assert_eq!(before, StatusCode::NOT_FOUND, "эталон виден до сохранения");

    ask_all(&app, &s).await;
    let saved = fill_and_save(&app, &s, correct_card()).await;

    let (status, stored) = call(&app, Method::GET, &url, Some(&s.token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["score"], saved["score"]);
    assert_eq!(stored["findings"].as_array().unwrap().len(),
               saved["findings"].as_array().unwrap().len());
}

/// Замечание со стенда: первая фраза «Горит крыша частного дома»,
/// а «Дом частный» и «Крыша» в разборе стояли как угаданные.
/// Газификацию же заявитель сам не называл — она угадана по-прежнему.
#[tokio::test]
async fn тип_дома_и_крыша_из_первой_фразы_не_угаданы() {
    let Some((app, pool)) = app().await else { return };
    let Some(s) = start(&app, &pool).await else { return };

    let grade = fill_and_save(&app, &s, correct_card()).await;
    let guessed = kinds(&grade, "guessed");

    assert!(!guessed.contains(&"answers.house-kind"), "{guessed:?}");
    assert!(!guessed.contains(&"answers.inhouse"), "{guessed:?}");
    assert!(guessed.contains(&"answers.gasified"), "{guessed:?}");
}

/// То же на всех утверждённых сценариях: карточка заполнена по
/// эталону без единого вопроса, и ни одно «угадал» не относится
/// к значению, которое заявитель назвал в первой фразе.
#[tokio::test]
async fn ни_в_одном_сценарии_сказанное_первой_фразой_не_угадано() {
    let Some((app, pool)) = app().await else { return };
    let tok = token(&app).await;

    let сценарии: Vec<(uuid::Uuid, String, Value)> = sqlx::query_as(
        "SELECT id, legend, expected FROM scenarios WHERE status = 'approved'")
        .fetch_all(&pool).await.unwrap();

    for (id, легенда, эталон) in сценарии {
        let (status, body) = call(&app, Method::POST, "/api/sessions", Some(&tok),
            Some(json!({ "scenarioId": id.to_string(), "mode": "card112" }))).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let сессия = body["session"]["id"].as_str().unwrap().to_string();

        let mut карточка = json!({
            "address": эталон.get("address").cloned().unwrap_or(json!({})),
            "answers": эталон.get("answers").cloned().unwrap_or(json!({})),
            "typeIds": эталон.get("typeIds").cloned().unwrap_or(json!([])),
        });
        if let Some(v) = эталон.get("victims").and_then(Value::as_i64) {
            карточка["victims"] = json!(v);
        }
        let s = Session { token: tok.clone(), id: сессия, facts: Vec::new() };
        let grade = fill_and_save(&app, &s, карточка).await;

        for f in grade["findings"].as_array().unwrap() {
            if f["kind"] != "guessed" {
                continue;
            }
            let значение = f["expected"].as_str().unwrap_or_default();
            assert!(!arm112::domain::grade::stated_in(&легенда, значение),
                "сценарий {id}: «{значение}» ({}) сказано в первой фразе «{легенда}», \
                 а в разборе — угадано", f["field"]);
        }
    }
}
