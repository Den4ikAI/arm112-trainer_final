//! Канал класса.
//!
//! Главное требование: карточка, сохранённая курсантом-оператором,
//! в течение секунды долетает до курсанта-диспетчера профильной ДДС.
//!
//! Поднимаем настоящий сервер на свободном порту и подключаемся
//! настоящим клиентом: прогон роутера через `oneshot` апгрейд до
//! WebSocket не выполняет, и проверять было бы нечего.
//!
//! Запуск: `cargo test --test ws_api`

use arm112::{api, config::Config, ml::MlSuite, state::AppState, ws};
use axum::{
    body::Body,
    http::{header, Method, Request},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

/// Стенд: тот же `AppState` (а значит, та же шина событий) доступен
/// и по HTTP, и по настоящему сокету.
///
/// HTTP гоняем через `oneshot` по роутеру, а не через порт: внутри
/// `#[tokio::test]` рантайм однопоточный, и блокирующий TCP-клиент
/// занял бы единственный поток — сервер на нём же перестал бы
/// отвечать. Для WebSocket порт всё-таки нужен: апгрейд соединения
/// через `oneshot` не выполняется.
struct Server {
    app: Router,
    addr: String,
}

/// Куда ходят проверки.
///
/// Отдельная база, если она задана: иначе прогон копит мусор в той же,
/// на которой стоит стенд — в учётных записях накапливаются сотни
/// курсантов, а в переписке сценариев реплики «проверка 1789977379959».
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn start() -> Option<Server> {
    let config = Config::from_env().ok()?;
    let pool = sqlx::PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let state = AppState::new(pool, config, ml);

    let app = api::routes()
        .merge(ws::routes::routes())
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
    let addr = listener.local_addr().ok()?.to_string();
    let served = app.clone();
    tokio::spawn(async move {
        axum::serve(listener, served).await.ok();
    });

    Some(Server { app, addr })
}

impl Server {
    async fn request(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>)
        -> Value
    {
        let mut req = Request::builder().method(method).uri(path);
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

        let resp = self.app.clone().oneshot(req).await.unwrap();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> Value {
        self.request(Method::POST, path, token, Some(body)).await
    }
    async fn patch(&self, path: &str, token: &str, body: Value) -> Value {
        self.request(Method::PATCH, path, Some(token), Some(body)).await
    }
    async fn get(&self, path: &str, token: &str) -> Value {
        self.request(Method::GET, path, Some(token), None).await
    }
    async fn token(&self, login: &str) -> String {
        self.post("/api/auth/login", None, json!({ "login": login, "password": "arm112" }))
            .await["token"]
            .as_str()
            .expect("вход не удался")
            .to_string()
    }
}

type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

async fn connect(server: &Server, token: &str) -> Socket {
    let url = format!("ws://{}/ws?token={token}", server.addr);
    let (socket, _) = tokio_tungstenite::connect_async(url).await.expect("канал не открылся");
    socket
}

async fn subscribe(socket: &mut Socket, topics: &[&str]) -> Value {
    socket
        .send(Message::text(json!({ "type": "subscribe", "topics": topics }).to_string()))
        .await
        .unwrap();
    next(socket).await.expect("нет ответа на подписку")
}

/// Следующее сообщение. `None`, если за секунду ничего не пришло —
/// ровно тот порог, который стоит в критерии приёмки.
async fn next(socket: &mut Socket) -> Option<Value> {
    let msg = tokio::time::timeout(Duration::from_secs(1), socket.next()).await.ok()??;
    match msg.ok()? {
        Message::Text(text) => serde_json::from_str(&text).ok(),
        _ => None,
    }
}

/// Оператор заполняет карточку пожара в частном доме и сохраняет её.
async fn dispatch_fire(server: &Server, operator: &str) -> Value {
    let scenarios = server.get("/api/scenarios", operator).await;
    let scenario = scenarios
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["ticket"] == 3)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let s = server.post("/api/sessions", Some(operator),
        json!({ "scenarioId": scenario, "mode": "card112" })).await;
    let session = s["session"]["id"].as_str().unwrap().to_string();

    server.patch(&format!("/api/sessions/{session}/card"), operator, json!({
        "typeIds": ["101"],
        "answers": { "where": ["Дом"], "fire-sign": ["Открытое пламя / Дым"],
                     "house-kind": ["Дом частный"], "people-threat": ["Нет"],
                     "inhouse": ["Крыша"], "gasified": ["Нет"] }
    })).await;

    server.post(&format!("/api/sessions/{session}/save"), Some(operator), json!({})).await
}

/// Критерий приёмки: сохранение карточки одним пользователем
/// в течение секунды долетает до другого, подписанного на свою службу.
#[tokio::test]
async fn карточка_долетает_до_диспетчера_профильной_ддс() {
    let Some(server) = start().await else { return };
    let mchs = server.token("student3").await;
    let operator = server.token("student1").await;

    let mut socket = connect(&server, &mchs).await;
    let ack = subscribe(&mut socket, &["service:mchs"]).await;
    assert_eq!(ack["type"], "subscribed");

    let saved = dispatch_fire(&server, &operator).await;
    let number = saved["card"]["number"].as_str().unwrap().to_string();

    let event = next(&mut socket).await.expect("карточка не долетела за секунду");
    assert_eq!(event["type"], "card.dispatched");
    assert_eq!(event["number"], number.as_str());
    assert_eq!(event["outcomeType"], "пожар: частный дом");
    assert!(event["services"].as_array().unwrap().iter().any(|s| s == "mchs"));
}

/// Пожар в частном доме не профилен для Мослифта — в его канал
/// он попасть не должен. Тот же фильтр, что и в журнале.
#[tokio::test]
async fn чужая_карточка_в_канал_не_попадает() {
    let Some(server) = start().await else { return };
    let lift = server.token("student2").await;
    let operator = server.token("student1").await;

    let mut socket = connect(&server, &lift).await;
    subscribe(&mut socket, &["service:moslift"]).await;

    dispatch_fire(&server, &operator).await;

    assert!(next(&mut socket).await.is_none(), "пожар пришёл в Мослифт");
}

/// Без проверки прав канал стал бы дырой в обход всего
/// разграничения: подписался на чужой топик и читаешь чужую ленту.
#[tokio::test]
async fn на_чужую_службу_подписаться_нельзя() {
    let Some(server) = start().await else { return };
    let lift = server.token("student2").await;

    let mut socket = connect(&server, &lift).await;
    socket
        .send(Message::text(
            json!({ "type": "subscribe", "topics": ["service:mchs"] }).to_string(),
        ))
        .await
        .unwrap();

    let first = next(&mut socket).await.unwrap();
    assert_eq!(first["type"], "error");
    assert_eq!(first["error"], "forbidden_topic");

    let ack = next(&mut socket).await.unwrap();
    assert_eq!(ack["type"], "subscribed");
    assert!(ack["topics"].as_array().unwrap().is_empty());
}

/// Преподаватель ведёт занятие и видит все службы.
#[tokio::test]
async fn преподаватель_подписывается_на_любую_службу() {
    let Some(server) = start().await else { return };
    let teacher = server.token("teacher1").await;
    let operator = server.token("student1").await;

    let mut socket = connect(&server, &teacher).await;
    let ack = subscribe(&mut socket, &["service:mchs", "service:codd"]).await;
    assert_eq!(ack["topics"].as_array().unwrap().len(), 2);

    dispatch_fire(&server, &operator).await;
    let event = next(&mut socket).await.expect("карточка не долетела");
    assert_eq!(event["type"], "card.dispatched");
}

/// Диспетчер проставил статус — это видят все, кто следит
/// за карточкой.
#[tokio::test]
async fn статус_реагирования_уходит_в_канал() {
    let Some(server) = start().await else { return };
    let mchs = server.token("student3").await;
    let operator = server.token("student1").await;

    let mut socket = connect(&server, &mchs).await;
    subscribe(&mut socket, &["service:mchs"]).await;

    let saved = dispatch_fire(&server, &operator).await;
    let card_id = saved["card"]["id"].as_str().unwrap().to_string();
    next(&mut socket).await.expect("карточка не долетела");

    server.post(&format!("/api/cards/{card_id}/responses"), Some(&mchs),
        json!({ "serviceId": "mchs", "status": "Принята", "orderNo": "23" })).await;

    let event = next(&mut socket).await.expect("статус не долетел");
    assert_eq!(event["type"], "response.recorded");
    assert_eq!(event["serviceId"], "mchs");
    assert_eq!(event["status"], "Принята");
    assert_eq!(event["operator"], "Сидоров С.С.");
}

/// Протухший или поддельный токен до канала не доводит.
#[tokio::test]
async fn с_плохим_токеном_канал_не_открывается() {
    let Some(server) = start().await else { return };

    let url = format!("ws://{}/ws?token=не-токен", server.addr);
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
}

/// Преподаватель следит за своей группой: он видит, кто взял вызов
/// и с каким результатом закончил.
#[tokio::test]
async fn преподаватель_видит_ход_занятия_в_своей_группе() {
    let Some(server) = start().await else { return };
    let teacher = server.token("teacher1").await;
    let operator = server.token("student1").await;

    let groups = server.get("/api/groups", &teacher).await;
    let Some(group) = groups.as_array().and_then(|g| g.first()) else { return };
    let topic = format!("group:{}", group["id"].as_str().unwrap());

    let mut socket = connect(&server, &teacher).await;
    subscribe(&mut socket, &[topic.as_str()]).await;

    let saved = dispatch_fire(&server, &operator).await;

    let started = next(&mut socket).await.expect("начало занятия не долетело");
    assert_eq!(started["type"], "session.started");

    let finished = next(&mut socket).await.expect("завершение занятия не долетело");
    assert_eq!(finished["type"], "session.finished");
    assert_eq!(finished["score"], saved["grade"]["score"]);
}
