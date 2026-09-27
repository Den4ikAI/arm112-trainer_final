//! Бригада диспетчера ДДС: назначение, доклады старшего, звонок
//! старшему и разбор.
//!
//! Раньше диспетчер ДДС просто нажимал статусы, и проверить требование
//! памятки — «статусы по факту получения информации» — было не на чем.
//! Здесь проверяется, что источник сведений появился и что разбор
//! отличает статус по докладу от статуса наугад.
//!
//! Каждая проверка заводит свою службу, своего курсанта и свою карточку:
//! прогон идёт параллельно по одной базе.
//!
//! Запуск: `cargo test --test brigade`

use arm112::{api, config::Config, jobs, ml::MlSuite, state::AppState, ws::Hub};
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

/// Куда ходят проверки.
///
/// Отдельная база, если она задана: иначе прогон копит мусор в той же,
/// на которой стоит стенд.
fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn app() -> Option<(Router, PgPool, Hub)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let ml = MlSuite::build(config.ml_provider).ok()?;
    let state = AppState::new(pool.clone(), config, ml);
    let hub = state.ws.clone();
    Some((api::routes().with_state(state), pool, hub))
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

/// Своя служба, свой курсант этой службы и сохранённая карточка,
/// направленная в неё.
struct Стенд {
    service: String,
    login: String,
    user_id: Uuid,
    card: Uuid,
}

async fn стенд(pool: &PgPool) -> Стенд {
    let метка = Uuid::new_v4().simple().to_string();
    let service = format!("проверка-бр-{}", &метка[..10]);
    let login = format!("бригадир-{}", &метка[..10]);

    sqlx::query("INSERT INTO services (id, title) VALUES ($1, 'Проверочная служба')")
        .bind(&service).execute(pool).await.unwrap();

    // Пароль тот же, что у остальных учебных учёток
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (login, password_hash, fio, role, service_id)
         SELECT $1, password_hash, 'Проверочный Б.Б.', 'student', $2
           FROM users WHERE login = 'student1'
         RETURNING id",
    )
    .bind(&login).bind(&service).fetch_one(pool).await.unwrap();

    let номер = format!("Б{}", &метка[..10]);
    let card: Uuid = sqlx::query_scalar(
        "INSERT INTO cards (number, saved_at, type_ids, description)
         VALUES ($1, now(), '{101}', 'проверочная карточка') RETURNING id",
    )
    .bind(&номер).fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO card_services (card_id, service_id) VALUES ($1, $2)")
        .bind(card).bind(&service).execute(pool).await.unwrap();

    Стенд { service, login, user_id, card }
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

/// Первая запись — статус и текст: так требует заказчик (чат, 27.09),
/// статус без текста разбор засчитывает замечанием.
async fn принять(app: &Router, с: &Стенд, токен: &str) {
    let (status, ответ) = call(app, Method::POST, &format!("/api/cards/{}/responses", с.card),
        Some(токен), Some(json!({ "serviceId": с.service, "status": "Принята",
                                  "comment": "Принято в работу" }))).await;
    assert_eq!(status, StatusCode::CREATED, "{ответ}");
}

async fn назначить(app: &Router, с: &Стенд, токен: &str) -> (StatusCode, Value) {
    call(app, Method::POST, &format!("/api/cards/{}/brigade", с.card), Some(токен),
        Some(json!({ "serviceId": с.service, "brigadeNo": 2 }))).await
}

async fn бригада(app: &Router, с: &Стенд, токен: &str) -> Value {
    let (status, ответ) = call(app, Method::GET,
        &format!("/api/cards/{}/brigade?serviceId={}", с.card, с.service), Some(токен), None).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    ответ
}

/// Доставка докладов общая на всю базу: разосланный одной проверкой
/// доклад мог оказаться докладом соседней, и та не видела своего
/// события. Кто старит шкалу и зовёт доставку, делает это под замком.
static ДОСТАВКА: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Этап наступил давно: сдвигаем шкалу, а не ждём. Так же проверки
/// сторожа нормативов старят карточку.
async fn наступил(pool: &PgPool, с: &Стенд, этап: &str, секунд_назад: i64) {
    sqlx::query(
        "UPDATE brigade_reports r SET due_at = now() - make_interval(secs => $3)
           FROM brigade_assignments a
          WHERE a.id = r.assignment_id AND a.card_id = $1 AND r.stage = $2::response_status",
    )
    .bind(с.card).bind(этап).bind(секунд_назад as f64)
    .execute(pool).await.unwrap();
}

