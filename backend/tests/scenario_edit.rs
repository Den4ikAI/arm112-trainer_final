//! Правка учебного вызова, который уже отыгран.
//!
//! Скрытый факт, который курсант успел выяснить, — часть истории
//! занятия: из неё считается «выяснил, а не угадал». Правка вызова
//! не должна её рушить, но и не должна отказывать преподавателю
//! в удалении лишнего вопроса.
//!
//! Запуск: `cargo test --test scenario_edit`

use arm112::config::Config;
use arm112::domain::scenario_edit::{self, HiddenRow, Patch};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Куда ходят проверки.
///
/// Отдельная база, если она задана: иначе прогон копит мусор в той же,
/// на которой стоит стенд — в учётных записях накапливаются сотни
/// курсантов, а в переписке сценариев реплики «проверка 1789977379959».
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn база() -> Option<PgPool> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    Some(pool)
}

fn факт(ключ: &str) -> HiddenRow {
    HiddenRow {
        key: ключ.into(),
        question: format!("вопрос про {ключ}"),
        answer: format!("ответ про {ключ}"),
        unlocks: vec![],
        statement: None,
    }
}

/// Вызов с двумя скрытыми фактами и активная версия классификатора.
async fn вызов(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let version_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM classifier_versions WHERE is_active LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("нужна активная версия классификатора");

    let scenario_id: Uuid = sqlx::query_scalar(
        "INSERT INTO scenarios
            (legend, applicant_name, applicant_phone, applicant_status,
             expected, version_id)
         VALUES ('горит мусоропровод', 'Петров Денис Андреевич',
                 '916-000-00-00', 'очевидец', $1, $2)
         RETURNING id",
    )
    .bind(json!({ "typeIds": ["101"], "answers": {} }))
    .bind(version_id)
    .fetch_one(pool)
    .await
    .unwrap();

    let первый: Uuid = sqlx::query_scalar(
        "INSERT INTO scenario_hidden_facts
            (scenario_id, key, question, answer, unlocks, sort_order)
         VALUES ($1, 'floor', 'на каком этаже?', 'на пятом', '{}', 1)
         RETURNING id",
    )
    .bind(scenario_id)
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO scenario_hidden_facts
            (scenario_id, key, question, answer, unlocks, sort_order)
         VALUES ($1, 'smoke', 'дым есть?', 'да, густой', '{}', 2)",
    )
    .bind(scenario_id)
    .execute(pool)
    .await
    .unwrap();

    (version_id, scenario_id, первый)
}

/// Занятие, в котором курсант выяснил указанный факт.
async fn отыграть(pool: &PgPool, scenario_id: Uuid, fact_id: Uuid) -> Uuid {
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(pool)
        .await
        .unwrap();

    let session_id: Uuid = sqlx::query_scalar(
        "INSERT INTO sessions (user_id, scenario_id, mode, norm_sec)
         VALUES ($1, $2, 'card112', 60) RETURNING id",
    )
    .bind(user_id)
    .bind(scenario_id)
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO revealed_facts (session_id, fact_id, at_sec) VALUES ($1, $2, 12)",
    )
    .bind(session_id)
    .bind(fact_id)
    .execute(pool)
    .await
    .unwrap();

    session_id
}

