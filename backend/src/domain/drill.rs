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

/// Засчитанная доля одной проверки сегмента.
///
/// Верность поля — та же, что в полном разборе (`domain::grade`):
/// верно — вся проверка; угадано без вопроса — значение верное, и,
/// как в полном разборе, засчитано две трети. Не заполнено, ошибка и
/// лишняя служба в блице — ноль: упражнение на то, заполнен ли сегмент
/// верно, и пустой сегмент не должен давать баллов. Наше допущение.
fn passed(kind: FindingKind) -> f64 {
    match kind {
        FindingKind::Ok => 1.0,
        FindingKind::Guessed => 1.0 - FindingKind::Guessed.weight() as f64 / 3.0,
        FindingKind::Wrong | FindingKind::Missing | FindingKind::Extra => 0.0,
    }
}

/// Балл блица — доля верных проверок сегмента, без норматива времени
/// и без поправки на короткую карточку: блиц — упражнение на точность
/// одного сегмента, время в нём не оценивается.
///
/// Верные поля адреса и опросной карты в разборе есть замечаниями
/// «верно», а верно выбранные службы и тип — нет: разбор перечисляет
/// только пропущенные и лишние. Их проверки добавляем здесь: каждая
/// эталонная служба, которую курсант выбрал, и тип, если он не
/// пропущен. Иначе почти верный сегмент и пустой получали один балл.
pub fn segment_score(g: &Grade, segment: &str) -> i32 {
    let mut checks: Vec<f64> = g
        .findings
        .iter()
        .filter(|f| in_segment(&f.field, segment))
        .map(|f| passed(f.kind))
        .collect();
    match segment {
        "services" => {
            let chosen = g.services.expected.iter().filter(|s| !g.services.missed.contains(s));
            checks.extend(chosen.map(|_| 1.0));
        }
        "type" if !g.findings.iter().any(|f| f.field == "typeIds") => checks.push(1.0),
        _ => {}
    }
    if checks.is_empty() {
        return 100;
    }
    let sum: f64 = checks.iter().sum();
    ((100.0 * sum / checks.len() as f64).round() as i32).clamp(0, 100)
}

/// Разбор блица: только поля сегмента и балл по ним.
///
/// Полный разбор в базе не меняется — ограничение накладывается при
/// показе. Так преподаватель в отчётах по-прежнему видит карточку
/// целиком, а курсант — то, на чём было упражнение.
pub fn restrict(mut g: Grade, segment: &str) -> Grade {
    g.score = segment_score(&g, segment);
    g.findings.retain(|f| in_segment(&f.field, segment));
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
        // Из двух эталонных служб выбрана одна: ошибка адреса не в счёт
        assert_eq!(g.score, 50);
    }

    fn службы(expected: &[&str], missed: &[&str], extra: &[&str]) -> ServiceDiff {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        ServiceDiff { expected: v(expected), actual: vec![], missed: v(missed), extra: v(extra) }
    }

    #[test]
    fn блиц_пустой_адрес_ноль_почти_верный_выше_верный_сто() {
        let пустой = restrict(
            разбор(vec![
                f(FindingKind::Missing, "address.street"),
                f(FindingKind::Missing, "address.house"),
                f(FindingKind::Missing, "address.settlement"),
            ]),
            "address",
        );
        assert_eq!(пустой.score, 0);

        let почти = restrict(
            разбор(vec![
                f(FindingKind::Ok, "address.street"),
                f(FindingKind::Ok, "address.house"),
                f(FindingKind::Ok, "address.settlement"),
                f(FindingKind::Missing, "address.flat"),
            ]),
            "address",
        );
        assert_eq!(почти.score, 75);

        let верный = restrict(
            разбор(vec![
                f(FindingKind::Ok, "address.street"),
                f(FindingKind::Ok, "address.house"),
            ]),
            "address",
        );
        assert_eq!(верный.score, 100);
    }

    #[test]
    fn блиц_по_службам_считает_верно_выбранные() {
        let mut пусто = разбор(vec![
            f(FindingKind::Missing, "service.mchs"),
            f(FindingKind::Missing, "service.smp"),
            f(FindingKind::Missing, "service.mvd"),
        ]);
        пусто.services = службы(&["mchs", "smp", "mvd"], &["mchs", "smp", "mvd"], &[]);
        assert_eq!(restrict(пусто, "services").score, 0);

        // Две из трёх выбраны, одна лишняя: 2 верных проверки из 4
        let mut почти = разбор(vec![
            f(FindingKind::Missing, "service.mvd"),
            f(FindingKind::Extra, "service.moslift"),
        ]);
        почти.services = службы(&["mchs", "smp", "mvd"], &["mvd"], &["moslift"]);
        assert_eq!(restrict(почти, "services").score, 50);

        let mut верно = разбор(vec![]);
        верно.services = службы(&["mchs", "smp"], &[], &[]);
        assert_eq!(restrict(верно, "services").score, 100);
    }

    #[test]
    fn блиц_по_типу_засчитывает_выбранный_тип() {
        let пустой = restrict(
            разбор(vec![
                f(FindingKind::Missing, "typeIds"),
                f(FindingKind::Missing, "answers.101-p1"),
            ]),
            "type",
        );
        assert_eq!(пустой.score, 0);

        let верный = restrict(разбор(vec![f(FindingKind::Ok, "answers.101-p1")]), "type");
        assert_eq!(верный.score, 100);
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
