//! Полный список типов «Что случилось?».
//!
//! Источник — refs/КАРТОЧКА 112.docx, материалы экспертов из чата
//! задачи. Раньше типов было 19, и 12 групп классификатора из 24 были
//! недостижимы: оператор не мог завести обрушение, угрозу взрыва,
//! «Ребёнок в опасности».
//!
//! Запуск: `cargo test --test incident_types`

use arm112::{
    api,
    config::Config,
    domain::classify::classify,
    ingest::survey::{derived_question_id, materialise},
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
use std::collections::{BTreeSet, HashMap};
use tower::ServiceExt;
use uuid::Uuid;

/// Куда ходят проверки: отдельная база, если она задана.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, PgPool, Uuid)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let version: Uuid =
        sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
            .fetch_optional(&pool)
            .await
            .ok()??;
    // Сервер строит карты при запуске; в проверке запуска нет.
    // Ошибка здесь — не «нет базы», а поломка: молча пропускать нельзя
    materialise(&pool, version).await.expect("карты не собрались");
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let router = api::routes().with_state(AppState::new(pool.clone(), config, ml));
    Some((router, pool, version))
}

async fn get(app: &Router, path: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn токен(app: &Router) -> String {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "login": "student1", "password": "arm112" }).to_string()))
        .unwrap();
    let bytes = app.clone().oneshot(req).await.unwrap()
        .into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<Value>(&bytes).unwrap()["token"].as_str().unwrap().to_string()
}

/// Типы, добавленные по полному списку, и их группы. Сверяется с тем,
/// что легло в базу: привязку нельзя незаметно поменять.
const НОВЫЕ: [(&str, Option<i32>); 32] = [
    ("transport-objects", Some(12)),
    ("collapse", Some(5)),
    ("collapse-threat", Some(6)),
    ("blast-threat", Some(4)),
    ("hazmat-threat", Some(11)),
    ("ecology", Some(8)),
    ("child-danger", Some(18)),
    ("death", Some(19)),
    ("social-help", Some(20)),
    ("animals", Some(21)),
    ("other", Some(23)),
    ("natural-disaster", Some(7)),
    ("water-pooling", Some(7)),
    ("radiation", Some(10)),
    ("broken-thermometer", Some(11)),
    ("help-services", Some(23)),
    ("complaint", Some(23)),
    ("training", Some(23)),
    ("ref-102", Some(23)),
    ("ref-103", Some(23)),
    ("ref-104", Some(23)),
    ("ref-gibdd", Some(23)),
    ("ref-city", Some(23)),
    ("ref-mchs", Some(23)),
    ("road-hazards", Some(16)),
    ("internal-call", None),
    ("extra-call", None),
    ("non-target", None),
    ("feedback-112", None),
    ("gathering", None),
    ("tech-failure", None),
    ("emergency-notice", None),
];

#[tokio::test]
async fn в_справочнике_весь_список_из_боевой_системы() {
    let Some((_, pool, _)) = app().await else { return };

    let всего: i64 = sqlx::query_scalar("SELECT count(*) FROM incident_types")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(всего, 51, "в выпадающем списке боевой системы 51 тип");

    for (id, группа) in НОВЫЕ {
        let в_базе: Option<Option<i32>> =
            sqlx::query_scalar("SELECT group_no FROM incident_types WHERE id = $1")
                .bind(id)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(в_базе, Some(группа), "тип {id}");
    }
}

/// Каждая группа классификатора достижима хотя бы одним типом.
///
/// Раньше 12 групп из 24 не открывал ни один тип: происшествие было
/// в классификаторе, а завести его оператор не мог.
#[tokio::test]
async fn каждая_группа_классификатора_достижима() {
    let Some((_, pool, v)) = app().await else { return };

    let в_классификаторе: BTreeSet<i32> = sqlx::query_scalar(
        "SELECT DISTINCT group_no FROM classifier_positions WHERE version_id = $1",
    )
    .bind(v)
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .collect();

    let достижимые: BTreeSet<i32> = sqlx::query_scalar(
        "SELECT DISTINCT group_no FROM incident_types WHERE group_no IS NOT NULL",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .collect();

    let недостижимые: Vec<_> = в_классификаторе.difference(&достижимые).collect();
    assert!(недостижимые.is_empty(), "группы без единого типа: {недостижимые:?}");
}

/// У типа с группой есть опросная карта, у типа без группы — нет,
/// и сервер отвечает 204, а не падает.
#[tokio::test]
async fn карта_есть_ровно_у_типов_с_группой() {
    let Some((app, _, _)) = app().await else { return };
    let токен = токен(&app).await;

    for (id, группа) in НОВЫЕ {
        let (status, карта) = get(&app, &format!("/api/catalog/survey-cards/{id}"), &токен).await;
        match группа {
            Some(_) => {
                assert_eq!(status, StatusCode::OK, "у {id} должна быть карта");
                assert_eq!(карта["source"], "classifier");
            }
            None => assert_eq!(status, StatusCode::NO_CONTENT, "у {id} карты быть не должно"),
        }
    }
}

/// Ответы, взятые из настоящей позиции группы.
///
/// Берём позицию прямо из классификатора, а не собираем комбинацию
/// из вариантов карты: так она заведомо существует.
async fn ответы_позиции(pool: &PgPool, v: Uuid, type_id: &str, group_no: i32)
    -> HashMap<String, Vec<String>>
{
    let (s1, s2, s3): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT sign1, sign2, sign3 FROM classifier_positions
          WHERE version_id = $1 AND group_no = $2
            AND sign1 IS NOT NULL AND sign1 <> ''
            AND sign1 <> 'Не отображается оператору 112'
          ORDER BY number LIMIT 1",
    )
    .bind(v)
    .bind(group_no)
    .fetch_one(pool)
    .await
    .unwrap();

    let mut ответы = HashMap::new();
    ответы.insert(derived_question_id(type_id, 1), vec![s1]);
    if let Some(s2) = s2.filter(|s| !s.is_empty()) {
        ответы.insert(derived_question_id(type_id, 2), vec![s2]);
    }
    if let Some(s3) = s3.filter(|s| !s.is_empty()) {
        ответы.insert(derived_question_id(type_id, 3), vec![s3]);
    }
    ответы
}

/// По типам из прежде недостижимых групп вызов классифицируется,
/// и службы подбираются.
#[tokio::test]
async fn вызов_из_новых_групп_классифицируется() {
    let Some((_, pool, v)) = app().await else { return };

    for (id, группа) in [("collapse", 5), ("child-danger", 18), ("blast-threat", 4)] {
        let ответы = ответы_позиции(&pool, v, id, группа).await;
        let r = classify(&pool, v, &[id.to_string()], &ответы).await.unwrap();

        assert!(r.reason.is_none(), "{id}: классификатор жалуется: {:?}", r.reason);
        assert!(r.outcome.is_some(), "{id}: итоговый тип не определился");
        assert!(!r.services.is_empty(), "{id}: службы не подобрались");
    }
}

/// Тип без группы классифицировать нечем — и сервер говорит об этом
/// словами, а не выдумывает группу.
#[tokio::test]
async fn тип_без_группы_объясняет_почему() {
    let Some((_, pool, v)) = app().await else { return };

    let r = classify(&pool, v, &["non-target".to_string()], &HashMap::new())
        .await
        .unwrap();

    assert!(r.outcome.is_none());
    assert!(r.services.is_empty());
    assert!(r.reason.as_deref().is_some_and(|p| p.contains("не задана группа")),
            "{:?}", r.reason);
}
