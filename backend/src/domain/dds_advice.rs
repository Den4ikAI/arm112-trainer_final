//! Рекомендации диспетчеру ДДС и проверка того, сработали ли они.
//!
//! Рекомендации финалистам, п. 5 и 10: «ИИ формирует задание →
//! обучаемый ДДС выполняет → результат → рекомендации для дальнейшего
//! обучения» без преподавателя. Здесь — звено «рекомендации».
//!
//! Устройство то же, что у рекомендаций оператору 112
//! (`domain::advice`): рекомендация фиксируется с временем после
//! разбора и задним числом не меняется, а первая следующая карточка,
//! где у ошибки была возможность повториться, ставит вердикт. Доля
//! сработавших — измеренная величина, рядом базовая линия.
//!
//! Отличие одно: у оператора 112 ошибка становится привычкой со второго
//! раза, а замечание ДДС выдаётся уже с первого. Замечание ДДС — не
//! «угадал поле», а нарушение памятки («статусы по факту получения
//! информации») или норматива заказчика (30 с на открытие, 3 мин на
//! первую запись): одного раза достаточно, чтобы сказать, что
//! тренировать дальше. Это допущение, см. OPEN-QUESTIONS.md.
//!
//! Тексты рекомендаций — из тех же нормативов и последствий, что
//! называет разбор (`domain::brigade::consequence`): ничего сверх того,
//! что курсант уже видел в замечании.
//!
//! Здесь только логика, базы модуль не знает.

use crate::domain::advice::{efficacy, Efficacy, MAX_OPEN};
use crate::domain::brigade::DdsFindingKind;

pub use crate::domain::advice::DEFAULT_WINDOW;

/// Со скольких раз замечание ДДС даёт рекомендацию. См. заголовок модуля.
pub const MIN_REPEATS: usize = 1;

/// Замечания по приёму карточки: считаются от поступления, а не от доклада.
const INTAKE: [DdsFindingKind; 3] = [
    DdsFindingKind::OpenedLate,
    DdsFindingKind::FirstRecordLate,
    DdsFindingKind::FirstRecordNoText,
];

/// Замечания по ходу работ: по этапам шкалы бригады.
const STAGES: [DdsFindingKind; 4] = [
    DdsFindingKind::Ok,
    DdsFindingKind::Late,
    DdsFindingKind::NotByFact,
    DdsFindingKind::Missed,
];

/// Одна закрытая карточка диспетчера глазами рекомендаций.
#[derive(Debug, Clone, Default)]
pub struct Past {
    /// Виды всех замечаний разбора, включая `Ok`
    pub kinds: Vec<DdsFindingKind>,
    /// Этап последнего замечания каждого вида — для текста
    pub stages: Vec<(DdsFindingKind, String)>,
    /// Приём карточки оценивался: вне блица «статусы по докладам»
    pub intake_graded: bool,
}

impl Past {
    fn has(&self, k: DdsFindingKind) -> bool {
        self.kinds.contains(&k)
    }

    fn stage_of(&self, k: DdsFindingKind) -> Option<&str> {
        self.stages.iter().find(|(kk, _)| *kk == k).map(|(_, s)| s.as_str())
    }

    /// Была ли в этой карточке возможность повторить ошибку, и если да —
    /// ушла ли она.
    ///
    /// `None` — возможности не было: в блице приём не оценивается, и
    /// отсутствие «открыта позже 30 с» ничего не доказывает; без
    /// единого оценённого этапа нечего сказать про статусы.
    pub fn outcome(&self, k: DdsFindingKind) -> Option<bool> {
        let possible = if INTAKE.contains(&k) {
            self.intake_graded
        } else if STAGES.contains(&k) {
            self.kinds.iter().any(|x| STAGES.contains(x))
        } else {
            // Бригада не назначена: попытка ДДС — это назначение бригады,
            // и в истории такое замечание не встречается
            return None;
        };
        possible.then(|| !self.has(k))
    }
}

/// Повторяющаяся ошибка, на которую пора выдать рекомендацию.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub kind: DdsFindingKind,
    pub stage: String,
    /// Сколько раз встретилась в окне
    pub occurrences: usize,
}

