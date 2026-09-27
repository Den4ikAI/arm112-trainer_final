//! Проверки парсера классификатора на настоящих файлах из материалов задачи.
//!
//! Тесты не требуют базы: проверяется только разбор XLSX.

use arm112::ingest::classifier;
use std::path::PathBuf;

fn find(dir: &str) -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(dir);
    std::fs::read_dir(root).ok()?.filter_map(|e| e.ok()).find_map(|e| {
        let p = e.path();
        (p.extension()?.to_str()? == "xlsx").then_some(p)
    })
}

#[test]
fn разбирает_актуальную_версию() {
    let Some(path) = find("task") else {
        eprintln!("пропуск: файл классификатора не найден");
        return;
    };
    let parsed = classifier::parse(&path).expect("разбор не удался");

    assert_eq!(parsed.positions.len(), 1283, "число позиций");
    assert_eq!(parsed.groups.len(), 24, "число групп");
    assert_eq!(parsed.services.len(), 58, "число служб без «Главной службы»");
}

/// Заголовок группы БПЛА в файле не имеет номера, в отличие от
/// остальных 23. Наивный парсер молча отправляет 13 позиций
/// в «Прочие происшествия» — вместе с тем самым БПЛА, ради которого
/// нужна дельта версий.
#[test]
fn группа_без_номера_в_заголовке_не_теряется() {
    let Some(path) = find("task") else { return };
    let parsed = classifier::parse(&path).unwrap();

    assert_eq!(parsed.groups.get(&24).map(String::as_str), Some("БПЛА"));

    let bpla: Vec<_> = parsed.positions.iter().filter(|p| p.group_no == 24).collect();
    assert_eq!(bpla.len(), 13, "позиций в группе БПЛА");
    assert!(
        bpla.iter().any(|p| p.number == "24120200"),
        "позиция 24120200 должна быть в группе 24"
    );
}

/// Служба занимает диапазон колонок, внутри — подколонки по условиям
/// оповещения. Поэтому 104 колонки при 58 службах.
#[test]
fn у_служб_есть_условия_оповещения() {
    let Some(path) = find("task") else { return };
    let parsed = classifier::parse(&path).unwrap();

    let mchs = parsed
        .services
        .iter()
        .find(|s| s.id == "mchs")
        .expect("служба МЧС должна быть распознана");

    assert!(
        mchs.conditions.len() >= 7,
        "у МЧС несколько условий: Служба 101, ОДС ПСЦ, МГПСС и их варианты"
    );
    assert!(mchs.conditions.iter().any(|(_, c)| c.contains("ОДС ПСЦ")));
}

#[test]
fn позиция_несёт_признаки_и_итоговый_тип() {
    let Some(path) = find("task") else { return };
    let parsed = classifier::parse(&path).unwrap();

    let p = parsed
        .positions
        .iter()
        .find(|p| p.number == "1010101")
        .expect("позиция 1010101 есть в классификаторе");

    assert_eq!(p.group_no, 1);
    assert_eq!(p.sign1.as_deref(), Some("на улице"));
    assert_eq!(p.sign2.as_deref(), Some("мусор"));
    assert_eq!(p.sign3.as_deref(), Some("открытое пламя"));
    assert_eq!(p.outcome_type.as_deref(), Some("пожар: мусор"));
    assert!(!p.services.is_empty(), "позиция должна кого-то оповещать");
}

/// Между версиями появились 8 служб и 2 позиции — на этом строится
/// функция переаттестации по дельте.
#[test]
fn две_версии_отличаются_ожидаемым_образом() {
    let (Some(new), Some(old)) = (find("task"), find("refs")) else { return };
    let new = classifier::parse(&new).unwrap();
    let old = classifier::parse(&old).unwrap();

    let old_ids: Vec<_> = old.services.iter().map(|s| s.id.as_str()).collect();
    let added: Vec<_> = new
        .services
        .iter()
        .filter(|s| !old_ids.contains(&s.id.as_str()))
        .collect();

    assert_eq!(added.len(), 8, "новых служб");
    assert!(added.iter().any(|s| s.title.contains("БПЛА")));

    let old_nums: Vec<_> = old.positions.iter().map(|p| p.number.as_str()).collect();
    let added_pos: Vec<_> = new
        .positions
        .iter()
        .filter(|p| !old_nums.contains(&p.number.as_str()))
        .collect();

    assert_eq!(added_pos.len(), 2, "новых позиций");
    assert!(added_pos.iter().any(|p| p.number == "24120200"));
}

/// В v046_11 перед «Главной службой» стоит «Сценарий реагирования» —
/// столбец технических кодов («15_10», «1_2»). Службой он не является,
/// и оповещать по нему некого.
#[test]
fn сценарий_реагирования_не_служба() {
    let Some(path) = find("refs") else { return };
    let parsed = classifier::parse(&path).unwrap();

    assert!(
        parsed.services.iter().all(|s| s.title != "Сценарий реагирования"
                                     && s.title != "Главная служба"),
        "служебные столбцы не должны попадать в службы"
    );
    // Службы нашлись и после сдвинутого столбца
    assert!(parsed.services.iter().any(|s| s.id == "mchs"));
    let p = parsed.positions.iter().find(|p| p.number == "1010101").unwrap();
    assert!(p.services.iter().all(|(id, _, _)| id != "scenarij-reagirovaniya"));
}
