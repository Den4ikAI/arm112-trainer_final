//! Рекомендации курсанту и проверка того, сработали ли они.
//!
//! Подпункт критерия финальной экспертизы:
//!
//! > проверка рекомендаций сервиса и реальных рекомендаций
//! > в соответствии с реальными условиями
//!
//! Рекомендация сама по себе — декларация. Проверяется она так же,
//! как прогноз: фиксируется с временем и задним числом не меняется,
//! а в следующем разборе смотрим, ушла ли ошибка. Доля сработавших —
//! измеренная величина, и рядом с ней базовая линия: как часто та же
//! повторяющаяся ошибка уходит сама, когда рекомендации не было.
//!
//! Здесь только логика, базы модуль не знает.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Сколько последних разборов смотрим, решая, что ошибка повторяется.
pub const DEFAULT_WINDOW: usize = 5;

/// Сколько раз ошибка должна случиться в окне, чтобы считаться
/// повторяющейся. Одна ошибка — случайность, две — уже привычка.
pub const MIN_REPEATS: usize = 2;

/// Больше трёх открытых рекомендаций курсант не удержит в голове,
/// и проверять их станет нечестно: всё смешается.
pub const MAX_OPEN: usize = 3;

/// Меньше этого числа проверенных рекомендаций о доле судить рано.
pub const MIN_CHECKED: usize = 5;

/// Вид замечания в том написании, что лежит в `grades.findings`.
pub const PROBLEM_KINDS: [&str; 4] = ["guessed", "wrong", "missing", "extra"];

/// Ошибка курсанта: поле карточки и вид замечания.
///
/// Вид входит в ключ: «угадал адрес» и «не заполнил адрес» лечатся
/// разными рекомендациями.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Problem {
    pub field: String,
    pub kind: String,
}

impl Problem {
    pub fn new(field: &str, kind: &str) -> Self {
        Self { field: field.to_string(), kind: kind.to_string() }
    }

    fn is_service(&self) -> bool {
        self.field.starts_with("service.")
    }

    fn service_id(&self) -> &str {
        self.field.strip_prefix("service.").unwrap_or(&self.field)
    }

    /// Тяжесть — те же веса, что в оценке: при равной частоте
    /// рекомендуем то, что дороже обходится.
    fn weight(&self) -> i32 {
        match self.kind.as_str() {
            "wrong" => 3,
            "missing" => 2,
            _ => 1,
        }
    }
}

/// Одно замечание разбора — ровно то, что нужно рекомендациям.
#[derive(Debug, Clone)]
pub struct Mark {
    pub field: String,
    pub kind: String,
    pub label: String,
}

/// Один разбор курсанта.
#[derive(Debug, Clone, Default)]
pub struct Past {
    /// Все замечания разбора, включая `ok` — по ним видно, что поле
    /// в этом вызове вообще проверялось
    pub marks: Vec<Mark>,
    /// Службы по эталону и выбранные курсантом
    pub expected_services: Vec<String>,
    pub actual_services: Vec<String>,
}

impl Past {
    fn has(&self, p: &Problem) -> bool {
        self.marks.iter().any(|m| m.field == p.field && m.kind == p.kind)
    }

    fn label_of(&self, p: &Problem) -> Option<&str> {
        self.marks
            .iter()
            .find(|m| m.field == p.field && m.kind == p.kind)
            .map(|m| m.label.as_str())
    }

    /// Была ли в этом разборе возможность повторить ошибку, и если да —
    /// ушла ли она.
    ///
    /// `None` — возможности не было, и отсутствие ошибки ничего не
    /// доказывает: если в следующем вызове нет вопроса про газификацию,
    /// рекомендация «выясняйте газификацию» не проверена, а не выполнена.
    ///
    /// `Some(true)` — поле проверялось и заполнено верно.
    /// `Some(false)` — поле проверялось, и с ним снова не так. Ошибка
    /// другого вида на том же поле тоже считается неудачей: поле
    /// по-прежнему заполняется неправильно, засчитывать это в пользу
    /// рекомендации было бы лестью.
    pub fn outcome(&self, p: &Problem) -> Option<bool> {
        if p.is_service() {
            let id = p.service_id().to_string();
            let expected = self.expected_services.contains(&id);
            let chosen = self.actual_services.contains(&id);
            return match p.kind.as_str() {
                // Не оповестил: проверяем там, где служба по эталону нужна
                "missing" => expected.then_some(chosen),
                // Лишняя: проверяем там, где по эталону её нет
                "extra" => (!expected).then_some(!chosen),
                _ => None,
            };
        }

        let graded: Vec<&Mark> = self.marks.iter().filter(|m| m.field == p.field).collect();
        if graded.is_empty() {
            return None;
        }
        Some(graded.iter().all(|m| m.kind == "ok"))
    }
}