/// Какие ошибки есть в последнем разборе и сколько раз они встретились
/// за окно. Порядок — по частоте, потом по тяжести, потом по виду:
/// выбор воспроизводим.
pub fn triggers(history: &[Past], window: usize) -> Vec<Trigger> {
    let Some(last) = history.last() else {
        return Vec::new();
    };
    let window = window.max(1);
    let tail = &history[history.len().saturating_sub(window)..];

    let mut kinds: Vec<DdsFindingKind> = last
        .kinds
        .iter()
        .copied()
        .filter(|k| *k != DdsFindingKind::Ok)
        .collect();
    kinds.sort();
    kinds.dedup();

    let mut out: Vec<Trigger> = kinds
        .into_iter()
        .filter_map(|k| {
            let occurrences = tail.iter().filter(|p| p.has(k)).count();
            (occurrences >= MIN_REPEATS).then(|| Trigger {
                kind: k,
                stage: last.stage_of(k).unwrap_or_default().to_string(),
                occurrences,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.occurrences
            .cmp(&a.occurrences)
            .then(b.kind.weight().cmp(&a.kind.weight()))
            .then(a.kind.cmp(&b.kind))
    });
    out
}

/// Какие рекомендации выдать сейчас: открытых не больше трёх, на одну
/// ошибку одна открытая.
pub fn to_issue(history: &[Past], window: usize, open: &[DdsFindingKind]) -> Vec<Trigger> {
    let slots = MAX_OPEN.saturating_sub(open.len());
    triggers(history, window)
        .into_iter()
        .filter(|t| !open.contains(&t.kind))
        .take(slots)
        .collect()
}

/// Нормативы, которые входят в текст рекомендации. Из `settings`.
#[derive(Debug, Clone, Copy)]
pub struct Norms {
    /// `brigade.update_norm_sec`
    pub update_sec: i64,
    /// `norm.ack_sec`
    pub open_sec: i64,
    /// `norm.first_record_sec`
    pub first_record_sec: i64,
}

fn срок(sec: i64) -> String {
    if sec >= 60 && sec % 60 == 0 {
        format!("{} мин", sec / 60)
    } else {
        format!("{sec} с")
    }
}

/// Короткое имя замечания — как в разборе на экране.
pub fn label(k: DdsFindingKind) -> &'static str {
    match k {
        DdsFindingKind::Ok => "по факту",
        DdsFindingKind::Late => "статус с опозданием",
        DdsFindingKind::NotByFact => "статус не по факту",
        DdsFindingKind::Missed => "статус не проставлен",
        DdsFindingKind::NoBrigade => "бригада не назначена",
        DdsFindingKind::OpenedLate => "карточка открыта поздно",
        DdsFindingKind::FirstRecordLate => "первая запись поздно",
        DdsFindingKind::FirstRecordNoText => "первая запись без текста",
    }
}

/// Формулировка: что тренировать дальше.
///
/// Только норматив и последствие, которые разбор уже назвал в
/// замечании; никаких фактов о конкретной карточке.
pub fn wording(k: DdsFindingKind, n: &Norms) -> String {
    match k {
        DdsFindingKind::Ok => "Замечаний нет — продолжайте так же".into(),
        DdsFindingKind::Late => format!(
            "Обновляйте статус в течение {} после доклада старшего: пока он не обновлён, \
             в карточке устаревшая картина хода работ",
            срок(n.update_sec)
        ),
        DdsFindingKind::NotByFact => "Проставляйте статус по факту получения сведений об этапе — \
             после доклада старшего или вашего звонка ему, не раньше: иначе служба 112 и \
             соседние ДДС видят этап, которого, возможно, ещё нет"
            .into(),
        DdsFindingKind::Missed => "Получив доклад старшего, проставляйте статус этапа: без него \
             служба 112 и соседние ДДС не видят хода работ"
            .into(),
        DdsFindingKind::NoBrigade => "После статуса «Принята» направляйте бригаду: пока её нет, \
             на место никто не выехал"
            .into(),
        DdsFindingKind::OpenedLate => format!(
            "Открывайте карточку в течение {} с её поступления в список: пока она не открыта, \
             служба не знает о происшествии",
            срок(n.open_sec)
        ),
        DdsFindingKind::FirstRecordLate => format!(
            "Делайте первую запись — статус и текст — в течение {} с поступления карточки: \
             до неё служба 112 не видит, взята ли карточка в работу",
            срок(n.first_record_sec)
        ),
        DdsFindingKind::FirstRecordNoText => "Первая запись — статус и текст: из одного статуса \
             не видно, что служба делает"
            .into(),
    }
}

/// Проверка рекомендации: первая карточка, закрытая после неё, где у
/// ошибки была возможность повториться. Проверяем по первой
/// возможности, а не «хоть раз получилось».
pub fn check(k: DdsFindingKind, after: &[Past]) -> Option<(usize, bool)> {
    after
        .iter()
        .enumerate()
        .find_map(|(i, p)| p.outcome(k).map(|ok| (i, ok)))
}

/// Базовая линия: как часто ошибка уходит сама, без рекомендации.
/// `covered(i, kind)` — была ли на эту ошибку открыта рекомендация
/// в момент разбора `i`.
pub fn baseline<F>(history: &[Past], window: usize, covered: F) -> (usize, usize)
where
    F: Fn(usize, DdsFindingKind) -> bool,
{
    let mut checked = 0usize;
    let mut worked = 0usize;
    for i in 0..history.len() {
        for t in triggers(&history[..=i], window) {
            if covered(i, t.kind) {
                continue;
            }
            if let Some((_, ok)) = check(t.kind, &history[i + 1..]) {
                checked += 1;
                worked += ok as usize;
            }
        }
    }
    (checked, worked)
}

pub fn summary(checked: usize, worked: usize, base_checked: usize, base_worked: usize) -> Efficacy {
    efficacy(checked, worked, base_checked, base_worked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use DdsFindingKind::*;

    fn разбор(виды: &[DdsFindingKind]) -> Past {
        Past {
            kinds: виды.to_vec(),
            stages: виды.iter().map(|k| (*k, "Прибытие".to_string())).collect(),
            intake_graded: true,
        }
    }

    fn нормы() -> Norms {
        Norms { update_sec: 30, open_sec: 30, first_record_sec: 180 }
    }

    /// В отличие от оператора 112, замечание ДДС даёт рекомендацию
    /// с первого раза: это нарушение памятки, а не догадка.
    #[test]
    fn замечание_ддс_даёт_рекомендацию_с_первого_раза() {
        let т = triggers(&[разбор(&[Ok, NotByFact])], 5);
        assert_eq!(т.len(), 1);
        assert_eq!(т[0].kind, NotByFact);
        assert_eq!(т[0].occurrences, 1);
    }

    #[test]
    fn исправленная_к_последней_карточке_не_рекомендуется() {
        let история = vec![разбор(&[Late]), разбор(&[Ok, Ok])];
        assert!(triggers(&история, 5).is_empty());
    }

    #[test]
    fn частые_и_тяжёлые_впереди() {
        let история = vec![разбор(&[Late, FirstRecordNoText]), разбор(&[Late, Missed, FirstRecordNoText])];
        let порядок: Vec<DdsFindingKind> = triggers(&история, 5).into_iter().map(|t| t.kind).collect();
        // Late и «без текста» встречались дважды; Late тяжелее? Нет — оба весят 1,
        // тогда по порядку видов. Missed — один раз, в конце
        assert_eq!(порядок, vec![Late, FirstRecordNoText, Missed]);
    }

    #[test]
    fn открытых_не_больше_трёх_и_без_повторов() {
        let много = разбор(&[Late, NotByFact, Missed, OpenedLate]);
        assert_eq!(to_issue(&[много.clone()], 5, &[]).len(), MAX_OPEN);
        let ещё = to_issue(&[много], 5, &[NotByFact]);
        assert_eq!(ещё.len(), MAX_OPEN - 1);
        assert!(ещё.iter().all(|t| t.kind != NotByFact));
    }

    /// В блице приём не оценивается: отсутствие «открыта поздно» там
    /// ничего не доказывает.
    #[test]
    fn без_возможности_повторить_не_проверена() {
        let блиц = Past { intake_graded: false, ..разбор(&[Ok]) };
        assert_eq!(блиц.outcome(OpenedLate), None);
        assert_eq!(блиц.outcome(Late), Some(true));
        let без_этапов = разбор(&[FirstRecordNoText]);
        assert_eq!(без_этапов.outcome(NotByFact), None, "ни одного этапа не оценено");
        assert_eq!(без_этапов.outcome(FirstRecordNoText), Some(false));
    }

    #[test]
    fn проверка_берёт_первую_возможность() {
        let после = vec![
            Past { intake_graded: false, ..разбор(&[Ok]) }, // блиц: приём не оценён
            разбор(&[OpenedLate, Ok]),                       // первая возможность — не вышло
            разбор(&[Ok]),                                   // позже получилось, но поздно
        ];
        assert_eq!(check(OpenedLate, &после), Some((1, false)));
    }

    #[test]
    fn формулировка_называет_норматив_и_не_выдумывает_фактов() {
        let т = wording(FirstRecordLate, &нормы());
        assert!(т.contains("3 мин"), "{т}");
        assert!(wording(Late, &нормы()).contains("30 с"));
        assert!(wording(NotByFact, &нормы()).contains("по факту"));
    }

    #[test]
    fn базовая_линия_исключает_закрытые_рекомендацией_случаи() {
        let история = vec![разбор(&[Late]), разбор(&[Ok])];
        assert_eq!(baseline(&история, 5, |_, _| false), (1, 1));
        assert_eq!(baseline(&история, 5, |_, _| true), (0, 0));
    }
}
