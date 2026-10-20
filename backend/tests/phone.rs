//! IP-телефон диспетчера ДДС: справочник, вызовы, журнал, запись.
//!
//! Обратная связь экспертов 18.10 — «виртуальный IP-телефон для
//! специалиста ДДС». Модель и синтез здесь — заглушки: собеседник
//! звука не даёт, поэтому запись разговора собирается из голоса
//! диспетчера, а реплики собеседника остаются текстом.
//!
//! Запуск: `cargo test --test phone`

mod common;
use arm112::{api, config::{Config, MlProvider}, domain::phone_audio, ml::MlSuite, state::AppState};
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
    config.ml_provider = MlProvider::Mock;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(MlProvider::Mock).ok()?;
    Some((api::routes().with_state(AppState::new(pool.clone(), config, ml)), pool))
}

/// Запрос с любым телом; ответ — статус, тип и сырые байты.
async fn сырой(app: &Router, m: Method, path: &str, token: Option<&str>, ct: &str, body: Vec<u8>)
    -> (StatusCode, String, Vec<u8>)
{
    let mut req = Request::builder().method(m).uri(path);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if !body.is_empty() {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    let resp = app.clone().oneshot(req.body(Body::from(body)).unwrap()).await.unwrap();
    let status = resp.status();
    let kind = resp.headers().get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, kind, bytes)
}

async fn call(app: &Router, m: Method, path: &str, token: Option<&str>, body: Option<Value>)
    -> (StatusCode, Value)
{
    let тело = body.map(|b| b.to_string().into_bytes()).unwrap_or_default();
    let (status, _, bytes) = сырой(app, m, path, token, "application/json", тело).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn токен(app: &Router, login: &str) -> String {
    let (_, b) = call(app, Method::POST, "/api/auth/login", None,
        Some(json!({ "login": login, "password": "arm112" }))).await;
    b["token"].as_str().expect("вход не удался").to_string()
}

/// Своя служба вне справочника, свой диспетчер и выданная ему карточка.
struct Стенд {
    service: String,
    login: String,
    user_id: Uuid,
    card: Uuid,
}

async fn стенд(pool: &PgPool) -> Стенд {
    let метка = Uuid::new_v4().simple().to_string();
    let service = format!("проверка-тел-{}", &метка[..10]);
    let login = format!("телефонист-{}", &метка[..10]);

    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&service).execute(pool).await.unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Звонарёв Т.Т.', 'student', $2
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).bind(&service).fetch_one(pool).await.unwrap();

    let номер = format!("Т{}", &метка[..10]);
    let card: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description)
         VALUES ($1, now(), '{101}', 'проверочная карточка') RETURNING id",
    )
    .bind(&номер).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
        .bind(card).bind(&service).execute(pool).await.unwrap();
    common::выдать(pool, card, user_id).await;
    Стенд { service, login, user_id, card }
}

/// Бригада № 2 уже направлена — есть кому звонить.
async fn направить(pool: &PgPool, с: &Стенд) {
    sqlx::query(
        "INSERT INTO brigade_assignments (card_id, service_id, brigade_no, user_id)
         VALUES ($1, $2, 2, $3)",
    )
    .bind(с.card).bind(&с.service).bind(с.user_id).execute(pool).await.unwrap();
}

async fn прибрать(pool: &PgPool, с: &Стенд) {
    sqlx::query("DELETE FROM cards WHERE id = $1").bind(с.card).execute(pool).await.ok();
    for sql in [
        "DELETE FROM group_members WHERE user_id = $1",
        "UPDATE audit_log SET user_id = NULL WHERE user_id = $1",
        "DELETE FROM users WHERE id = $1",
    ] {
        sqlx::query(sql).bind(с.user_id).execute(pool).await.ok();
    }
    sqlx::query("DELETE FROM services WHERE id = $1").bind(&с.service).execute(pool).await.ok();
}

/// Полсекунды «речи» диспетчера — как присылает браузер: WAV 16 кГц.
fn голос() -> Vec<u8> {
    let звук: Vec<f32> = (0..8000).map(|i| (i as f32 * 0.07).sin() * 0.3).collect();
    phone_audio::encode(&звук, 16_000)
}