/// Повторяющаяся ошибка, на которую пора выдать рекомендацию.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub problem: Problem,
    pub label: String,
    /// Сколько раз встретилась в окне
    pub occurrences: usize,
}

/// Какие ошибки повторяются к концу истории.
///
/// Условие — ошибка есть в последнем разборе и встретилась хотя бы
/// дважды за окно. Последний разбор обязателен: рекомендация
/// выдаётся после разбора и про то, что в нём случилось снова, а не
/// про давно исправленное.
///
/// Порядок — по частоте, потом по тяжести, потом по полю, чтобы
/// выбор был воспроизводимым.
pub fn triggers(history: &[Past], window: usize) -> Vec<Trigger> {
    let Some(last) = history.last() else {
        return Vec::new();
    };
    let window = window.max(MIN_REPEATS);
    let tail = &history[history.len().saturating_sub(window)..];

    let mut in_last: BTreeSet<Problem> = BTreeSet::new();
    for m in &last.marks {
        if PROBLEM_KINDS.contains(&m.kind.as_str()) {
            in_last.insert(Problem::new(&m.field, &m.kind));
        }
    }

    let mut out: Vec<Trigger> = in_last
        .into_iter()
        .filter_map(|p| {
            let occurrences = tail.iter().filter(|s| s.has(&p)).count();
            (occurrences >= MIN_REPEATS).then(|| Trigger {
                label: last.label_of(&p).unwrap_or(&p.field).to_string(),
                problem: p,
                occurrences,
            })
        })
        .collect();

    out.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then(b.problem.weight().cmp(&a.problem.weight()))
            .then(a.problem.cmp(&b.problem))
    });
    out
}

/// Какие рекомендации выдать сейчас.
///
/// Открытых не больше трёх, и на одну ошибку одна открытая
/// рекомендация: пока прежняя не проверена, повторять её бессмысленно.
pub fn to_issue(history: &[Past], window: usize, open: &[Problem]) -> Vec<Trigger> {
    let slots = MAX_OPEN.saturating_sub(open.len());
    triggers(history, window)
        .into_iter()
        .filter(|t| !open.contains(&t.problem))
        .take(slots)
        .collect()
}

/// Формулировка рекомендации.
///
/// Строится только из вида замечания и подписи поля — ничего сверх
/// того, что уже написано в разборе. «Уточняйте у заявителя „Улица“»
/// выводится из того, что улица была угадана, а не выяснена;
/// никаких фактов о конкретном вызове формулировка не утверждает.
pub fn wording(p: &Problem, label: &str) -> String {
    if p.is_service() {
        return match p.kind.as_str() {
            "missing" => format!(
                "Проверяйте список оповещения: служба «{label}» по эталону нужна, а вы её не оповещали"
            ),
            "extra" => format!(
                "Не добавляйте службу «{label}» без оснований: она отвлекается на непрофильный вызов"
            ),
            _ => format!("Проверяйте службу «{label}» в списке оповещения"),
        };
    }

    match p.kind.as_str() {
        "guessed" => format!(
            "Выясняйте «{label}» у заявителя вопросом, а не вписывайте по догадке"
        ),
        "missing" => format!(
            "Не оставляйте пустым поле «{label}»: служба получит неполную карточку"
        ),
        "wrong" => format!(
            "Сверяйте «{label}» с тем, что сказал заявитель: значение расходится с эталоном"
        ),
        _ => format!("Обратите внимание на поле «{label}»"),
    }
}

/// Проверка рекомендации: первый разбор, начатый после неё, где у
/// ошибки была возможность повториться.
///
/// `after` — разборы, начатые после выдачи рекомендации, от старых
/// к новым. Возвращает номер разбора в `after` и вердикт.
///
/// Проверяем по первой возможности, а не «хоть раз получилось»:
/// иначе любая рекомендация рано или поздно «сработает».
pub fn check(p: &Problem, after: &[Past]) -> Option<(usize, bool)> {
    after
        .iter()
        .enumerate()
        .find_map(|(i, s)| s.outcome(p).map(|ok| (i, ok)))
}

