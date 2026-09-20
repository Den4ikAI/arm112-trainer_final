//! Машиночитаемое описание API.
//!
//! Главная проверка здесь не «схема существует», а «схема не врёт».
//! Описание, разошедшееся с кодом, хуже отсутствующего: интегратор
//! доверяет ему и пишет клиент под несуществующий контракт.
//!
//! Поэтому список маршрутов берётся из исходников роутеров, а не
//! пишется рядом: забытый `#[utoipa::path]` на новом обработчике
//! обязан ронять этот тест.
//!
//! Запуск: `cargo test --test openapi`

use arm112::api::openapi::{ApiDoc, NOT_IN_SPEC};
use std::collections::BTreeSet;
use utoipa::OpenApi;

/// Все пути, зарегистрированные в роутерах.
///
/// Читаем исходники: у axum 0.8 нет способа перечислить маршруты
/// собранного `Router`, а держать второй список руками — ровно та
/// беда, от которой тест и защищает.
fn registered_paths() -> BTreeSet<String> {
    let mut paths = BTreeSet::new();

    for entry in std::fs::read_dir("src/api").unwrap().chain(std::fs::read_dir("src/ws").unwrap()) {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();

        // Ищем вхождение, а не начало строки: маршрут пишут и с новой
        // строки, и сразу после Router::new()
        for piece in text.split(".route(\"").skip(1) {
            let Some(url) = piece.split('"').next() else { continue };
            paths.insert(url.to_string());
        }
    }

    paths
}

fn documented_paths() -> BTreeSet<String> {
    ApiDoc::openapi().paths.paths.keys().cloned().collect()
}

/// Каждый маршрут описан. Забыли аннотацию — тест падает и называет
/// пропущенный путь.
#[test]
fn все_маршруты_попали_в_схему() {
    let registered = registered_paths();
    let documented = documented_paths();
    let skip: BTreeSet<String> = NOT_IN_SPEC.iter().map(|s| s.to_string()).collect();

    assert!(!registered.is_empty(), "не удалось прочитать маршруты из исходников");

    let missing: Vec<&String> = registered
        .iter()
        .filter(|p| !documented.contains(*p) && !skip.contains(*p))
        .collect();

    assert!(missing.is_empty(), "маршруты без описания в OpenAPI: {missing:?}");
}

/// И наоборот: описанного, но несуществующего пути быть не должно —
/// по нему сгенерировали бы клиент, который получит 404.
#[test]
fn в_схеме_нет_несуществующих_маршрутов() {
    let registered = registered_paths();
    let documented = documented_paths();

    let extra: Vec<&String> = documented.iter().filter(|p| !registered.contains(*p)).collect();

    assert!(extra.is_empty(), "в схеме есть пути, которых нет в роутере: {extra:?}");
}

/// Токен получают один раз и дальше шлют заголовком — это должно быть
/// видно из схемы, иначе интегратору придётся догадываться.
#[test]
fn схема_описывает_аутентификацию() {
    let doc = ApiDoc::openapi();
    let components = doc.components.expect("в схеме нет components");

    assert!(components.security_schemes.contains_key("bearer"),
        "не описана схема аутентификации");
    assert!(doc.info.description.as_ref().is_some_and(|d| d.contains("Bearer")),
        "в описании нет указания, как передавать токен");
}

/// Ключевые структуры описаны: без них клиент не собрать.
#[test]
fn основные_схемы_данных_описаны() {
    let doc = ApiDoc::openapi();
    let schemas: BTreeSet<String> = doc
        .components
        .expect("нет components")
        .schemas
        .keys()
        .cloned()
        .collect();

    for name in [
        "LoginResponse", "UserView", "CardView", "ClassifyResult",
        "Grade", "Finding", "ScenarioPublic", "SurveyCard",
    ] {
        assert!(schemas.contains(name), "в схеме нет {name}");
    }
}

/// У разбора занятия должны быть описаны виды находок: на различии
/// «выяснил» и «угадал» держится весь продукт, и оно обязано быть
/// видно тому, кто встраивается.
#[test]
fn различие_выяснил_и_угадал_видно_в_схеме() {
    let doc = ApiDoc::openapi();
    let json = serde_json::to_string(&doc).unwrap();

    assert!(json.contains("guessed"), "в схеме нет вида находки «угадал»");
    assert!(json.contains("missing") && json.contains("wrong"));
}

/// Документ должен быть валидным JSON нужной версии: его будут
/// скармливать генераторам клиентов.
#[test]
fn схема_сериализуется_в_валидный_openapi() {
    let json = serde_json::to_value(ApiDoc::openapi()).unwrap();

    assert!(json["openapi"].as_str().unwrap().starts_with("3."));
    assert!(!json["info"]["title"].as_str().unwrap().is_empty());
    assert!(json["paths"].as_object().unwrap().len() > 25);
}
