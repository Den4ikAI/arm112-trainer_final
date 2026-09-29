//! Доклад диспетчера ДДС вышестоящему начальнику.
//!
//! Прогон идёт на заглушке модели: доложенное ищут правила, вопросы
//! начальника — готовые. Где важен ответ модели, её заменяет подставная
//! с заранее заданным JSON — проверяется код вокруг модели: проверка
//! цитат, выбор вопроса, хранение, права.
//!
//! Запуск: `cargo test --test chief`

mod common;
use arm112::{
    api,
    config::{Config, MlProvider},
    domain::chief,
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

async fn app() -> Option<(Router, PgPool)> {
    let mut config = Config::from_env().ok()?;
    // Разговор с начальником на заглушке: правила вместо модели, чтобы
    // прогон не зависел от того, поднята ли она
    config.ml_provider = MlProvider::Mock;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(MlProvider::Mock).ok()?;
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

struct Стенд {
    service: String,
    login: String,
    user_id: Uuid,
    card: Uuid,
}

/// Своя служба, свой курсант и карточка пожара в Королёве с соседней
/// службой 103 в списке оповещения.
async fn стенд(pool: &PgPool, пострадавших: i32) -> Стенд {
    let метка = Uuid::new_v4().simple().to_string();
    let service = format!("проверка-нач-{}", &метка[..10]);
    let login = format!("докладчик-{}", &метка[..10]);

    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&service).execute(pool).await.unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Докладчиков Д.Д.', 'student', $2
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).bind(&service).fetch_one(pool).await.unwrap();

    let номер = format!("Н{}", &метка[..10]);
    let card: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description, outcome_type, victims, address)
         VALUES ($1, now(), '{101}', 'Горит крыша частного дома', 'Пожар в частном доме', $2,
                 '{\"settlement\": \"Королёв\", \"street\": \"Станционная\", \"house\": \"28\"}')
         RETURNING id",
    )
    .bind(&номер).bind(пострадавших).fetch_one(pool).await.unwrap();
    for sv in [service.as_str(), "smp"] {
        sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
            .bind(card).bind(sv).execute(pool).await.unwrap();
    }
    // Диспетчеру карточку выдаёт преподаватель
    common::выдать(pool, card, user_id).await;
    Стенд { service, login, user_id, card }
}

async fn прибрать(pool: &PgPool, с: &Стенд) {
    sqlx::query("DELETE FROM cards WHERE id = $1").bind(с.card).execute(pool).await.ok();
    for sql in [
        "UPDATE audit_log SET user_id = NULL WHERE user_id = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(sql).bind(с.user_id).execute(pool).await.ok();
    }
    sqlx::query("DELETE FROM services WHERE id = $1").bind(&с.service).execute(pool).await.ok();
}

async fn сказать(app: &Router, с: &Стенд, т: &str, текст: &str) -> Value {
    let (status, ответ) = call(app, Method::POST, &format!("/api/cards/{}/chief/say", с.card),
        Some(т), Some(json!({ "text": текст }))).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    ответ
}

fn последняя(вид: &Value) -> &Value {
    вид["lines"].as_array().unwrap().last().unwrap()
}

fn сведение<'a>(вид: &'a Value, ключ: &str) -> &'a Value {
    вид["review"]["items"].as_array().unwrap().iter()
        .find(|i| i["key"] == ключ).unwrap_or_else(|| panic!("нет сведения {ключ}: {вид}"))
}

