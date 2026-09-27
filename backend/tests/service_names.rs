//! Названия служб — как в боевой системе, а не как заголовки столбцов
//! классификатора.
//!
//! Проверка на стенде 23.09 (REF-01): курсант выбирал «Классификатор МЧС»
//! и «Классификатор СМП», а на рабочем АРМ в окне «Добавьте службы»
//! стоят «Служба 101 (ГУ МЧС России по г.Москве , …)» и «Служба 103 (…)».
//!
//! Запуск: `cargo test --test service_names`

use arm112::{
    api,
    config::Config,
    ingest::classifier::{self, Parsed, ServiceColumn},
    ml::MlSuite,
    state::AppState,
};
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

/// Отдельная база, если задана: прогон не должен трогать стенд.
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
    b["token"].as_str().unwrap().to_string()
}

async fn службы(app: &Router) -> Vec<Value> {
    let tok = token(app, "student1").await;
    let (status, body) = call(app, Method::GET, "/api/catalog/services", Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

fn найти<'a>(список: &'a [Value], id: &str) -> Option<&'a Value> {
    список.iter().find(|s| s["id"] == id)
}

/// Кадр `СЛУЖБЫ 112/image1`: полное имя в окне добавления, на плашке
/// полосы оповещения — «Служба 101» (кадр `КАРТОЧКА 112/image19`).
#[tokio::test]
async fn службы_называются_как_на_кадрах_боевой_системы() {
    let Some((app, _)) = app().await else { return };
    let список = службы(&app).await;

    let мчс = найти(&список, "mchs").expect("служба МЧС есть в справочнике");
    assert_eq!(
        мчс["title"],
        "Служба 101 (ГУ МЧС России по г.Москве , ГКУ \"Пожарно спасательный центр\" ОДС)"
    );
    assert_eq!(мчс["shortTitle"], "Служба 101");
    assert_eq!(мчс["sourceTitle"], "Классификатор МЧС", "заголовок столбца не теряется");

    let мосгаз = найти(&список, "mosgaz").expect("МОСГАЗ есть в справочнике");
    assert_eq!(мосгаз["shortTitle"], "Служба 104");
}

/// Курсант, привыкший к «скорой», должен найти «Службу 103»: поиск в
/// окне добавления идёт и по дополнительным словам.
#[tokio::test]
async fn у_службы_103_есть_слово_для_поиска_скорая() {
    let Some((app, _)) = app().await else { return };
    let список = службы(&app).await;
    let Some(смп) = найти(&список, "smp") else { return };

    assert!(смп["title"].as_str().unwrap().starts_with("Служба 103 ("));
    let слова: Vec<&str> = смп["aliases"].as_array().unwrap()
        .iter().map(|v| v.as_str().unwrap()).collect();
    assert!(слова.contains(&"скорая"), "{слова:?}");
}

/// Службы, которой нет на кадрах, не переименовываем по догадке:
/// остаётся заголовок из классификатора, плашка — он же.
#[tokio::test]
async fn служба_без_кадра_сохраняет_заголовок_классификатора() {
    let Some((app, _)) = app().await else { return };
    let список = службы(&app).await;
    let Some(росгвардия) = найти(&список, "rosgvardiya") else { return };

    assert_eq!(росгвардия["title"], "Росгвардия");
    assert_eq!(росгвардия["shortTitle"], "Росгвардия");
    assert_eq!(росгвардия["aliases"], json!([]));
}

/// Импорт новой версии классификатора обновляет заголовок столбца, но
/// не возвращает его на экран курсанта вместо имени с кадра.
#[tokio::test]
async fn повторный_импорт_не_возвращает_заголовок_столбца() {
    let Some((_, pool)) = app().await else { return };

    let parsed = Parsed {
        services: vec![ServiceColumn {
            id: "mchs".into(),
            title: "Классификатор МЧС".into(),
            conditions: vec![],
        }],
        positions: vec![],
        groups: Default::default(),
    };
    let report = classifier::store(&pool, &parsed, "проверка имён служб", "проверка.xlsx")
        .await
        .unwrap();

    let (title, source): (String, Option<String>) =
        sqlx::query_as("SELECT title, source_title FROM services WHERE id = 'mchs'")
            .fetch_one(&pool)
            .await
            .unwrap();

    // Пустую версию сразу убираем: она не должна мешать другим проверкам
    sqlx::query("DELETE FROM classifier_versions WHERE id = $1")
        .bind(report.version_id)
        .execute(&pool)
        .await
        .unwrap();

    assert!(title.starts_with("Служба 101 ("), "{title}");
    assert_eq!(source.as_deref(), Some("Классификатор МЧС"));
}

/// «Сценарий реагирования» из v046_11 — столбец технических кодов,
/// а не служба. Раньше парсер заводил по нему тысячу «оповещений».
#[tokio::test]
async fn сценарий_реагирования_не_оповещается() {
    let Some((_, pool)) = app().await else { return };

    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM classifier_position_services
          WHERE service_id = 'scenarij-reagirovaniya'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}
