//! Справочники служб и типов — ровно как в боевой системе.
//!
//! Источник — кадры материалов экспертов: окно «Добавьте службы»
//! (`refs/СЛУЖБЫ 112/image1…55`) и выпадающий список «что случилось?»
//! (`refs/КАРТОЧКА 112/image2…13`). Выписки — `docs/справочники/`,
//! миграция `20260929100000_catalog_by_frames.sql`.
//!
//! Запуск: `cargo test --test catalog_frames`. Без базы — пропускаются.

use arm112::{api, config::Config, domain::classify::classify, ml::MlSuite, state::AppState};
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

async fn справочник(app: &Router, путь: &str) -> Vec<Value> {
    let tok = token(app, "student1").await;
    let (status, body) = call(app, Method::GET, путь, Some(&tok), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().clone()
}

/// Окно «Добавьте службы» — 211 служб в порядке прокрутки кадров, начало
/// списка — как на `image1`, конец — как на `image55`.
#[tokio::test]
async fn окно_служб_совпадает_с_кадрами() {
    let Some((app, _)) = app().await else { return };
    let все = справочник(&app, "/api/catalog/services").await;
    let окно: Vec<&Value> = все.iter().filter(|s| s["listed"] == true).collect();

    assert_eq!(окно.len(), 211, "на кадрах image1…55 — 211 служб");

    let начало: Vec<&str> = окно.iter().take(6).map(|s| s["title"].as_str().unwrap()).collect();
    assert_eq!(начало, vec![
        "Служба 101 (ГУ МЧС России по г.Москве , ГКУ \"Пожарно спасательный центр\" ОДС)",
        "ФСБ",
        "ЦЭМП",
        "Служба 103 (ГБУ города Москвы Станция скорой и неотложной медицинской помощи им.А.С. Пучкова)",
        "Служба 104 (АО \"МОСГАЗ\" Диспетчерское управление)",
        "ОГДЦ",
    ], "начало списка — кадр image1");
    assert_eq!(окно.last().unwrap()["title"],
               "Мосжилинспекция (Государственная жилищная инспекция города Москвы)",
               "конец списка — кадр image55");

    // Сервер отдаёт окно по порядку кадров, без пропусков
    let позиции: Vec<i64> = окно.iter().map(|s| s["position"].as_i64().unwrap()).collect();
    assert_eq!(позиции, (1..=211).collect::<Vec<_>>());
}

/// Районные ДДС и префектуры, которых раньше не было, теперь есть; на
/// плашке — часть до скобки.
#[tokio::test]
async fn новые_службы_с_кадров_заведены() {
    let Some((app, _)) = app().await else { return };
    let все = справочник(&app, "/api/catalog/services").await;
    let найти = |id: &str| все.iter().find(|s| s["id"] == id).cloned();

    let щукино = найти("poselenie-schukino").expect("Поселение Щукино — кадр image12");
    assert_eq!(щукино["title"], "Поселение Щукино (ДДС района Щукино города Москвы)");
    assert_eq!(щукино["shortTitle"], "Поселение Щукино");
    assert_eq!(щукино["listed"], true);

    for (id, кадр) in [("ogdc", 1), ("112-mos-obl", 4), ("gbu-ad-vao", 51), ("upr-rajona-veshnyaki", 8)] {
        assert!(найти(id).is_some_and(|s| s["listed"] == true), "{id} — кадр image{кадр}");
    }
}

/// Столбцы классификатора, которых в окне нет, скрыты, но остаются в
/// справочнике: по ним подписаны старые карточки и журнал.
#[tokio::test]
async fn службы_не_с_кадров_скрыты_но_названы() {
    let Some((app, _)) = app().await else { return };
    let все = справочник(&app, "/api/catalog/services").await;
    let оив = все.iter().find(|s| s["id"] == "territorial-nye-oiv")
        .expect("служба не удаляется — на неё ссылаются карточки и матрица");
    assert_eq!(оив["listed"], false);
    assert!(оив["position"].is_null());
}

/// Скрытая служба не подбирается и не входит в эталон. Для пожара в
/// квартире (позиция 1050101) матрица оповещения назначает ДГП —
/// реагирующей службой, — а в окне «Добавьте службы» ДГП нет: курсант не
/// смог бы её добавить, а разбор штрафовал бы за её отсутствие.
#[tokio::test]
async fn скрытые_службы_не_подбираются_и_не_в_эталоне() {
    let Some((_, pool)) = app().await else { return };
    let Some(версия): Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM classifier_versions ORDER BY is_active DESC, imported_at DESC LIMIT 1",
    ).fetch_optional(&pool).await.unwrap() else { return };

    // Предпосылка: в матрице позиции действительно есть скрытая служба,
    // иначе проверка ничего не доказывает
    let скрытых_в_матрице: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM classifier_position_services ps
           JOIN classifier_positions p ON p.id = ps.position_id
          WHERE p.version_id = $1 AND p.number = '1050101'
            AND NOT EXISTS (SELECT 1 FROM service_catalog c WHERE c.service_id = ps.service_id)",
    ).bind(версия).fetch_one(&pool).await.unwrap();
    assert!(скрытых_в_матрице > 0, "в матрице пожара в квартире есть службы не из окна");

    let ответы: HashMap<String, Vec<String>> = [
        ("where", "Дом"), ("inhouse", "Квартира"), ("fire-sign", "Открытое пламя / Дым"),
    ].iter().map(|(k, v)| (k.to_string(), vec![v.to_string()])).collect();
    let r = classify(&pool, версия, &["101".into()], &ответы).await.unwrap();

    let в_окне: Vec<String> = sqlx::query_scalar("SELECT service_id FROM service_catalog")
        .fetch_all(&pool).await.unwrap();
    for s in r.services.iter().chain(r.informational.iter()) {
        assert!(в_окне.contains(&s.service_id), "{} нет в окне «Добавьте службы»", s.service_id);
    }
    assert!(r.services.iter().any(|s| s.service_id == "mchs"), "МЧС при этом на месте");
    assert!(!r.services.iter().any(|s| s.service_id.starts_with("dgp")));
}