fn замечание<'a>(вид: &'a Value, этап: &str) -> &'a Value {
    вид["debrief"]["findings"].as_array().unwrap()
        .iter().find(|f| f["stage"] == этап)
        .unwrap_or_else(|| panic!("нет замечания по этапу «{этап}»: {вид}"))
}

/// Бригаду направляют после «Принята» — не раньше.
#[tokio::test]
async fn бригаду_назначают_только_по_принятой_карточке() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;

    let до = бригада(&app, &с, &курсант).await;
    assert_eq!(до["canAssign"], false);
    let (status, _) = назначить(&app, &с, &курсант).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "до «Принята» бригаду не направить");

    принять(&app, &с, &курсант).await;
    assert_eq!(бригада(&app, &с, &курсант).await["canAssign"], true);

    let (status, вид) = назначить(&app, &с, &курсант).await;
    assert_eq!(status, StatusCode::CREATED, "{вид}");
    assert_eq!(вид["assignment"]["brigadeNo"], 2);

    let (status, _) = назначить(&app, &с, &курсант).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "вторую бригаду той же службе не назначить");

    // Номер вне списка учебных бригад
    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/brigade", с.card),
        Some(&курсант), Some(json!({ "serviceId": с.service, "brigadeNo": 99 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    прибрать(&pool, &с).await;
}

/// Будущий этап — скрытая истина: курсант его не видит, преподаватель видит.
#[tokio::test]
async fn шкала_работ_курсанту_не_видна() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    принять(&app, &с, &курсант).await;
    назначить(&app, &с, &курсант).await;

    let у_курсанта = бригада(&app, &с, &курсант).await;
    assert_eq!(у_курсанта["reports"].as_array().unwrap().len(), 0,
               "ни о чём ещё не узнал — ничего и не видит");

    let преподаватель = токен(&app, "teacher1").await;
    let у_преподавателя = бригада(&app, &с, &преподаватель).await;
    let доклады = у_преподавателя["reports"].as_array().unwrap();
    assert_eq!(доклады.len(), 4, "вся шкала работ");
    assert!(доклады.iter().all(|d| d["dueAt"].is_string()));

    прибрать(&pool, &с).await;
}

/// Первый вариант из ответа заказчика: старший звонит сам.
#[tokio::test]
async fn старший_докладывает_сам_после_задержки() {
    let Some((app, pool, hub)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    принять(&app, &с, &курсант).await;
    назначить(&app, &с, &курсант).await;

    let _замок = ДОСТАВКА.lock().await;

    // Этап наступил, но задержка доклада ещё не прошла — доклада нет
    наступил(&pool, &с, "Начало реагирования", 1).await;
    jobs::brigade::deliver(&pool, &hub).await.unwrap();
    assert_eq!(бригада(&app, &с, &курсант).await["reports"].as_array().unwrap().len(), 0);

    // Прошло больше задержки — старший позвонил
    let mut события = hub.subscribe();
    наступил(&pool, &с, "Начало реагирования", 3600).await;
    let доставлено = jobs::brigade::deliver(&pool, &hub).await.unwrap();
    assert!(доставлено >= 1);

    let вид = бригада(&app, &с, &курсант).await;
    let доклад = &вид["reports"][0];
    assert_eq!(доклад["stage"], "Начало реагирования");
    assert_eq!(доклад["channel"], "incoming");
    assert_eq!(доклад["text"], "Старший бригады 2: выехали на вызов.");
    assert!(доклад["dueAt"].is_null(), "время этапа курсанту не отдаётся");

    // Входящий звонок разослан по каналу, чтобы карточка показала его сразу
    let событие = события.try_recv().expect("событие о докладе");
    assert_eq!(событие.kind, "brigade.report");

    прибрать(&pool, &с).await;
}

/// Второй вариант: диспетчер звонит старшему сам и узнаёт раньше доклада.
#[tokio::test]
async fn звонок_старшему_раскрывает_наступившее_раньше_доклада() {
    let Some((app, pool, hub)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    принять(&app, &с, &курсант).await;
    назначить(&app, &с, &курсант).await;

    let звонок = format!("/api/cards/{}/brigade/call", с.card);

    // Пока ничего не наступило — «без изменений»
    let (status, ответ) = call(&app, Method::POST, &звонок, Some(&курсант),
        Some(json!({ "serviceId": с.service }))).await;
    assert_eq!(status, StatusCode::OK, "{ответ}");
    assert!(ответ["learned"].is_null());
    assert_eq!(ответ["text"], "Старший бригады 2: пока без изменений.");

    // Выехали секунду назад: сам старший позвонит только через задержку
    наступил(&pool, &с, "Начало реагирования", 1).await;
    let (_, ответ) = call(&app, Method::POST, &звонок, Some(&курсант),
        Some(json!({ "serviceId": с.service }))).await;
    assert_eq!(ответ["learned"], "Начало реагирования");
    assert_eq!(ответ["text"], "Старший бригады 2: выехали на вызов.");
    assert_eq!(ответ["brigade"]["reports"][0]["channel"], "outgoing");
    assert_eq!(ответ["brigade"]["calls"].as_array().unwrap().len(), 2);
    assert!(ответ["brigade"]["calls"][0]["said"].is_null(), "без голоса реплики нет");

    // Голосом: реплика диспетчера остаётся в разговоре — преподаватель
    // видит, как курсант ведёт разговор
    let (_, ответ) = call(&app, Method::POST, &звонок, Some(&курсант),
        Some(json!({ "serviceId": с.service, "said": "  Доложите обстановку.  " }))).await;
    assert_eq!(ответ["brigade"]["calls"][2]["said"], "Доложите обстановку.");

    // Узнанное звонком старший сам уже не повторяет
    let _замок = ДОСТАВКА.lock().await;
    наступил(&pool, &с, "Начало реагирования", 3600).await;
    jobs::brigade::deliver(&pool, &hub).await.unwrap();
    let вид = бригада(&app, &с, &курсант).await;
    assert_eq!(вид["reports"].as_array().unwrap().len(), 1);
    assert_eq!(вид["reports"][0]["channel"], "outgoing");

    прибрать(&pool, &с).await;
}

/// Главное в разборе: статус раньше сведений — не по факту.
#[tokio::test]
async fn статус_наугад_в_разборе_не_по_факту() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    принять(&app, &с, &курсант).await;
    назначить(&app, &с, &курсант).await;

    // Докладов не было, а статус поставлен
    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/responses", с.card),
        Some(&курсант), Some(json!({ "serviceId": с.service, "status": "Начало реагирования" }))).await;
    assert_eq!(status, StatusCode::CREATED);

    let вид = бригада(&app, &с, &курсант).await;
    let з = замечание(&вид, "Начало реагирования");
    assert_eq!(з["kind"], "notByFact");
    assert!(з["consequence"].as_str().unwrap().contains("не по факту"));
    assert!(вид["debrief"]["score"].as_i64().unwrap() < 100);

    прибрать(&pool, &с).await;
}

/// Статус по докладу, в нормативе, засчитывается.
#[tokio::test]
async fn статус_по_докладу_засчитан() {
    let Some((app, pool, hub)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    принять(&app, &с, &курсант).await;
    назначить(&app, &с, &курсант).await;

    {
        let _замок = ДОСТАВКА.lock().await;
        наступил(&pool, &с, "Начало реагирования", 3600).await;
        jobs::brigade::deliver(&pool, &hub).await.unwrap();
    }

    call(&app, Method::POST, &format!("/api/cards/{}/responses", с.card),
        Some(&курсант), Some(json!({ "serviceId": с.service, "status": "Начало реагирования" }))).await;

    let вид = бригада(&app, &с, &курсант).await;
    assert_eq!(замечание(&вид, "Начало реагирования")["kind"], "ok");
    assert_eq!(вид["debrief"]["score"], 100);

    прибрать(&pool, &с).await;
}

/// Диспетчер отвечает за свою службу и ни за чью больше.
#[tokio::test]
async fn чужой_службе_бригада_недоступна() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    принять(&app, &с, &курсант).await;

    // Курсант другой службы
    let чужой = токен(&app, "student1").await;
    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/brigade", с.card),
        Some(&чужой), Some(json!({ "serviceId": с.service, "brigadeNo": 1 }))).await;
    assert!(status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
            "назначить бригаду чужой службе нельзя: {status}");

    let (status, _) = call(&app, Method::POST, &format!("/api/cards/{}/brigade/call", с.card),
        Some(&курсант), Some(json!({ "serviceId": с.service }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "звонить некому, пока бригада не назначена");

    прибрать(&pool, &с).await;
}

/// Ход занятия ДДС получает балл из разбора работы с бригадой.
///
/// Раньше балла в этом режиме не было вовсе — разбирать было нечего.
#[tokio::test]
async fn ход_занятия_ддс_получает_балл() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let teacher = токен(&app, "teacher1").await;
    let курсант = токен(&app, &с.login).await;

    let учитель: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(&pool).await.unwrap();
    let группа: Uuid = sqlx::query_scalar(
        "INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id")
        .bind(format!("занятие-{}", Uuid::new_v4().simple()))
        .bind(учитель)
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO group_members (group_id, user_id) VALUES ($1, $2)")
        .bind(группа).bind(с.user_id).execute(&pool).await.unwrap();

    let (_, занятие) = call(&app, Method::POST, "/api/lessons", Some(&teacher),
        Some(json!({ "groupId": группа, "title": "Доклады бригады", "mode": "dds" }))).await;
    let id = занятие["id"].as_str().unwrap().to_string();
    let (status, начато) = call(&app, Method::POST, &format!("/api/lessons/{id}/start"),
        Some(&teacher), None).await;
    assert_eq!(status, StatusCode::OK, "{начато}");

    принять(&app, &с, &курсант).await;
    назначить(&app, &с, &курсант).await;
    call(&app, Method::POST, &format!("/api/cards/{}/responses", с.card),
        Some(&курсант), Some(json!({ "serviceId": с.service, "status": "Начало реагирования" }))).await;

    let (_, ход) = call(&app, Method::GET, &format!("/api/lessons/{id}"), Some(&teacher), None).await;
    let балл = &ход["progress"][0]["avgScore"];
    assert!(балл.is_number(), "в занятии ДДС должен появиться балл: {ход}");

    call(&app, Method::POST, &format!("/api/lessons/{id}/finish"), Some(&teacher), None).await;
    for sql in [
        "DELETE FROM training_groups WHERE id = $1",
    ] {
        sqlx::query(sql).bind(группа).execute(&pool).await.ok();
    }
    прибрать(&pool, &с).await;
}

// ─── Приём карточки: открытие и первая запись ───────────────────────

/// Карточка поступила давно: сдвигаем сохранение, а не ждём.
async fn поступила(pool: &PgPool, с: &Стенд, секунд_назад: i64) {
    sqlx::query("UPDATE cards SET saved_at = now() - make_interval(secs => $2) WHERE id = $1")
        .bind(с.card).bind(секунд_назад as f64).execute(pool).await.unwrap();
}

fn виды(вид: &Value) -> Vec<String> {
    вид["debrief"]["findings"].as_array().unwrap().iter()
        .map(|f| f["kind"].as_str().unwrap().to_string())
        .collect()
}

async fn записи(pool: &PgPool, с: &Стенд, статус: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM response_entries
          WHERE card_id = $1 AND service_id = $2 AND status = $3::response_status")
        .bind(с.card).bind(&с.service).bind(статус)
        .fetch_one(pool).await.unwrap()
}

/// Открытие оставляет «Получена службой» один раз. Преподаватель,
/// открывший карточку, норматив диспетчера не выполняет.
#[tokio::test]
async fn открытие_карточки_отмечается_один_раз() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    let teacher = токен(&app, "teacher1").await;
    let путь = format!("/api/cards/{}/opened", с.card);

    let (status, _) = call(&app, Method::POST, &путь, Some(&teacher), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(записи(&pool, &с, "Получена службой").await, 0, "преподаватель следа не оставляет");

    for _ in 0..2 {
        let (status, _) = call(&app, Method::POST, &путь, Some(&курсант), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    assert_eq!(записи(&pool, &с, "Получена службой").await, 1);

    // Открытие — системная запись, первичные статусы всё ещё доступны
    let (_, карточка) = call(&app, Method::GET, &format!("/api/cards/{}", с.card),
        Some(&курсант), None).await;
    let своя = карточка["services"].as_array().unwrap().iter()
        .find(|x| x["serviceId"] == с.service.as_str()).unwrap();
    assert_eq!(своя["allowedNext"], json!(["Принята", "Не принята"]));

    прибрать(&pool, &с).await;
}

/// Ответ заказчика 27.09: 30 секунд на открытие, 3 минуты на первую
/// запись со статусом и текстом — считая от поступления карточки.
#[tokio::test]
async fn нормативы_приёма_в_разборе() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;

    // Только что поступила — судить рано
    let вид = бригада(&app, &с, &курсант).await;
    assert!(виды(&вид).is_empty(), "{вид}");
    assert_eq!(вид["debrief"]["pending"], 2);

    // Прошла минута без открытия — замечание по открытию, запись ещё ждёт
    поступила(&pool, &с, 60).await;
    let вид = бригада(&app, &с, &курсант).await;
    assert_eq!(виды(&вид), vec!["openedLate"]);

    // Открыл и принял без текста через четыре минуты после поступления
    поступила(&pool, &с, 240).await;
    call(&app, Method::POST, &format!("/api/cards/{}/opened", с.card), Some(&курсант), None).await;
    call(&app, Method::POST, &format!("/api/cards/{}/responses", с.card),
        Some(&курсант), Some(json!({ "serviceId": с.service, "status": "Принята" }))).await;
    let вид = бригада(&app, &с, &курсант).await;
    let mut итог = виды(&вид);
    итог.sort();
    assert_eq!(итог, vec!["firstRecordLate", "firstRecordNoText", "openedLate"]);
    let запись = вид["debrief"]["findings"].as_array().unwrap().iter()
        .find(|f| f["kind"] == "firstRecordLate").unwrap();
    assert!(запись["lagSec"].as_i64().unwrap() >= 240);

    прибрать(&pool, &с).await;
}

/// В занятии нормативы считаются от выпуска карточки в поток, а не от
/// давнего сохранения: старая карточка занятия не просрочена с первой
/// секунды.
#[tokio::test]
async fn в_занятии_нормативы_от_выпуска_карточки() {
    let Some((app, pool, _)) = app().await else { return };
    let с = стенд(&pool).await;
    let курсант = токен(&app, &с.login).await;
    поступила(&pool, &с, 3 * 24 * 3600).await;

    let учитель: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE login = 'teacher1'")
        .fetch_one(&pool).await.unwrap();
    let занятие: Uuid = sqlx::query_scalar(
        "WITH г AS (INSERT INTO training_groups (title, teacher_id) VALUES ($1, $2) RETURNING id)
         INSERT INTO lessons (group_id, teacher_id, title, mode, started_at)
         SELECT id, $2, 'Поток', 'dds', now() FROM г RETURNING id")
        .bind(format!("поток-{}", Uuid::new_v4().simple()))
        .bind(учитель)
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO lesson_cards (lesson_id, card_id, release_at) VALUES ($1, $2, now())")
        .bind(занятие).bind(с.card).execute(&pool).await.unwrap();

    let вид = бригада(&app, &с, &курсант).await;
    assert!(виды(&вид).is_empty(), "карточка только что выпущена: {вид}");

    // Выпуск минуту назад — открытие просрочено
    sqlx::query("UPDATE lesson_cards SET release_at = now() - interval '60 seconds'
                  WHERE lesson_id = $1")
        .bind(занятие).execute(&pool).await.unwrap();
    let вид = бригада(&app, &с, &курсант).await;
    assert_eq!(виды(&вид), vec!["openedLate"]);

    sqlx::query("DELETE FROM training_groups WHERE id = (SELECT group_id FROM lessons WHERE id = $1)")
        .bind(занятие).execute(&pool).await.ok();
    прибрать(&pool, &с).await;
}