/// Доклад на заглушке: начальник спрашивает о недоложенном по порядку —
/// где, пострадавшие, службы — и, когда всё доложено, говорит «Принял».
#[tokio::test]
async fn доклад_начальнику_до_принял() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 0).await;
    let т = токен(&app, &с.login).await;

    let (status, вид) = call(&app, Method::POST, &format!("/api/cards/{}/chief/start", с.card),
        Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{вид}");
    assert_eq!(последняя(&вид)["text"], chief::GREETING);
    assert!(вид["review"].is_null());

    let вид = сказать(&app, &с, &т, "Докладываю: у нас пожар, горит частный дом.").await;
    assert_eq!(последняя(&вид)["topic"], "where");
    assert_eq!(последняя(&вид)["text"], chief::template_question("where"));

    let вид = сказать(&app, &с, &т, "Королёв, улица Станционная, дом 28.").await;
    assert_eq!(последняя(&вид)["topic"], "victims");

    let вид = сказать(&app, &с, &т, "Пострадавших нет.").await;
    assert_eq!(последняя(&вид)["topic"], "services");

    let вид = сказать(&app, &с, &т, "Оповещена скорая.").await;
    assert_eq!(последняя(&вид)["text"], chief::ACCEPTED);
    assert!(вид["finishedAt"].is_string(), "после «Принял» звонок окончен");
    assert_eq!(вид["review"]["reported"], 4, "{}", вид["review"]);
    assert_eq!(вид["review"]["total"], 4);
    assert_eq!(вид["review"]["mode"], "rules");
    assert_eq!(сведение(&вид, "where")["quote"], "Королёв, улица Станционная, дом 28.");
    assert_eq!(сведение(&вид, "where")["expected"], "Королёв, Станционная, 28");

    // Приветствие, четыре реплики диспетчера и четыре начальника
    assert_eq!(вид["lines"].as_array().unwrap().len(), 9);

    // После «Принял» говорить некому
    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/chief/say", с.card),
        Some(&т), Some(json!({ "text": "Алло?" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    прибрать(&pool, &с).await;
}

/// Начальник не ведёт допрос: три вопроса — и «Принял», даже если
/// доложено не всё. Недоложенное остаётся в разборе.
#[tokio::test]
async fn начальник_задаёт_не_больше_трёх_вопросов() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 0).await;
    let т = токен(&app, &с.login).await;
    call(&app, Method::POST, &format!("/api/cards/{}/chief/start", с.card), Some(&т), Some(json!({}))).await;

    let mut вид = Value::Null;
    for _ in 0..4 {
        вид = сказать(&app, &с, &т, "Алло, это диспетчер.").await;
    }
    let вопросов = вид["lines"].as_array().unwrap().iter()
        .filter(|l| l["who"] == "chief" && l["topic"].is_string()).count();
    assert_eq!(вопросов, chief::MAX_QUESTIONS);
    assert_eq!(последняя(&вид)["text"], chief::ACCEPTED);
    assert_eq!(вид["review"]["reported"], 0);
    assert_eq!(сведение(&вид, "what")["status"], "missing");

    прибрать(&pool, &с).await;
}

/// «Пострадавших нет» при двух пострадавших в карточке — доложено
/// неверно, и в разборе дословная фраза диспетчера. Доклад можно
/// закончить самому, не дожидаясь вопросов.
#[tokio::test]
async fn неверно_доложенное_с_цитатой() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 2).await;
    let т = токен(&app, &с.login).await;
    call(&app, Method::POST, &format!("/api/cards/{}/chief/start", с.card), Some(&т), Some(json!({}))).await;
    сказать(&app, &с, &т, "Пожар в Королёве на Станционной, 28. Пострадавших нет.").await;

    let (status, вид) = call(&app, Method::POST, &format!("/api/cards/{}/chief/finish", с.card),
        Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{вид}");
    assert_eq!(последняя(&вид)["text"], chief::ACCEPTED);
    let пострадавшие = сведение(&вид, "victims");
    assert_eq!(пострадавшие["status"], "wrong");
    assert_eq!(пострадавшие["expected"], "2");
    assert_eq!(пострадавшие["quote"], "Пострадавших нет.");

    прибрать(&pool, &с).await;
}