#[tokio::test]
async fn выясненный_факт_переживает_правку_вызова() {
    let Some(pool) = база().await else { return };
    let (version_id, scenario_id, факт_этажа) = вызов(&pool).await;
    let session_id = отыграть(&pool, scenario_id, факт_этажа).await;

    // Преподаватель убирает вопрос про этаж — тот самый, который
    // курсант уже выяснил. Раньше это падало на внешнем ключе
    let patch = Patch { hidden: Some(vec![факт("smoke")]), ..Default::default() };
    let после = scenario_edit::apply(&pool, version_id, scenario_id, &patch)
        .await
        .expect("правка отыгранного вызова не должна падать");

    // Преподавателю убранного вопроса больше не видно
    let ключи: Vec<&str> = после.hidden.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(ключи, vec!["smoke"]);

    // Но строка цела, и история занятия вместе с ней
    let (жив, снят): (bool, bool) = sqlx::query_as(
        "SELECT true, retired FROM scenario_hidden_facts WHERE id = $1",
    )
    .bind(факт_этажа)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(жив && снят, "факт должен остаться, но быть снят с учёта");

    let выяснено: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM revealed_facts WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(выяснено, 1, "выясненное курсантом не должно пропасть");

    sqlx::query("DELETE FROM sessions WHERE id = $1").bind(session_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM scenarios WHERE id = $1").bind(scenario_id).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn невостребованный_факт_удаляется_совсем() {
    let Some(pool) = база().await else { return };
    let (version_id, scenario_id, _) = вызов(&pool).await;

    // Ни одного занятия не было, значит копить нечего
    let patch = Patch { hidden: Some(vec![факт("floor")]), ..Default::default() };
    scenario_edit::apply(&pool, version_id, scenario_id, &patch).await.unwrap();

    let всего: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scenario_hidden_facts WHERE scenario_id = $1",
    )
    .bind(scenario_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(всего, 1, "черновик не должен копить мусор от правок");

    sqlx::query("DELETE FROM scenarios WHERE id = $1").bind(scenario_id).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn возвращённый_вопрос_снова_в_строю() {
    let Some(pool) = база().await else { return };
    let (version_id, scenario_id, факт_этажа) = вызов(&pool).await;
    let session_id = отыграть(&pool, scenario_id, факт_этажа).await;

    // Убрали…
    let patch = Patch { hidden: Some(vec![факт("smoke")]), ..Default::default() };
    scenario_edit::apply(&pool, version_id, scenario_id, &patch).await.unwrap();

    // …и передумали
    let patch = Patch {
        hidden: Some(vec![факт("smoke"), факт("floor")]),
        ..Default::default()
    };
    let после = scenario_edit::apply(&pool, version_id, scenario_id, &patch).await.unwrap();

    let ключи: Vec<&str> = после.hidden.iter().map(|f| f.key.as_str()).collect();
    assert!(ключи.contains(&"floor"), "вернувшийся вопрос должен снова показываться");

    // Вернулась та же строка, а не новая: ссылка из занятия цела
    let id_сейчас: Uuid = sqlx::query_scalar(
        "SELECT id FROM scenario_hidden_facts WHERE scenario_id = $1 AND key = 'floor'",
    )
    .bind(scenario_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(id_сейчас, факт_этажа);

    sqlx::query("DELETE FROM sessions WHERE id = $1").bind(session_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM scenarios WHERE id = $1").bind(scenario_id).execute(&pool).await.unwrap();
}

/// Удаление вызова, выданного группе.
///
/// Раньше решение «удалить или заархивировать» принималось по одним
/// занятиям. Вызов, назначенный группе, но ещё никем не начатый,
/// занятий не имеет — и удаление упиралось в ссылку из заданий.
#[tokio::test]
async fn выданный_группе_вызов_архивируется_а_не_падает() {
    use arm112::{api, config::Config, ml::MlSuite, state::AppState};
    use axum::{body::Body, http::{header, Method, Request, StatusCode}};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let Some(pool) = база().await else { return };
    let Some(config) = Config::from_env().ok() else { return };
    let Some(ml) = MlSuite::build(config.ml_provider).ok() else { return };
    let app = api::routes().with_state(AppState::new(pool.clone(), config, ml));

    let (_, scenario_id, _) = вызов(&pool).await;

    let teacher: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let group_id: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id",
    )
    .bind(format!("проверка-{}", Uuid::new_v4()))
    .bind(teacher)
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO assignments (group_id, scenario_id, created_by) VALUES ($1, $2, $3)",
    )
    .bind(group_id)
    .bind(scenario_id)
    .bind(teacher)
    .execute(&pool)
    .await
    .unwrap();

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "login": "teacher1", "password": "arm112" }).to_string()))
        .unwrap();
    let body = app.clone().oneshot(req).await.unwrap().into_body().collect().await.unwrap().to_bytes();
    let token = serde_json::from_slice::<Value>(&body).unwrap()["token"].as_str().unwrap().to_string();

    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/api/scenarios/{scenario_id}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let status = app.oneshot(req).await.unwrap().status();
    assert_eq!(status, StatusCode::NO_CONTENT, "удаление не должно падать");

    let статус: String = sqlx::query_scalar("SELECT status::text FROM scenarios WHERE id = $1")
        .bind(scenario_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(статус, "archived", "выданный вызов архивируется, а не исчезает");

    sqlx::query("DELETE FROM assignments WHERE scenario_id = $1").bind(scenario_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM training_groups WHERE id = $1").bind(group_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM scenarios WHERE id = $1").bind(scenario_id).execute(&pool).await.unwrap();
}

/// Смена ФИО правит и скрытый факт, где заявитель его называет.
///
/// Иначе заявитель в разговоре продолжит называть старое имя, а
/// эталон будет ждать новое — курсант не сможет выяснить то, что
/// от него требуют.
#[tokio::test]
async fn смена_фио_доходит_до_ответа_заявителя() {
    let Some(pool) = база().await else { return };
    let (version_id, scenario_id, _) = вызов(&pool).await;

    sqlx::query(
        "INSERT INTO scenario_hidden_facts
            (scenario_id, key, question, answer, unlocks, sort_order)
         VALUES ($1, 'name', 'Как вас зовут?', 'Меня зовут Петров Денис Андреевич.', '{}', 3)",
    )
    .bind(scenario_id)
    .execute(&pool)
    .await
    .unwrap();

    let patch = Patch {
        applicant_name: Some("Смирнова Анна Павловна".into()),
        ..Default::default()
    };
    let после = scenario_edit::apply(&pool, version_id, scenario_id, &patch).await.unwrap();

    assert_eq!(после.applicant_name, "Смирнова Анна Павловна");

    let ответ = после
        .hidden
        .iter()
        .find(|f| f.key == "name")
        .expect("вопрос про имя на месте");
    assert!(ответ.answer.contains("Смирнова Анна Павловна"),
            "заявитель должен называть новое имя, а не «{}»", ответ.answer);
    assert!(!ответ.answer.contains("Петров"), "старое имя должно уйти");

    sqlx::query("DELETE FROM scenarios WHERE id = $1")
        .bind(scenario_id).execute(&pool).await.unwrap();
}
