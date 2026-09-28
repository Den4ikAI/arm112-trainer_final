//! Машиночитаемое описание API.
//!
//! Критерий экспертизы — «возможность интеграции в любые другие
//! системы». Проза в BACKEND.md для этого не годится: по ней нельзя
//! сгенерировать клиент и нельзя проверить, что сервер отвечает тем,
//! что обещал.
//!
//! Схема собирается из самих обработчиков и структур, а не пишется
//! отдельно. Отдельная разошлась бы с кодом на первой же правке —
//! и тогда была бы хуже, чем её отсутствие: интегратор доверял бы
//! описанию, которое врёт.
//!
//! За тем, чтобы ни один маршрут не остался неописанным, следит тест
//! `tests/openapi.rs`.

use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Тренажёр оператора ДДС (АРМ-112)",
        version = env!("CARGO_PKG_VERSION"),
        description = "\
Серверная часть учебного тренажёра для операторов Системы-112 \
и диспетчеров дежурно-диспетчерских служб.

Все содержательные расчёты живут здесь: подбор служб по Единому \
классификатору происшествий, разбор занятия и машина статусов \
реагирования. Интерфейс их не дублирует, поэтому сторонняя система \
может пользоваться теми же эндпоинтами и получать те же результаты.

**Аутентификация.** `POST /api/auth/login` возвращает JWT, который \
дальше передаётся заголовком `Authorization: Bearer <token>`. \
Тот же токен открывает канал событий `/ws?token=<token>`: заголовок \
там выставить нельзя, это ограничение браузерного клиента.

**Разграничение доступа.** Роль и служба — независимые оси. Роль \
говорит, что человек умеет делать, служба (`serviceId`) — какие \
карточки он видит. ДДС это организация, а не человек.",
    ),
    tags(
        (name = "Аутентификация", description = "Вход, токен, текущий пользователь"),
        (name = "Справочники", description = "Типы происшествия, службы, опросные карты"),
        (name = "Классификатор", description = "Подбор служб, версии и их сравнение"),
        (name = "Занятие", description = "Учебный вызов: карточка, разговор, разбор"),
        (name = "Журнал", description = "Лента карточек и статусы реагирования ДДС"),
        (name = "Отчёты", description = "По занятию и по группе"),
        (name = "Администрирование", description = "Журнал действий и учётные записи"),
        (name = "Вызовы", description = "Подготовка и правка учебных вызовов"),
        (name = "Голос", description = "Распознавание реплики и озвучивание собеседника"),
        (name = "Состояние", description = "Живость сервиса"),
    ),
    paths(
        crate::api::auth::login,
        crate::api::auth::logout,
        crate::api::auth::me,
        crate::api::catalog::incident_types,
        crate::api::catalog::services,
        crate::api::catalog::survey_card,
        crate::api::catalog::applicant_statuses,
        crate::api::catalog::evaluate,
        crate::api::sessions::list_scenarios,
        crate::api::sessions::create,
        crate::api::sessions::ask,
        crate::api::sessions::ask_free,
        crate::api::sessions::patch_card,
        crate::api::sessions::save,
        crate::api::sessions::transcript,
        crate::api::sessions::grade_view,
        crate::api::sessions::active,
        crate::api::sessions::state_view,
        crate::api::cards::list,
        crate::api::cards::one,
        crate::api::cards::add_response,
        crate::api::cards::opened,
        crate::api::reports::list_groups,
        crate::api::reports::session,
        crate::api::reports::group_report,
        crate::api::reports::group_insights,
        crate::api::reports::job,
        crate::api::admin::audit_log,
        crate::api::admin::list_users,
        crate::api::admin::create_user,
        crate::api::admin::set_active,
        crate::api::admin::delete_user,
        crate::api::admin::versions,
        crate::api::admin::activate_version,
        crate::api::admin::version_diff,
        crate::api::admin::version_impact,
        crate::api::scenarios::one,
        crate::api::scenarios::edit,
        crate::api::scenarios::remove,
        crate::api::scenarios::chat_history,
        crate::api::scenarios::chat_turn,
        crate::api::scenarios::approve,
        crate::api::scenarios::edit_statements,
        crate::api::scenarios::prepare_statements,
        crate::api::scenarios::generate,
        crate::api::lessons::list,
        crate::api::lessons::create,
        crate::api::lessons::one,
        crate::api::lessons::remove,
        crate::api::lessons::start,
        crate::api::lessons::finish,
        crate::api::drill::protocol,
        crate::api::forecast::group,
        crate::api::forecast::mine,
        crate::api::forecast::my_advice,
        crate::api::forecast::group_advice,
        crate::api::forecast::settings,
        crate::api::forecast::set_setting,
        crate::api::brigade::view,
        crate::api::brigade::assign,
        crate::api::brigade::call,
        crate::api::review::view,
        crate::api::review::set_expert,
        crate::api::review::set_verdict,
        crate::api::review::analyse,
        crate::api::chief::view,
        crate::api::chief::start,
        crate::api::chief::say,
        crate::api::chief::finish,
        crate::api::voice::status,
        crate::api::voice::asr,
        crate::api::voice::tts,
        crate::api::health::health,
    ),
    modifiers(&BearerAuth),
)]
pub struct ApiDoc;

/// Схема безопасности одна на всё API, поэтому проще добавить её
/// модификатором, чем повторять в каждом обработчике.
struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .description(Some("Токен из POST /api/auth/login"))
                        .build(),
                ),
            );
        }
    }
}

/// Пути, которые описывать незачем: WebSocket в OpenAPI не
/// выражается, а сама документация себя не документирует.
pub const NOT_IN_SPEC: [&str; 1] = ["/ws"];