/// Докладывает только диспетчер своей службы. Преподаватель звонок
/// видит, но не ведёт; курсант чужой службы не звонит вовсе.
#[tokio::test]
async fn звонит_только_диспетчер_своей_службы() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 0).await;
    let т = токен(&app, &с.login).await;
    let преподаватель = токен(&app, "teacher1").await;
    let чужой = токен(&app, "student1").await;
    let начать = format!("/api/cards/{}/chief/start", с.card);

    let (status, _) = call(&app, Method::POST, &начать, Some(&преподаватель), Some(json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(&app, Method::POST, &начать, Some(&чужой), Some(json!({}))).await;
    assert!(!status.is_success(), "курсант чужой службы позвонил: {status}");

    // Без начатого звонка говорить некому
    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/chief/say", с.card),
        Some(&т), Some(json!({ "text": "Докладываю" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    call(&app, Method::POST, &начать, Some(&т), Some(json!({}))).await;
    сказать(&app, &с, &т, "Пожар.").await;

    let (status, вид) = call(&app, Method::GET,
        &format!("/api/cards/{}/chief?serviceId={}", с.card, с.service), Some(&преподаватель), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(вид["student"], "Докладчиков Д.Д.");
    assert_eq!(вид["lines"].as_array().unwrap().len(), 3);

    // Пустую реплику не принимаем
    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/chief/say", с.card),
        Some(&т), Some(json!({ "text": "   " }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    прибрать(&pool, &с).await;
}

/// Разбор доклада виден в разборе попытки ДДС — там же, где балл правил
/// и экспертная оценка.
#[tokio::test]
async fn доклад_в_разборе_попытки() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool, 0).await;
    let т = токен(&app, &с.login).await;

    let (status, ответ) = call(&app, Method::POST, &format!("/api/cards/{}/responses", с.card),
        Some(&т), Some(json!({ "serviceId": с.service, "status": "Принята",
                               "comment": "Принято в работу" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");

    call(&app, Method::POST, &format!("/api/cards/{}/chief/start", с.card), Some(&т), Some(json!({}))).await;
    сказать(&app, &с, &т, "Пожар в частном доме, Королёв, Станционная 28.").await;
    call(&app, Method::POST, &format!("/api/cards/{}/chief/finish", с.card), Some(&т), Some(json!({}))).await;

    let преподаватель = токен(&app, "teacher1").await;
    let (status, разбор) = call(&app, Method::GET, &format!("/api/cards/{}/review", с.card),
        Some(&преподаватель), None).await;
    assert_eq!(status, StatusCode::OK, "{разбор}");
    let попытка = разбор["attempts"].as_array().unwrap().iter()
        .find(|a| a["kind"] == "dds" && a["serviceId"] == с.service.as_str())
        .unwrap_or_else(|| panic!("нет попытки ДДС: {разбор}"));
    assert_eq!(попытка["chief"]["review"]["total"], 4);
    assert_eq!(попытка["chief"]["review"]["reported"], 2);

    прибрать(&pool, &с).await;
}

/// С моделью «Принял» звучит сразу, а разбор модель уточняет в фоне.
/// Модель недоступна — разбор всё равно доходит до конца: остаются
/// ключевые слова с пометкой, а не вечное «уточняется».
#[tokio::test]
async fn разбор_моделью_после_принял_идёт_в_фоне() {
    let Some((_, pool)) = app().await else { return };
    let mut config = Config::from_env().unwrap();
    config.ml_provider = MlProvider::Http;
    // Порт 9 никто не слушает: модель «недоступна» сразу, без ожидания
    let ml = arm112::ml::http::suite("http://127.0.0.1:9".into(), "local".into()).unwrap();
    let app = api::routes().with_state(AppState::new(pool.clone(), config, ml));

    let с = стенд(&pool, 0).await;
    let т = токен(&app, &с.login).await;
    call(&app, Method::POST, &format!("/api/cards/{}/chief/start", с.card), Some(&т), Some(json!({}))).await;
    сказать(&app, &с, &т, "Пожар в частном доме.").await;
    let (status, вид) = call(&app, Method::POST, &format!("/api/cards/{}/chief/finish", с.card),
        Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{вид}");
    assert_eq!(последняя(&вид)["text"], chief::ACCEPTED);
    // Недоступная модель отказывает мгновенно, и фоновый разбор может
    // записаться раньше, чем уйдёт ответ: тогда пометки «уточняется»
    // уже нет, но видно, что модель пробовали
    assert!(вид["review"]["pending"] == true
            || вид["review"]["note"].as_str().is_some_and(|n| n.contains("модель не ответила")),
            "разбор должен уйти модели: {}", вид["review"]);

    let mut итог = Value::Null;
    for _ in 0..50 {
        let (_, в) = call(&app, Method::GET, &format!("/api/cards/{}/chief", с.card), Some(&т), None).await;
        if в["review"]["pending"] == false {
            итог = в;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(!итог.is_null(), "фоновый разбор так и не записался");
    assert!(итог["review"]["note"].as_str().unwrap().contains("модель не ответила"), "{итог}");
    assert_eq!(сведение(&итог, "what")["status"], "reported");

    прибрать(&pool, &с).await;
}

// ─── С моделью ───────────────────────────────────────────────────────

struct Подставная {
    ответ: Value,
}

#[async_trait]
impl Llm for Подставная {
    async fn generate_scenario(&self, _: Value) -> anyhow::Result<Value> { unreachable!() }
    async fn refine_scenario(&self, _: Value) -> anyhow::Result<Value> { unreachable!() }
    async fn group_insights(&self, _: Value) -> anyhow::Result<String> { unreachable!() }
    async fn chat_json(&self, _: Value, _: u32, schema: Option<Value>) -> anyhow::Result<Value> {
        assert!(schema.is_some(), "ответ начальника обязан ограничиваться схемой");
        Ok(self.ответ.clone())
    }
}

/// Модель отметила доложенным то, чего диспетчер не говорил, — отметка
/// отброшена. Вопрос модели с подсказкой («Станционная?») заменён
/// готовым: начальник спрашивает, а не подсказывает.
#[tokio::test]
async fn выдуманная_цитата_и_подсказка_модели_отброшены() {
    let Some((_, pool)) = app().await else { return };
    let с = стенд(&pool, 0).await;
    let факты = chief::facts(&pool, с.card, &с.service).await.unwrap();
    let сказано = vec!["Горит частный дом.".to_string()];

    let модель = Подставная { ответ: json!({
        "items": [
            { "key": "what", "status": "доложено", "quote": "Горит частный дом." },
            { "key": "where", "status": "доложено", "quote": "улица Станционная, 28" },
            { "key": "victims", "status": "не доложено", "quote": "" },
            { "key": "services", "status": "не доложено", "quote": "" },
        ],
        "question": { "key": "where", "text": "Это на Станционной?" },
    }) };

    let (разбор, реплика, тема, _) = chief::evaluate(Some(&модель), &факты, &сказано, &[], 0).await;
    assert_eq!(разбор.mode, "model");
    assert_eq!(разбор.dropped, 1, "адрес диспетчер не называл");
    assert_eq!(разбор.items.iter().find(|i| i.key == "where").unwrap().status, "missing");
    assert_eq!(тема.as_deref(), Some("where"));
    assert_eq!(реплика, chief::template_question("where"));

    // Чистый вопрос модели — её же формулировкой
    let модель = Подставная { ответ: json!({
        "items": [{ "key": "what", "status": "доложено", "quote": "Горит частный дом." }],
        "question": { "key": "where", "text": "А где это случилось?" },
    }) };
    let (_, реплика, _, _) = chief::evaluate(Some(&модель), &факты, &сказано, &[], 0).await;
    assert_eq!(реплика, "А где это случилось?");

    прибрать(&pool, &с).await;
}

/// Живой разговор с настоящей моделью: диспетчер докладывает не всё,
/// начальник переспрашивает. Печатает разговор и время каждой реплики —
/// из этого прогона пример в backend/docs/замеры/.
#[tokio::test]
#[ignore = "нужен запущенный сервер модели"]
async fn настоящая_модель_ведёт_доклад() {
    let Some((_, pool)) = app().await else { return };
    let с = стенд(&pool, 1).await;
    // Бригада направлена и уже доложила о выезде
    let назначение: Uuid = sqlx::query_scalar(
        "INSERT INTO brigade_assignments (card_id, service_id, brigade_no) VALUES ($1, $2, 2) RETURNING id")
        .bind(с.card).bind(&с.service).fetch_one(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO brigade_reports (assignment_id, stage, text, due_at, delivered_at, channel)
         VALUES ($1, 'Начало реагирования', 'Старший бригады 2: выехали на вызов.', now(), now(), 'incoming')")
        .bind(назначение).execute(&pool).await.unwrap();

    let факты = chief::facts(&pool, с.card, &с.service).await.unwrap();
    let ml = MlSuite::build(MlProvider::Http).unwrap();

    // Что диспетчер отвечает на вопрос о каждом сведении
    let ответ = |тема: &str| match тема {
        "what" => "Пожар, горит крыша частного дома.",
        "where" => "Королёв, Станционная улица, дом 28.",
        "victims" => "Есть один пострадавший.",
        "brigade" => "Направил вторую бригаду.",
        "stage" => "Бригада выехала, едут на место.",
        "services" => "Скорую тоже оповестили.",
        _ => "Больше сведений нет.",
    };

    let mut сказано = vec!["Товарищ начальник, докладываю: в Королёве пожар в частном доме.".to_string()];
    let mut спрошено: Vec<String> = Vec::new();
    println!("Начальник: {}", chief::GREETING);
    println!("Диспетчер: {}", сказано[0]);
    // Модель в самом разговоре — CHIEF_LIVE=1; по умолчанию, как на
    // стенде, вопросы по ключевым словам, а модель разбирает в конце
    let live = std::env::var("CHIEF_LIVE").is_ok_and(|v| v == "1");
    loop {
        let t = std::time::Instant::now();
        let (разбор, реплика, тема, ms) = chief::evaluate(
            live.then(|| ml.llm.as_ref()), &факты, &сказано, &спрошено, спрошено.len()).await;
        println!("Начальник ({:.1} с, модель {:?} мс): {реплика}", t.elapsed().as_secs_f64(), ms);
        match тема {
            None => {
                let разбор = if live {
                    разбор
                } else {
                    let (r, ms) = chief::review_with_model(ml.llm.as_ref(), &факты, &сказано).await;
                    println!("Разбор моделью после «Принял»: {:.1} с", ms as f64 / 1000.0);
                    r
                };
                println!("{}", serde_json::to_string_pretty(&разбор).unwrap());
                break;
            }
            Some(тема) => {
                let a = ответ(&тема).to_string();
                println!("Диспетчер: {a}");
                сказано.push(a);
                спрошено.push(тема);
            }
        }
    }
    прибрать(&pool, &с).await;
}
