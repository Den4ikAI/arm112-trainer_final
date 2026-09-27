//! Марафон и блиц: правила поверх обычного разбора.
//!
//! Пожелание команды: «блиц — это обучение на конкретных сегментах
//! карточки, а марафон — это экзамен на 5–10 карточек». Новой оценки
//! здесь нет: оба вида берут разбор `domain::grade` и только
//! ограничивают его (блиц) или откладывают показ (марафон).

use crate::domain::grade::{Finding, FindingKind, Grade};

/// Сегменты блица у приёма вызова.
pub const SEGMENTS_112: [&str; 3] = ["address", "type", "services"];
/// Сегмент блица у ДДС: статусы по докладам бригады. Разбор ДДС
/// (`domain::brigade`) и так оценивает только их.
pub const SEGMENT_DDS: &str = "statuses";

/// Сколько вызовов в марафоне — «экзамен на 5–10 карточек».
pub const MARATHON_CALLS: (usize, usize) = (5, 10);
/// Сколько заданий в серии блица. Нижняя граница — наше допущение:
/// меньше трёх — уже не серия.
pub const BLITZ_CALLS: (usize, usize) = (3, 10);
/// Сколько вызовов раздавать, если преподаватель не задал число.
pub const DEFAULT_CALLS: usize = 5;

/// Подпись сегмента — как её видит курсант.
pub fn segment_label(segment: &str) -> &'static str {
    match segment {
        "address" => "Адрес",
        "type" => "Тип происшествия и опросная карта",
        "services" => "Службы",
        "statuses" => "Статусы по докладам",
        _ => "Сегмент",
    }
}

/// Относится ли поле разбора к сегменту.
///
/// «Пострадавшие» отнесены к типу происшествия: на экране это кнопка
/// рядом с «Что случилось?», и от неё зависит список служб так же,
/// как от признаков опросной карты. Наше допущение.
pub fn in_segment(field: &str, segment: &str) -> bool {
    match segment {
        "address" => field.starts_with("address."),
        "type" => field == "typeIds" || field == "victims" || field.starts_with("answers."),
        "services" => field.starts_with("service."),
        _ => false,
    }
}

/// Балл по замечаниям: та же шкала, что у полного разбора (угадал и
/// лишнее — 1, не заполнено — 2, ошибка — 3), но без норматива
/// времени и без поправки на короткую карточку. Блиц — упражнение
/// на точность одного сегмента, время в нём не оценивается.
pub fn score_of(findings: &[Finding]) -> i32 {
    if findings.is_empty() {
        return 100;
    }
    let penalty: i32 = findings.iter().map(|f| f.kind.weight()).sum();
    let max = findings.len() as i32 * 3;
    ((100.0 * (1.0 - penalty as f64 / max as f64)).round() as i32).clamp(0, 100)
}

/// Разбор блица: только поля сегмента и балл по ним.
///
/// Полный разбор в базе не меняется — ограничение накладывается при
/// показе. Так преподаватель в отчётах по-прежнему видит карточку
/// целиком, а курсант — то, на чём было упражнение.
pub fn restrict(mut g: Grade, segment: &str) -> Grade {
    g.findings.retain(|f| in_segment(&f.field, segment));
    g.score = score_of(&g.findings);
    // Время в блице не оценивается: не красим его как нарушение
    g.over_norm = false;
    g.segment = Some(segment.to_string());
    g
}

/// Грубые ошибки для итогового протокола марафона.
///
/// Грубой считаем ошибку с наибольшим весом шкалы (неверное значение,
/// вес 3) в любом поле и пропущенную службу или пропущенный тип
/// происшествия (вес 2): от них зависит, приедет ли вообще нужная
/// служба. Наше допущение — в материалах задачи грубые ошибки
/// не перечислены.
pub fn is_gross(f: &Finding) -> bool {
    match f.kind {
        FindingKind::Wrong => true,
        FindingKind::Missing => f.field.starts_with("service.") || f.field == "typeIds",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::grade::ServiceDiff;

    fn f(kind: FindingKind, field: &str) -> Finding {
        Finding {
            kind,
            field: field.into(),
            label: field.into(),
            expected: None,
            actual: None,
            consequence: None,
        }
    }

    fn разбор(findings: Vec<Finding>) -> Grade {
        Grade {
            findings,
            elapsed_sec: 200,
            norm_sec: 60,
            over_norm: true,
            services: ServiceDiff { expected: vec![], actual: vec![], missed: vec![], extra: vec![] },
            score: 10,
            segment: None,
        }
    }

    #[test]
    fn блиц_по_адресу_не_штрафует_за_службы_и_время() {
        let g = restrict(
            разбор(vec![
                f(FindingKind::Ok, "address.street"),
                f(FindingKind::Ok, "address.house"),
                f(FindingKind::Missing, "service.mchs"),
                f(FindingKind::Wrong, "typeIds"),
            ]),
            "address",
        );
        assert_eq!(g.findings.len(), 2);
        assert_eq!(g.score, 100);
        assert!(!g.over_norm);
        assert_eq!(g.segment.as_deref(), Some("address"));
    }

    #[test]
    fn блиц_считает_балл_только_по_своему_сегменту() {
        let g = restrict(
            разбор(vec![
                f(FindingKind::Ok, "service.mchs"),
                f(FindingKind::Missing, "service.smp"),
                f(FindingKind::Wrong, "address.street"),
            ]),
            "services",
        );
        // 2 штрафа из 6 возможных
        assert_eq!(g.score, 67);
    }

    #[test]
    fn пострадавшие_и_признаки_относятся_к_типу() {
        assert!(in_segment("victims", "type"));
        assert!(in_segment("answers.101-p1", "type"));
        assert!(in_segment("typeIds", "type"));
        assert!(!in_segment("address.street", "type"));
        assert!(!in_segment("service.mchs", "type"));
    }

    #[test]
    fn грубые_ошибки_это_неверное_и_пропущенная_служба() {
        assert!(is_gross(&f(FindingKind::Wrong, "address.street")));
        assert!(is_gross(&f(FindingKind::Missing, "service.mchs")));
        assert!(is_gross(&f(FindingKind::Missing, "typeIds")));
        assert!(!is_gross(&f(FindingKind::Missing, "address.flat")));
        assert!(!is_gross(&f(FindingKind::Guessed, "address.settlement")));
        assert!(!is_gross(&f(FindingKind::Extra, "service.moslift")));
    }
}