/// Базовая линия: как часто повторяющаяся ошибка уходит сама.
///
/// Проходим историю курсанта так же, как это делает выдача: после
/// каждого разбора — какие ошибки повторяются, и что с каждой было
/// при следующей возможности. Случаи, на которые рекомендация была
/// открыта, исключаются: `covered(i, problem)` говорит, была ли
/// рекомендация по этой ошибке открыта в момент разбора `i`.
///
/// Смещение известно и работает против рекомендаций, а не за них:
/// рекомендации получают три самые частые ошибки, а в базовую линию
/// попадают остальные, более редкие — их и исправить проще.
pub fn baseline<F>(history: &[Past], window: usize, covered: F) -> (usize, usize)
where
    F: Fn(usize, &Problem) -> bool,
{
    let mut checked = 0usize;
    let mut worked = 0usize;
    for i in 0..history.len() {
        for t in triggers(&history[..=i], window) {
            if covered(i, &t.problem) {
                continue;
            }
            if let Some((_, ok)) = check(&t.problem, &history[i + 1..]) {
                checked += 1;
                worked += ok as usize;
            }
        }
    }
    (checked, worked)
}

/// Насколько рекомендации работают.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Efficacy {
    /// Рекомендаций, проверенных следующим разбором
    pub checked: usize,
    /// Из них сработали: ошибка при следующей возможности не повторилась
    pub worked: usize,
    /// Доля сработавших, 0..1
    pub rate: Option<f64>,
    /// Повторяющихся ошибок без рекомендации, проверенных так же
    pub baseline_checked: usize,
    /// Доля тех, что ушли сами
    pub baseline_rate: Option<f64>,
    /// Проверок мало — о доле судить рано
    pub too_few: bool,
}