async fn набрать(app: &Router, с: &Стенд, т: &str, peer: &str, direction: &str) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/cards/{}/phone/calls", с.card), Some(т),
        Some(json!({ "peer": peer, "direction": direction }))).await
}

#[tokio::test]
async fn параметры_телефонии_и_свой_номер() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    let т = токен(&app, &с.login).await;

    let (status, конф) = call(&app, Method::GET, "/api/phone/config", Some(&т), None).await;
    assert_eq!(status, StatusCode::OK, "{конф}");
    // Служба вне справочника — в общей сотне номеров
    assert_eq!(конф["extension"], "2980");
    assert!(конф["ringSec"].as_i64().unwrap() >= 5);
    assert!(конф["answerSec"].is_i64());
    // На заглушке голоса нет — разговор текстом
    assert_eq!(конф["asr"], false);
    assert_eq!(конф["tts"], false);

    // Диспетчер службы из справочника — в её сотне: 2000 + 20 × место
    let место: i32 = sqlx::query_scalar("SELECT position FROM service_catalog WHERE service_id = 'mchs'")
        .fetch_one(&pool).await.unwrap();
    let логин: Option<String> = sqlx::query_scalar(
        "SELECT login FROM users WHERE role = 'student' AND duty = 'dds' AND service_id = 'mchs'
          ORDER BY login LIMIT 1")
        .fetch_optional(&pool).await.unwrap();
    if let Some(логин) = логин {
        let т2 = токен(&app, &логин).await;
        let (_, к) = call(&app, Method::GET, "/api/phone/config", Some(&т2), None).await;
        assert_eq!(к["extension"], (2000 + 20 * место).to_string());
    }

    // Преподавателю свой номер не нужен — он не на линии
    let тп = токен(&app, "teacher1").await;
    let (_, к) = call(&app, Method::GET, "/api/phone/config", Some(&тп), None).await;
    assert!(к["extension"].is_null());

    // Параметры — у администратора, в пределах
    let та = токен(&app, "admin").await;
    for (ключ, плохо) in [("ui.phone", 2.0), ("phone.ring_sec", 1.0), ("phone.answer_sec", 60.0)] {
        let (status, _) = call(&app, Method::PATCH, &format!("/api/settings/{ключ}"), Some(&та),
            Some(json!({ "value": плохо }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{ключ} = {плохо}");
    }
    let (status, _) = call(&app, Method::PATCH, "/api/settings/phone.ring_sec", Some(&тп),
        Some(json!({ "value": 30 }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "преподаватель параметры не меняет");

    прибрать(&pool, &с).await;
}

#[tokio::test]
async fn справочник_бригада_появляется_после_направления() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    let т = токен(&app, &с.login).await;
    let путь = format!("/api/cards/{}/phone", с.card);

    let (status, вид) = call(&app, Method::GET, &путь, Some(&т), None).await;
    assert_eq!(status, StatusCode::OK, "{вид}");
    let книга = вид["phonebook"].as_array().unwrap();
    let бригада = книга.iter().find(|з| з["peer"] == "brigade_head").unwrap();
    assert_eq!(бригада["available"], false);
    assert_eq!(бригада["note"], "бригада не назначена");
    let начальник = книга.iter().find(|з| з["peer"] == "response_head").unwrap();
    assert_eq!(начальник["available"], true);
    assert_eq!(начальник["number"], "2999");

    // Звонить некому — вызова нет
    let (status, _) = набрать(&app, &с, &т, "brigade", "out").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    направить(&pool, &с).await;
    let (_, вид) = call(&app, Method::GET, &путь, Some(&т), None).await;
    let бригада = вид["phonebook"].as_array().unwrap().iter()
        .find(|з| з["peer"] == "brigade_head").unwrap().clone();
    assert_eq!(бригада["available"], true);
    assert_eq!(бригада["title"], "Начальник аварийной бригады № 2");
    assert_eq!(бригада["number"], "2981");

    // Служба закончила работу — старшему звонить незачем
    sqlx::query("INSERT INTO response_entries (card_id, service_id, status, operator)
                  VALUES ($1, $2, 'Работы завершены', 'проверка')")
        .bind(с.card).bind(&с.service).execute(&pool).await.unwrap();
    let (status, ответ) = набрать(&app, &с, &т, "brigade", "out").await;
    assert_eq!(status, StatusCode::CONFLICT, "{ответ}");

    прибрать(&pool, &с).await;
}

#[tokio::test]
async fn исходящий_разговор_попадает_в_журнал_и_запись() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    направить(&pool, &с).await;
    let т = токен(&app, &с.login).await;

    let (status, вызов) = набрать(&app, &с, &т, "brigade", "out").await;
    assert_eq!(status, StatusCode::CREATED, "{вызов}");
    assert_eq!(вызов["outcome"], "ringing");
    assert_eq!(вызов["number"], "2981");
    assert_eq!(вызов["direction"], "out");
    let id = вызов["id"].as_str().unwrap().to_string();

    // Реплика до ответа — разговора ещё нет
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/lines"), Some(&т),
        Some(json!({ "who": "dispatcher", "text": "Алло" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, вызов) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/answer"),
        Some(&т), None).await;
    assert_eq!(status, StatusCode::OK, "{вызов}");
    assert_eq!(вызов["outcome"], "talking");

    let (status, реплика) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/lines"), Some(&т),
        Some(json!({ "who": "dispatcher", "text": "  Диспетчер ДДС, доложите обстановку  " }))).await;
    assert_eq!(status, StatusCode::CREATED, "{реплика}");
    let строка = реплика["id"].as_i64().unwrap();
    let звук = format!("/api/phone/calls/{id}/lines/{строка}/audio");
    let (status, _, _) = сырой(&app, Method::PUT, &звук, Some(&т), "audio/wav", голос()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Не WAV — не сохраняем
    let (status, _, _) = сырой(&app, Method::PUT, &звук, Some(&т), "audio/wav", b"mp3?".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, ответ) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/lines"), Some(&т),
        Some(json!({ "who": "peer", "text": "Бригада 2 прибыла на место" }))).await;
    assert_eq!(status, StatusCode::CREATED);
    // Голос сохраняется только у диспетчера
    let чужой = format!("/api/phone/calls/{id}/lines/{}/audio", ответ["id"]);
    let (status, _, _) = сырой(&app, Method::PUT, &чужой, Some(&т), "audio/wav", голос()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, вызов) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/end"),
        Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(вызов["outcome"], "ended");
    assert!(вызов["durationSec"].as_i64().unwrap() >= 0);
    // Повторное «положить трубку» ничего не меняет
    let (_, снова) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/end"),
        Some(&т), Some(json!({ "outcome": "declined" }))).await;
    assert_eq!(снова["outcome"], "ended");

    // Журнал: реплики по порядку, текст без лишних пробелов, голос есть
    let (_, вид) = call(&app, Method::GET, &format!("/api/cards/{}/phone", с.card), Some(&т), None).await;
    let в = &вид["calls"][0];
    assert_eq!(в["id"], id.as_str());
    assert_eq!(в["title"], "Начальник аварийной бригады № 2");
    assert_eq!(в["lines"][0]["text"], "Диспетчер ДДС, доложите обстановку");
    assert_eq!(в["lines"][0]["hasAudio"], true);
    assert_eq!(в["lines"][1]["who"], "peer");
    assert_eq!(в["lines"][1]["hasAudio"], false);
    assert_eq!(в["recording"], true);

    // Запись — WAV 16 кГц: на заглушке синтеза в ней только голос
    // диспетчера, полсекунды
    let (status, тип, wav) = сырой(&app, Method::GET, &format!("/api/phone/calls/{id}/recording"),
        Some(&т), "", vec![]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(тип, "audio/wav");
    let pcm = phone_audio::parse(&wav).expect("запись — не WAV");
    assert_eq!(pcm.rate, 16_000);
    assert_eq!(pcm.samples.len(), 8000);

    // Преподаватель слушает ту же запись и видит, чей это вызов
    let тп = токен(&app, "teacher1").await;
    let (status, _, _) = сырой(&app, Method::GET, &format!("/api/phone/calls/{id}/recording"),
        Some(&тп), "", vec![]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, журнал) = call(&app, Method::GET,
        &format!("/api/cards/{}/phone?serviceId={}", с.card, с.service), Some(&тп), None).await;
    assert_eq!(status, StatusCode::OK, "{журнал}");
    assert_eq!(журнал["calls"][0]["dispatcher"], "Звонарёв Т.Т.");
    assert!(журнал["phonebook"].as_array().unwrap().is_empty());

    // Администратор выключил запись: голос не хранится, разговор —
    // только текстом, и записывать нечего
    sqlx::query("UPDATE settings SET value = '0' WHERE key = 'phone.record'")
        .execute(&pool).await.unwrap();
    let (_, второй) = набрать(&app, &с, &т, "brigade", "out").await;
    let id2 = второй["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/phone/calls/{id2}/answer"), Some(&т), None).await;
    let (_, р) = call(&app, Method::POST, &format!("/api/phone/calls/{id2}/lines"), Some(&т),
        Some(json!({ "who": "dispatcher", "text": "Как обстановка?" }))).await;
    let (status, _, _) = сырой(&app, Method::PUT,
        &format!("/api/phone/calls/{id2}/lines/{}/audio", р["id"]), Some(&т), "audio/wav", голос()).await;
    sqlx::query("UPDATE settings SET value = '1' WHERE key = 'phone.record'")
        .execute(&pool).await.unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, вид) = call(&app, Method::GET, &format!("/api/cards/{}/phone", с.card), Some(&т), None).await;
    assert_eq!(вид["calls"][0]["lines"][0]["hasAudio"], false);
    assert_eq!(вид["calls"][0]["recording"], false);
    let (status, _, _) = сырой(&app, Method::GET, &format!("/api/phone/calls/{id2}/recording"),
        Some(&т), "", vec![]).await;
    assert_eq!(status, StatusCode::CONFLICT);

    прибрать(&pool, &с).await;
}

#[tokio::test]
async fn входящий_пропущен_или_сброшен() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    направить(&pool, &с).await;
    let т = токен(&app, &с.login).await;

    // Не взяли трубку
    let (status, вызов) = набрать(&app, &с, &т, "brigade", "in").await;
    assert_eq!(status, StatusCode::CREATED, "{вызов}");
    let id = вызов["id"].as_str().unwrap().to_string();
    let (_, вызов) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/end"), Some(&т), None).await;
    assert_eq!(вызов["outcome"], "missed");
    assert!(вызов["durationSec"].is_null(), "разговора не было");
    // Пропущенный уже не снять, и реплик в нём нет
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/answer"), Some(&т), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/lines"), Some(&т),
        Some(json!({ "who": "peer", "text": "доклад" }))).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Сбросили
    let (_, вызов) = набрать(&app, &с, &т, "brigade", "in").await;
    let id = вызов["id"].as_str().unwrap().to_string();
    let (_, вызов) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/end"), Some(&т),
        Some(json!({ "outcome": "declined" }))).await;
    assert_eq!(вызов["outcome"], "declined");

    // Начальник сам не звонит, незнакомых собеседников нет
    let (status, _) = набрать(&app, &с, &т, "response_head", "in").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = набрать(&app, &с, &т, "соседи", "out").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = набрать(&app, &с, &т, "chief", "туда").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Брошенный разговор закрывается следующим вызовом: линия одна
    let (_, первый) = набрать(&app, &с, &т, "chief", "out").await;
    let id1 = первый["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/phone/calls/{id1}/answer"), Some(&т), None).await;
    набрать(&app, &с, &т, "brigade", "out").await;
    let (_, вид) = call(&app, Method::GET, &format!("/api/cards/{}/phone", с.card), Some(&т), None).await;
    let первый = вид["calls"].as_array().unwrap().iter().find(|з| з["id"] == id1.as_str()).unwrap();
    assert_eq!(первый["outcome"], "ended");
    assert_eq!(вид["calls"][0]["outcome"], "ringing", "новый — сверху и звонит");

    прибрать(&pool, &с).await;
}