/// Выпадающий список «что случилось?» — 51 тип в порядке кадров, с их
/// написанием; быстрые плашки — слева направо по кадру пустой карточки.
#[tokio::test]
async fn типы_совпадают_с_выпадающим_списком() {
    let Some((app, _)) = app().await else { return };
    let все = справочник(&app, "/api/catalog/incident-types").await;
    let список: Vec<&str> = все.iter().filter(|t| t["listed"] == true)
        .map(|t| t["title"].as_str().unwrap()).collect();

    assert_eq!(список.len(), 51, "кадры image2…13");
    assert_eq!(&список[..6], &[
        "101", "102", "103", "104",
        "Аварии и происшествия в городском хозяйстве",
        "Аварии и происшествия на транспортных объектах",
    ]);
    // Боевая система ставит «Благодарность службам» перед «БПЛА» (image3–4)
    let благ = список.iter().position(|t| *t == "Благодарность службам").unwrap();
    let бпла = список.iter().position(|t| *t == "БПЛА").unwrap();
    assert!(благ < бпла);
    // Написание — как на кадре, а не как в таблице документа
    for t in ["Справка-102", "Справка-ГИБДД", "Справка Городское Хозяйство",
              "Внутренний звонок (звонки от работников)", "Экологические происшествия",
              "Технический сбой (сбой в работе оборудования 112 Москва)"] {
        assert!(список.contains(&t), "{t}");
    }
    assert_eq!(список.last(), Some(&"Экологические происшествия"));

    let mut быстрые: Vec<(i64, &str)> = все.iter()
        .filter_map(|t| t["quickOrder"].as_i64().map(|n| (n, t["title"].as_str().unwrap())))
        .collect();
    быстрые.sort();
    let быстрые: Vec<&str> = быстрые.into_iter().map(|(_, t)| t).collect();
    assert_eq!(быстрые, vec![
        "ДТП", "Ошибочно набран номер", "104", "Человек в опасности", "Отмена вызова",
        "Тестовый вызов", "Передача дежурства", "Консультация",
        "Вызов на иностранном языке", "Справка-101",
    ], "кадр kartochka_01");
}