pub fn efficacy(checked: usize, worked: usize, base_checked: usize, base_worked: usize) -> Efficacy {
    let rate = |w: usize, c: usize| (c > 0).then(|| ((w as f64 / c as f64) * 100.0).round() / 100.0);
    Efficacy {
        checked,
        worked,
        rate: rate(worked, checked),
        baseline_checked: base_checked,
        baseline_rate: rate(base_worked, base_checked),
        too_few: checked < MIN_CHECKED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn разбор(замечания: &[(&str, &str)]) -> Past {
        Past {
            marks: замечания
                .iter()
                .map(|(f, k)| Mark { field: f.to_string(), kind: k.to_string(), label: f.to_string() })
                .collect(),
            ..Default::default()
        }
    }

    fn со_службами(ожидались: &[&str], выбраны: &[&str]) -> Past {
        Past {
            marks: Vec::new(),
            expected_services: ожидались.iter().map(|s| s.to_string()).collect(),
            actual_services: выбраны.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn единичная_ошибка_рекомендации_не_даёт() {
        let история = vec![
            разбор(&[("address.street", "ok")]),
            разбор(&[("address.street", "guessed")]),
        ];
        assert!(triggers(&история, 5).is_empty(), "одна ошибка — случайность, не привычка");
    }

    #[test]
    fn повторившаяся_ошибка_даёт_рекомендацию() {
        let история = vec![
            разбор(&[("address.street", "guessed")]),
            разбор(&[("address.street", "ok")]),
            разбор(&[("address.street", "guessed")]),
        ];
        let т = triggers(&история, 5);
        assert_eq!(т.len(), 1);
        assert_eq!(т[0].problem, Problem::new("address.street", "guessed"));
        assert_eq!(т[0].occurrences, 2);
    }

    /// Ошибка, которой нет в последнем разборе, уже исправлена —
    /// рекомендовать её поздно.
    #[test]
    fn исправленная_к_последнему_разбору_не_рекомендуется() {
        let история = vec![
            разбор(&[("address.street", "guessed")]),
            разбор(&[("address.street", "guessed")]),
            разбор(&[("address.street", "ok")]),
        ];
        assert!(triggers(&история, 5).is_empty());
    }

    #[test]
    fn старая_ошибка_за_окном_не_считается() {
        let история = vec![
            разбор(&[("answers.gasified", "wrong")]),
            разбор(&[]),
            разбор(&[]),
            разбор(&[("answers.gasified", "wrong")]),
        ];
        assert!(triggers(&история, 3).is_empty(), "первая ошибка за окном в три разбора");
        assert_eq!(triggers(&история, 4).len(), 1);
    }

    #[test]
    fn частые_и_тяжёлые_впереди() {
        let три = [("service.mosgaz", "missing"), ("address.street", "guessed"), ("answers.x", "wrong")];
        let история = vec![
            разбор(&три),
            разбор(&[("service.mosgaz", "missing"), ("answers.x", "wrong")]),
            разбор(&три),
        ];
        let т = triggers(&история, 5);
        let порядок: Vec<&str> = т.iter().map(|t| t.problem.field.as_str()).collect();
        // Мосгаз и x встречались трижды, x тяжелее (ошибка значения)
        assert_eq!(порядок, vec!["answers.x", "service.mosgaz", "address.street"]);
    }

    #[test]
    fn открытых_не_больше_трёх_и_без_повторов() {
        let много = [("a", "wrong"), ("b", "wrong"), ("c", "wrong"), ("d", "wrong")];
        let история = vec![разбор(&много), разбор(&много)];

        let сразу = to_issue(&история, 5, &[]);
        assert_eq!(сразу.len(), MAX_OPEN);

        let открыта = vec![Problem::new("a", "wrong")];
        let ещё = to_issue(&история, 5, &открыта);
        assert_eq!(ещё.len(), MAX_OPEN - 1);
        assert!(ещё.iter().all(|t| t.problem.field != "a"), "на одну ошибку одна открытая");
    }

    /// Нет вопроса в следующем вызове — рекомендация не проверена,
    /// а не выполнена.
    #[test]
    fn без_возможности_повторить_рекомендация_не_проверена() {
        let п = Problem::new("answers.gasified", "guessed");
        let без_поля = разбор(&[("address.street", "ok")]);
        assert_eq!(без_поля.outcome(&п), None);
        assert_eq!(check(&п, &[без_поля]), None);
    }

    #[test]
    fn проверка_берёт_первую_возможность() {
        let п = Problem::new("answers.gasified", "guessed");
        let после = vec![
            разбор(&[("address.street", "ok")]),        // вопроса нет
            разбор(&[("answers.gasified", "guessed")]), // первая возможность — не вышло
            разбор(&[("answers.gasified", "ok")]),      // позже получилось, но поздно
        ];
        assert_eq!(check(&п, &после), Some((1, false)),
                   "иначе любая рекомендация рано или поздно «сработает»");
    }

    #[test]
    fn другая_ошибка_на_том_же_поле_не_засчитывается() {
        let п = Problem::new("address.street", "guessed");
        let снова_не_так = разбор(&[("address.street", "wrong")]);
        assert_eq!(снова_не_так.outcome(&п), Some(false));
    }

    #[test]
    fn пропущенная_служба_проверяется_где_она_нужна() {
        let п = Problem::new("service.mosgaz", "missing");
        assert_eq!(со_службами(&["mchs"], &["mchs"]).outcome(&п), None, "МОСГАЗ не нужен — не проверка");
        assert_eq!(со_службами(&["mosgaz"], &["mosgaz"]).outcome(&п), Some(true));
        assert_eq!(со_службами(&["mosgaz"], &[]).outcome(&п), Some(false));
    }

    #[test]
    fn лишняя_служба_проверяется_где_она_не_нужна() {
        let п = Problem::new("service.mosgaz", "extra");
        assert_eq!(со_службами(&["mosgaz"], &["mosgaz"]).outcome(&п), None, "там она по делу");
        assert_eq!(со_службами(&["mchs"], &["mchs"]).outcome(&п), Some(true));
        assert_eq!(со_службами(&["mchs"], &["mchs", "mosgaz"]).outcome(&п), Some(false));
    }

    #[test]
    fn формулировка_не_выдумывает_фактов() {
        let поле = wording(&Problem::new("address.street", "guessed"), "Улица");
        assert!(поле.contains("«Улица»") && поле.contains("заявител"));
        let служба = wording(&Problem::new("service.mosgaz", "missing"), "Классификатор МОСГАЗ");
        assert!(служба.contains("«Классификатор МОСГАЗ»"));
    }

    #[test]
    fn базовая_линия_исключает_закрытые_рекомендацией_случаи() {
        let история = vec![
            разбор(&[("address.street", "guessed")]),
            разбор(&[("address.street", "guessed")]),
            разбор(&[("address.street", "ok")]),
        ];
        // Без рекомендаций: после второго разбора ошибка повторялась,
        // а в третьем ушла
        assert_eq!(baseline(&история, 5, |_, _| false), (1, 1));
        // Если на неё была рекомендация — в базовую линию не идёт
        assert_eq!(baseline(&история, 5, |_, _| true), (0, 0));
    }

    #[test]
    fn мало_проверок_честно_помечено() {
        let э = efficacy(2, 1, 10, 3);
        assert!(э.too_few);
        assert_eq!(э.rate, Some(0.5));
        assert_eq!(э.baseline_rate, Some(0.3));
        assert!(efficacy(0, 0, 0, 0).rate.is_none(), "не из чего считать — нечего показывать");
    }
}
