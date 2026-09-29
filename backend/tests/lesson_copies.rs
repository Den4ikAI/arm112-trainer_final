//! Занятие ДДС выдаёт копию учебного происшествия.
//!
//! Статусы, бригада, доклады, звонки и разбор привязаны к карточке.
//! Пока занятие выдавало само происшествие, одно происшествие в двух
//! занятиях или группах делило всё это: «Принята» в одной группе
//! оказывалась принятой и в другой, бригаду второй группе было не
//! назначить (одна на карточку и службу), в ленте появлялись две строки
//! одной карточки (`docs/UX-ПРОХОД-2.md`, проблема A). Теперь каждая
//! выдача — своя карточка с `source_card_id` на исходник.
//!
//! Запуск: `cargo test --test lesson_copies`

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

/// Своя служба и утверждённое учебное происшествие преподавателя в неё.
struct Стенд {
    служба: String,
    исходник: Uuid,
    курсанты: Vec<Uuid>,
    группы: Vec<Uuid>,
}

async fn стенд(pool: &PgPool) -> Стенд {
    let метка = Uuid::new_v4().simple().to_string();
    let служба = format!("копии-сл-{}", &метка[..10]);
    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&служба).execute(pool).await.unwrap();
    let исходник: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description, origin, approved_at, title)
         VALUES ($1, now(), '{101}', 'учебное происшествие', 'teacher', now(), 'Пожар')
         RETURNING id")
        .bind(format!("К{}", &метка[..10])).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
        .bind(исходник).bind(&служба).execute(pool).await.unwrap();
    Стенд { служба, исходник, курсанты: vec![], группы: vec![] }
}

/// Группа с одним диспетчером службы стенда. Возвращает группу и логин.
async fn группа(pool: &PgPool, с: &mut Стенд) -> (Uuid, String) {
    let метка = Uuid::new_v4().simple().to_string();
    let login = format!("копии-{}", &метка[..12]);
    let user: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id, duty)
         SELECT $1, password_hash, 'Копиев К.К.', 'student', $2, 'dds'
           FROM users WHERE login = 'student1' RETURNING id")
        .bind(&login).bind(&с.служба).fetch_one(pool).await.unwrap();
    let group: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id)
         SELECT $1, id FROM users WHERE login = 'teacher1' RETURNING id")
        .bind(format!("копии-{}", &метка[..12])).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(group).bind(user).execute(pool).await.unwrap();
    с.курсанты.push(user);
    с.группы.push(group);
    (group, login)
}

/// Занятие ДДС с исходником, начатое. Возвращает занятие и выданную копию.
async fn выдать(app: &Router, pool: &PgPool, учитель: &str, group: Uuid, исходник: Uuid)
    -> (String, Uuid)
{
    let (status, занятие) = call(app, Method::POST, "/api/lessons", Some(учитель),
        Some(json!({ "groupId": group, "title": "Статусы", "mode": "dds",
                     "cardIds": [исходник] }))).await;
    assert_eq!(status, StatusCode::CREATED, "{занятие}");
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, начато) = call(app, Method::POST, &format!("/api/lessons/{id}/start"),
        Some(учитель), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");
    let копия: Uuid = sqlx::query_scalar(
        "SELECT card_id FROM lesson_cards WHERE lesson_id = $1::text::uuid")
        .bind(&id).fetch_one(pool).await.unwrap();
    (id, копия)
}

async fn прибрать(app: &Router, pool: &PgPool, учитель: &str, с: &Стенд, занятия: &[String]) {
    for id in занятия {
        call(app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(учитель), None).await;
    }
    sqlx::query("DELETE FROM cards WHERE source_card_id = $1 OR id = $1")
        .bind(с.исходник).execute(pool).await.ok();
    for g in &с.группы {
        sqlx::query("DELETE FROM training_groups WHERE id = $1").bind(g).execute(pool).await.ok();
    }
    for u in &с.курсанты {
        for sql in [
            "DELETE FROM group_members WHERE user_id = $1",
            "UPDATE audit_log SET user_id = NULL WHERE user_id = $1",
            "DELETE FROM users WHERE id = $1",
        ] {
            sqlx::query(sql).bind(u).execute(pool).await.ok();
        }
    }
    sqlx::query("DELETE FROM services WHERE id = $1").bind(&с.служба).execute(pool).await.ok();
}