#[tokio::test]
async fn чужой_вызов_не_виден_и_звонит_только_диспетчер() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    let чужой = стенд(&pool).await;
    направить(&pool, &с).await;
    let т = токен(&app, &с.login).await;
    let тч = токен(&app, &чужой.login).await;

    let (_, вызов) = набрать(&app, &с, &т, "brigade", "out").await;
    let id = вызов["id"].as_str().unwrap().to_string();

    // Диспетчер другой службы: ни снять, ни дописать, ни послушать
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/answer"), Some(&тч), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/end"), Some(&тч), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = сырой(&app, Method::GET, &format!("/api/phone/calls/{id}/recording"),
        Some(&тч), "", vec![]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // И карточку чужой службы он не видит вовсе
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{}/phone", с.card), Some(&тч), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Преподаватель вызов видит, но сам не звонит
    let тп = токен(&app, "teacher1").await;
    let (status, _) = набрать(&app, &с, &тп, "brigade", "out").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Оператору 112 телефон ДДС не положен
    let т112 = токен(&app, "student1").await;
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{}/phone", с.card), Some(&т112), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Без входа — ничего
    let (status, _) = call(&app, Method::GET, "/api/phone/config", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    прибрать(&pool, &с).await;
    прибрать(&pool, &чужой).await;
}

// ─── Справочник от преподавателя (рекомендации финалистам, п. 8) ─────

async fn сохранить_справочник(app: &Router, т: &str, service: &str, entries: Value) -> (StatusCode, Value) {
    call(app, Method::PUT, &format!("/api/phonebook?serviceId={service}"), Some(т),
        Some(json!({ "entries": entries }))).await
}

#[tokio::test]
async fn справочник_правит_преподаватель_не_более_десяти_номеров() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    let т = токен(&app, &с.login).await;
    let тп = токен(&app, "teacher1").await;
    let путь = format!("/api/phonebook?serviceId={}", с.service);

    // Служба без своих записей — справочник по умолчанию: четыре роли
    let (status, книга) = call(&app, Method::GET, &путь, Some(&тп), None).await;
    assert_eq!(status, StatusCode::OK, "{книга}");
    assert_eq!(книга["isDefault"], true);
    assert_eq!(книга["max"], 10);
    let роли: Vec<&str> = книга["entries"].as_array().unwrap().iter()
        .map(|e| e["role"].as_str().unwrap()).collect();
    assert_eq!(роли, ["brigade_head", "brigade_specialist", "response_head", "org_head"]);
    assert_eq!(книга["entries"][2]["number"], "2999", "начальник отдела — прежний номер начальника");
    assert_eq!(книга["roles"].as_array().unwrap().len(), 5);

    // Курсант читает свой, чужой — нет, и не правит
    let (status, своя) = call(&app, Method::GET, "/api/phonebook", Some(&т), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(своя["serviceId"], с.service.as_str());
    let (status, _) = call(&app, Method::GET, "/api/phonebook?serviceId=mchs", Some(&т), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = сохранить_справочник(&app, &т, &с.service, json!([])).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Больше десяти — нельзя
    let много: Vec<Value> = (0..11).map(|i| json!({
        "role": "brigade_specialist", "number": format!("31{i:02}"), "title": format!("Специалист {i}") })).collect();
    let (status, ответ) = сохранить_справочник(&app, &тп, &с.service, json!(много)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{ответ}");
    // Повтор номера, чужая роль, не цифры, пустая подпись — тоже
    for плохо in [
        json!([{ "role": "org_head", "number": "3001", "title": "Руководитель" },
               { "role": "response_head", "number": "3001", "title": "Начальник отдела" }]),
        json!([{ "role": "neighbour", "number": "3001", "title": "Сосед" }]),
        json!([{ "role": "org_head", "number": "30-01", "title": "Руководитель" }]),
        json!([{ "role": "org_head", "number": "3001", "title": "  " }]),
    ] {
        let (status, _) = сохранить_справочник(&app, &тп, &с.service, плохо).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // Преподаватель сохранил десять с заявителем — сохранилось как есть
    let mut десять: Vec<Value> = (0..9).map(|i| json!({
        "role": if i == 0 { "org_head" } else { "brigade_specialist" },
        "number": format!("31{i:02}"), "title": format!("Абонент {i}") })).collect();
    десять.push(json!({ "role": "applicant", "number": "", "title": "Заявитель" }));
    let (status, книга) = сохранить_справочник(&app, &тп, &с.service, json!(десять)).await;
    assert_eq!(status, StatusCode::OK, "{книга}");
    assert_eq!(книга["isDefault"], false);
    assert_eq!(книга["entries"].as_array().unwrap().len(), 10);
    assert_eq!(книга["entries"][0]["title"], "Абонент 0");
    let (_, своя) = call(&app, Method::GET, "/api/phonebook", Some(&т), None).await;
    assert_eq!(своя["entries"][9]["role"], "applicant");

    // Сброс — снова по умолчанию; службы нет — 404
    let (status, книга) = call(&app, Method::DELETE, &путь, Some(&тп), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(книга["isDefault"], true);
    let (status, _) = call(&app, Method::GET, "/api/phonebook?serviceId=нет-такой", Some(&тп), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    прибрать(&pool, &с).await;
}

#[tokio::test]
async fn звонок_по_ролям_справочника_и_заявителю_по_карточке() {
    let Some((app, pool)) = app().await else { return };
    let с = стенд(&pool).await;
    направить(&pool, &с).await;
    let т = токен(&app, &с.login).await;
    let тп = токен(&app, "teacher1").await;
    let вид = format!("/api/cards/{}/phone", с.card);

    // Заявителя в справочнике нет — его нет и в телефоне, звонок не идёт
    let (_, книга) = call(&app, Method::GET, &вид, Some(&т), None).await;
    assert!(книга["phonebook"].as_array().unwrap().iter().all(|з| з["peer"] != "applicant"));
    let (status, _) = набрать(&app, &с, &т, "applicant", "out").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Преподаватель составил перечень: руководитель, бригада, заявитель
    let (status, _) = сохранить_справочник(&app, &тп, &с.service, json!([
        { "role": "org_head", "number": "3010", "title": "Руководитель организации Петров" },
        { "role": "brigade_head", "number": "3011", "title": "Начальник аварийной бригады" },
        { "role": "brigade_specialist", "number": "3012", "title": "Специалист аварийной бригады" },
        { "role": "applicant", "number": "", "title": "Заявитель" },
    ])).await;
    assert_eq!(status, StatusCode::OK);

    // В карточке номера заявителя нет — звонить некуда
    let (_, книга) = call(&app, Method::GET, &вид, Some(&т), None).await;
    let заявитель = книга["phonebook"].as_array().unwrap().iter().find(|з| з["peer"] == "applicant").unwrap().clone();
    assert_eq!(заявитель["available"], false);
    assert_eq!(заявитель["note"], "в карточке нет номера заявителя");
    let (status, _) = набрать(&app, &с, &т, "applicant", "out").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Появился АОН — диспетчер набирает номер из карточки
    sqlx::query("UPDATE cards SET phone_aon = '+7 (916) 000-00-01', applicant_name = 'Смирнова Анна Ильинична',
                        description = 'Собака застряла в вентиляционной шахте подвала' WHERE id = $1")
        .bind(с.card).execute(&pool).await.unwrap();
    let (_, книга) = call(&app, Method::GET, &вид, Some(&т), None).await;
    let заявитель = книга["phonebook"].as_array().unwrap().iter().find(|з| з["peer"] == "applicant").unwrap().clone();
    assert_eq!(заявитель["available"], true);
    assert_eq!(заявитель["number"], "+7 (916) 000-00-01");
    assert_eq!(заявитель["title"], "Заявитель — Смирнова Анна Ильинична");
    assert_eq!(заявитель["voice"], "applicant_f");
    // Заявитель сам не звонит
    let (status, _) = набрать(&app, &с, &т, "applicant", "in").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, вызов) = набрать(&app, &с, &т, "applicant", "out").await;
    assert_eq!(status, StatusCode::CREATED, "{вызов}");
    assert_eq!(вызов["number"], "+7 (916) 000-00-01");
    let id = вызов["id"].as_str().unwrap().to_string();
    // До ответа заявитель молчит
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/reply"), Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    call(&app, Method::POST, &format!("/api/phone/calls/{id}/answer"), Some(&т), None).await;
    // Снял трубку: повторяет то, что сообщил в 112, — описание карточки
    let (status, ответ) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/reply"), Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    let сказал = ответ["text"].as_str().unwrap();
    assert!(сказал.contains("звонила в 112"), "{сказал}");
    assert!(сказал.contains("вентиляционной шахте"), "{сказал}");
    call(&app, Method::POST, &format!("/api/phone/calls/{id}/lines"), Some(&т),
        Some(json!({ "who": "dispatcher", "text": "Бригада выехала, ждите" }))).await;
    let (_, ответ) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/reply"), Some(&т),
        Some(json!({ "text": "Бригада выехала, ждите" }))).await;
    assert_eq!(ответ["finished"], false);
    let (_, вид_ж) = call(&app, Method::GET, &вид, Some(&т), None).await;
    assert_eq!(вид_ж["calls"][0]["lines"].as_array().unwrap().len(), 3, "реплики заявителя — в журнале");
    assert_eq!(вид_ж["calls"][0]["title"], "Заявитель — Смирнова Анна Ильинична");
    assert_eq!(вид_ж["calls"][0]["voice"], "applicant_f");
    call(&app, Method::POST, &format!("/api/phone/calls/{id}/end"), Some(&т), None).await;

    // Руководитель организации: слушает, уточняет, принимает и кладёт трубку
    let (status, вызов) = набрать(&app, &с, &т, "org_head", "out").await;
    assert_eq!(status, StatusCode::CREATED, "{вызов}");
    assert_eq!(вызов["number"], "3010");
    assert_eq!(вызов["title"], "Руководитель организации Петров");
    let id = вызов["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/phone/calls/{id}/answer"), Some(&т), None).await;
    let (_, ответ) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/reply"), Some(&т), Some(json!({}))).await;
    assert!(ответ["text"].as_str().unwrap().starts_with("Руководитель организации Петров слушает"), "{ответ}");
    for (i, фраза) in ["Докладываю: бригада на месте", "Сил достаточно, помощь не нужна"].iter().enumerate() {
        call(&app, Method::POST, &format!("/api/phone/calls/{id}/lines"), Some(&т),
            Some(json!({ "who": "dispatcher", "text": фраза }))).await;
        let (_, ответ) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/reply"), Some(&т),
            Some(json!({ "text": фраза }))).await;
        assert_eq!(ответ["finished"], i == 1, "{ответ}");
    }

    // Специалист бригады — по этапам бригады, своими словами
    let (status, вызов) = набрать(&app, &с, &т, "brigade_specialist", "out").await;
    assert_eq!(status, StatusCode::CREATED, "{вызов}");
    assert_eq!(вызов["title"], "Специалист аварийной бригады № 2");
    assert_eq!(вызов["voice"], "brigade");
    let id = вызов["id"].as_str().unwrap().to_string();
    call(&app, Method::POST, &format!("/api/phone/calls/{id}/answer"), Some(&т), None).await;
    let (status, _) = call(&app, Method::POST, &format!("/api/phone/calls/{id}/reply"), Some(&т), Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "специалист говорит через бригаду");
    let (status, бригада) = call(&app, Method::POST, &format!("/api/cards/{}/brigade/call", с.card), Some(&т),
        Some(json!({ "serviceId": с.service, "speaker": "specialist" }))).await;
    assert_eq!(status, StatusCode::OK, "{бригада}");
    assert!(бригада["text"].as_str().unwrap().starts_with("Специалист бригады 2:"), "{бригада}");

    // Прежние имена собеседников принимаются: brigade — начальник бригады
    let (status, вызов) = набрать(&app, &с, &т, "brigade", "in").await;
    assert_eq!(status, StatusCode::CREATED, "{вызов}");
    assert_eq!(вызов["peer"], "brigade_head");
    assert_eq!(вызов["number"], "3011");

    call(&app, Method::DELETE, &format!("/api/phonebook?serviceId={}", с.service), Some(&тп), None).await;
    прибрать(&pool, &с).await;
}