/// Одно происшествие в занятиях двух групп: у каждой своя карточка —
/// свой номер, свои статусы и своя бригада; в ленте по строке.
#[tokio::test]
async fn одно_происшествие_в_двух_группах_статусы_и_бригада_раздельны() {
    let Some((app, pool)) = app().await else { return };
    let учитель = токен(&app, "teacher1").await;
    let mut с = стенд(&pool).await;
    let (группа_а, логин_а) = группа(&pool, &mut с).await;
    let (группа_б, логин_б) = группа(&pool, &mut с).await;
    let (занятие_а, копия_а) = выдать(&app, &pool, &учитель, группа_а, с.исходник).await;
    let (занятие_б, копия_б) = выдать(&app, &pool, &учитель, группа_б, с.исходник).await;

    assert_ne!(копия_а, с.исходник, "занятие выдаёт копию, а не исходник");
    assert_ne!(копия_а, копия_б);
    let копии: Vec<(Uuid, String, Option<Uuid>, String, bool, bool, Vec<String>)> = sqlx::query_as(
        "SELECT c.id, c.number, c.source_card_id, c.origin,
                c.approved_at IS NOT NULL, c.saved_at IS NOT NULL,
                ARRAY(SELECT service_id FROM card_services WHERE card_id = c.id)
           FROM cards c WHERE c.id = ANY($1)")
        .bind(vec![копия_а, копия_б]).fetch_all(&pool).await.unwrap();
    assert_eq!(копии.len(), 2);
    let номер_исходника: String = sqlx::query_scalar("SELECT number FROM cards WHERE id = $1")
        .bind(с.исходник).fetch_one(&pool).await.unwrap();
    for (_, номер, источник, origin, утверждена, сохранена, службы) in &копии {
        assert_eq!(*источник, Some(с.исходник));
        assert_eq!(origin, "teacher");
        assert!(*утверждена && *сохранена);
        assert_eq!(службы, &vec![с.служба.clone()]);
        assert_ne!(номер, &номер_исходника, "у копии свой номер");
    }
    assert_ne!(копии[0].1, копии[1].1);

    let а = токен(&app, &логин_а).await;
    let б = токен(&app, &логин_б).await;

    // Лента: у каждого одна строка — своя копия
    for (т, своя) in [(&а, копия_а), (&б, копия_б)] {
        let (status, лента) = call(&app, Method::GET, "/api/cards?size=50", Some(т), None).await;
        assert_eq!(status, StatusCode::OK, "{лента}");
        let ids: Vec<&str> = лента["items"].as_array().unwrap().iter()
            .map(|x| x["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec![своя.to_string().as_str()], "{лента}");
    }
    // Чужая копия по ссылке не открывается
    let (status, _) = call(&app, Method::GET, &format!("/api/cards/{копия_б}"), Some(&а), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Группа А приняла и направила бригаду
    let (status, ответ) = call(&app, Method::POST, &format!("/api/cards/{копия_а}/responses"),
        Some(&а), Some(json!({ "serviceId": с.служба, "status": "Принята",
                                "comment": "Принято в работу" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");
    let (status, вид) = call(&app, Method::POST, &format!("/api/cards/{копия_а}/brigade"),
        Some(&а), Some(json!({ "serviceId": с.служба, "brigadeNo": 2 }))).await;
    assert_eq!(status, StatusCode::CREATED, "{вид}");

    // У группы Б по её копии ничего не произошло
    let записей_б: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM response_entries WHERE card_id = $1 AND status <> 'Добавлена'")
        .bind(копия_б).fetch_one(&pool).await.unwrap();
    assert_eq!(записей_б, 0);
    let (status, бригада_б) = call(&app, Method::GET,
        &format!("/api/cards/{копия_б}/brigade?serviceId={}", с.служба), Some(&б), None).await;
    assert_eq!(status, StatusCode::OK, "{бригада_б}");
    assert!(бригада_б["assignment"].is_null(), "{бригада_б}");
    assert_eq!(бригада_б["canAssign"], false, "группа Б ещё не приняла");

    // Группа Б принимает и направляет ту же бригаду — по своей карточке
    call(&app, Method::POST, &format!("/api/cards/{копия_б}/responses"),
        Some(&б), Some(json!({ "serviceId": с.служба, "status": "Принята",
                                "comment": "Принято" }))).await;
    let (status, вид) = call(&app, Method::POST, &format!("/api/cards/{копия_б}/brigade"),
        Some(&б), Some(json!({ "serviceId": с.служба, "brigadeNo": 2 }))).await;
    assert_eq!(status, StatusCode::CREATED, "{вид}");

    // Работа по исходнику не записана вовсе
    let по_исходнику: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM response_entries WHERE card_id = $1)
              + (SELECT count(*) FROM brigade_assignments WHERE card_id = $1)")
        .bind(с.исходник).fetch_one(&pool).await.unwrap();
    assert_eq!(по_исходнику, 0);

    прибрать(&app, &pool, &учитель, &с, &[занятие_а, занятие_б]).await;
}

/// Копия — не учебный материал: в списке преподавателя её нет, открыть,
/// править, утвердить и удалить её через материал нельзя. Исходник
/// считает свои выдачи.
#[tokio::test]
async fn копия_не_видна_в_учебном_материале() {
    let Some((app, pool)) = app().await else { return };
    let учитель = токен(&app, "teacher1").await;
    let mut с = стенд(&pool).await;
    let (группа_а, _) = группа(&pool, &mut с).await;
    let (занятие, копия) = выдать(&app, &pool, &учитель, группа_а, с.исходник).await;

    let (status, материал) = call(&app, Method::GET, "/api/dds-incidents", Some(&учитель), None).await;
    assert_eq!(status, StatusCode::OK, "{материал}");
    let материал = материал.as_array().unwrap();
    assert!(!материал.iter().any(|x| x["id"] == копия.to_string()), "копия в материале");
    let исходник = материал.iter().find(|x| x["id"] == с.исходник.to_string())
        .expect("исходника нет в материале");
    assert_eq!(исходник["lessons"], 1, "{исходник}");

    for (m, путь) in [
        (Method::GET, format!("/api/dds-incidents/{копия}")),
        (Method::POST, format!("/api/dds-incidents/{копия}/approve")),
        (Method::DELETE, format!("/api/dds-incidents/{копия}")),
    ] {
        let (status, _) = call(&app, m.clone(), &путь, Some(&учитель), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{m} {путь}");
    }
    let (status, _) = call(&app, Method::PUT, &format!("/api/dds-incidents/{копия}"),
        Some(&учитель), Some(json!({ "title": "Правка копии", "typeIds": ["101"],
                                      "answers": {}, "description": "правка" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let описание: String = sqlx::query_scalar("SELECT description FROM cards WHERE id = $1")
        .bind(копия).fetch_one(&pool).await.unwrap();
    assert_eq!(описание, "учебное происшествие", "копию не поправить");

    // И выдать копию занятием, как исходник, нельзя
    let (status, _) = call(&app, Method::POST, "/api/lessons", Some(&учитель),
        Some(json!({ "groupId": группа_а, "title": "Копия", "mode": "dds",
                     "cardIds": [копия] }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    прибрать(&app, &pool, &учитель, &с, &[занятие]).await;
}

/// Исходник выдаётся сколько угодно раз, в том числе той же группе:
/// каждое занятие получает свою копию, счётчик выдач растёт.
#[tokio::test]
async fn исходник_выдаётся_повторно() {
    let Some((app, pool)) = app().await else { return };
    let учитель = токен(&app, "teacher1").await;
    let mut с = стенд(&pool).await;
    let (группа_а, _) = группа(&pool, &mut с).await;
    let (первое, копия_1) = выдать(&app, &pool, &учитель, группа_а, с.исходник).await;
    call(&app, Method::POST, &format!("/api/lessons/{первое}/finish"), Some(&учитель), None).await;
    let (второе, копия_2) = выдать(&app, &pool, &учитель, группа_а, с.исходник).await;
    assert_ne!(копия_1, копия_2);

    let копий: i64 = sqlx::query_scalar("SELECT count(*) FROM cards WHERE source_card_id = $1")
        .bind(с.исходник).fetch_one(&pool).await.unwrap();
    assert_eq!(копий, 2);
    let (status, исходник) = call(&app, Method::GET, &format!("/api/dds-incidents/{}", с.исходник),
        Some(&учитель), None).await;
    assert_eq!(status, StatusCode::OK, "{исходник}");
    assert_eq!(исходник["lessons"], 2, "{исходник}");

    прибрать(&app, &pool, &учитель, &с, &[первое, второе]).await;
}

/// Исходник, у которого есть копии, удалить нельзя (409): по копиям —
/// работа курсантов, а без исходника копии оказались бы в материале.
/// Копии при этом на месте.
#[tokio::test]
async fn исходник_с_копиями_не_удаляется_копии_на_месте() {
    let Some((app, pool)) = app().await else { return };
    let учитель = токен(&app, "teacher1").await;
    let mut с = стенд(&pool).await;
    let (группа_а, _) = группа(&pool, &mut с).await;
    let (занятие, копия) = выдать(&app, &pool, &учитель, группа_а, с.исходник).await;

    let (status, ответ) = call(&app, Method::DELETE, &format!("/api/dds-incidents/{}", с.исходник),
        Some(&учитель), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{ответ}");
    let (осталось, источник): (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM cards WHERE id = $1 OR id = $2),
                (SELECT source_card_id FROM cards WHERE id = $2)")
        .bind(с.исходник).bind(копия).fetch_one(&pool).await.unwrap();
    assert_eq!(осталось, 2);
    assert_eq!(источник, Some(с.исходник));

    // Удаление исходника в обход материала (уборка базы) копию не уносит
    sqlx::query("DELETE FROM cards WHERE id = $1").bind(с.исходник).execute(&pool).await.unwrap();
    let источник: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT source_card_id FROM cards WHERE id = $1")
        .bind(копия).fetch_optional(&pool).await.unwrap();
    assert_eq!(источник, Some(None), "копия живёт без исходника");
    let в_занятии: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM lesson_cards WHERE card_id = $1)")
        .bind(копия).fetch_one(&pool).await.unwrap();
    assert!(в_занятии);

    sqlx::query("DELETE FROM cards WHERE id = $1").bind(копия).execute(&pool).await.ok();
    прибрать(&app, &pool, &учитель, &с, &[занятие]).await;
}
