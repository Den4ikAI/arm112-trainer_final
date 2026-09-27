//! Бригада диспетчера ДДС: шкала работ, доклады старшего и разбор.
//!
//! Устройство взято из ответов заказчика в чате задачи (21.09.2026,
//! `refs/чат-ответы-заказчика-20-22.09.txt`):
//!
//! > Для ДДС необходимые бригады выбираются в ручную.
//!
//! > Старший группы звонит в ДДС докладывает об обстановке и ходе
//! > работ. Или диспетчер сам через определённое время набирает по
//! > телефону старшего. И уточняет ход работ. Работают как правило
//! > два варианта.
//!
//! > Специфики нет. <…> Пусть будет универсальный алгоритм для всех.
//!
//! Отсюда один алгоритм на все службы. Этап работ наступает по шкале
//! (`due_at`) — это скрытая истина, как факты заявителя у оператора 112.
//! Старший докладывает сам с задержкой; диспетчер, который звонит ему
//! раньше, узнаёт раньше. Так в одной модели живут оба варианта из
//! ответа заказчика, и у проактивного диспетчера есть преимущество.
//!
//! Памятка ДДС требует проставлять статусы по факту получения
//! информации о соответствующем этапе. Разбор проверяет ровно это:
//! статус поставлен после того, как диспетчер узнал об этапе, и не
//! слишком поздно. Статус раньше доклада — аналог «угадал» у оператора
//! 112: значение, может, и верное, но не выясненное.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Этапы работ бригады, по порядку. Это статусы реагирования из
/// памятки ДДС, которые диспетчер ставит по докладам старшего.
pub const STAGES: [&str; 4] =
    ["Начало реагирования", "Прибытие", "Проведение работ", "Работы завершены"];

/// Статус, после которого бригада не работает, и докладов не ждут.
const REFUSAL: &str = "Отказ от выполнения работ";

/// Параметры шкалы, секунды. Из таблицы `settings`.
#[derive(Debug, Clone, Copy)]
pub struct Timeline {
    /// От назначения до выезда
    pub start_sec: i64,
    /// От выезда до прибытия
    pub arrive_sec: i64,
    /// От прибытия до начала работ
    pub work_start_sec: i64,
    /// От начала работ до завершения
    pub work_sec: i64,
}

impl Timeline {
    /// Смещения этапов от момента назначения, по порядку `STAGES`.
    pub fn offsets(&self) -> [i64; 4] {
        let start = self.start_sec;
        let arrive = start + self.arrive_sec;
        let work = arrive + self.work_start_sec;
        let done = work + self.work_sec;
        [start, arrive, work, done]
    }
}

/// Текст доклада старшего.
///
/// Нарочно без подробностей обстановки: их у нас нет, а выдумывать
/// обстановку на месте значит подсовывать курсанту несуществующие факты.
/// Доклад сообщает ровно этап — то, по чему диспетчер ставит статус.
pub fn report_text(stage: &str, brigade_no: i32) -> String {
    let what = match stage {
        "Начало реагирования" => "выехали на вызов",
        "Прибытие" => "прибыли на место",
        "Проведение работ" => "приступили к работам",
        "Работы завершены" => "работы завершены",
        other => other,
    };
    format!("Старший бригады {brigade_no}: {what}.")
}

/// Ответ старшего на звонок диспетчера, когда нового этапа нет.
pub fn no_change_text(brigade_no: i32) -> String {
    format!("Старший бригады {brigade_no}: пока без изменений.")
}

/// Этап работ глазами разбора.
#[derive(Debug, Clone)]
pub struct Report {
    pub stage: String,
    pub due_at: DateTime<Utc>,
    /// Когда диспетчер узнал об этапе. `None` — ещё не узнал
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Статус, проставленный службой, с временем.
#[derive(Debug, Clone)]
pub struct Status {
    pub status: String,
    pub at: DateTime<Utc>,
}

/// Первая запись диспетчера по карточке: статус и был ли при нём текст.
#[derive(Debug, Clone)]
pub struct FirstRecord {
    pub status: String,
    pub at: DateTime<Utc>,
    pub with_text: bool,
}

/// Приём карточки: открытие и первая запись.
///
/// Нормативы — из ответа заказчика в чате задачи 27.09.2026:
///
/// > 30 сек при поступлении карточки в систему т.е. с момента ее
/// > добавления в строку сообщений. 3 минуты на открытие карточки и
/// > добавления первой записи. Статуса и текста.
///
/// Оба срока считаются от поступления карточки в список.
#[derive(Debug, Clone)]
pub struct Intake {
    /// Когда карточка появилась в списке ДДС
    pub arrived_at: DateTime<Utc>,
    /// Когда диспетчер открыл её — запись «Получена службой»
    pub opened_at: Option<DateTime<Utc>>,
    pub first_record: Option<FirstRecord>,
    pub open_norm_sec: i64,
    pub first_record_norm_sec: i64,
}

/// Всё, что нужно разбору одной службы по одной карточке.
#[derive(Debug, Clone)]
pub struct Input {
    /// Когда служба приняла карточку — первое «Принята»
    pub accepted_at: Option<DateTime<Utc>>,
    /// Когда назначена бригада
    pub assigned_at: Option<DateTime<Utc>>,
    pub reports: Vec<Report>,
    pub statuses: Vec<Status>,
    /// Норматив обновления статуса после доклада, секунды
    pub update_norm_sec: i64,
    /// Приём карточки. `None` — не оцениваем (карточка ещё не
    /// поступила в список)
    pub intake: Option<Intake>,
    pub now: DateTime<Utc>,
}

/// Имена с приставкой `Dds`: схемы OpenAPI различаются по имени типа,
/// и одноимённые `Grade` и `Finding` затёрли бы разбор оператора 112.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DdsFindingKind {
    /// Статус проставлен вовремя и после доклада
    Ok,
    /// Статус проставлен позже норматива
    Late,
    /// Статус проставлен раньше, чем диспетчер узнал об этапе
    NotByFact,
    /// Доклад получен, статус так и не проставлен
    Missed,
    /// Карточка принята, бригада не назначена
    NoBrigade,
    /// Карточка открыта позже норматива или не открыта вовсе
    OpenedLate,
    /// Первая запись сделана позже норматива или не сделана вовсе
    FirstRecordLate,
    /// Первая запись — статус без текста
    FirstRecordNoText,
}

impl DdsFindingKind {
    /// Веса — по той же шкале, что у оператора 112: угаданное и лишнее —
    /// 1, пропущенное — 2, неверное — 3. Статус не по факту весит как
    /// пропуск: в карточке видят этап, которого, может быть, ещё нет,
    /// и соседние службы строят на этом свои действия.
    fn weight(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Late | Self::OpenedLate | Self::FirstRecordLate | Self::FirstRecordNoText => 1,
            Self::NotByFact | Self::Missed | Self::NoBrigade => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DdsFinding {
    pub kind: DdsFindingKind,
    /// Этап работ, к которому относится замечание. У «бригада не
    /// назначена» этапа нет — там «Принята»
    pub stage: String,
    /// Когда диспетчер узнал об этапе
    pub known_at: Option<DateTime<Utc>>,
    /// Когда проставлен статус
    pub status_at: Option<DateTime<Utc>>,
    /// Насколько позже доклада проставлен статус, секунды. Отрицательное —
    /// раньше доклада
    pub lag_sec: Option<i64>,
    /// Цена ошибки словами, как в разборе оператора 112
    pub consequence: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DdsGrade {
    pub findings: Vec<DdsFinding>,
    /// Балл 0..100. `None`, пока оценивать нечего: бригада не назначена,
    /// а норматив на назначение ещё не истёк
    pub score: Option<i32>,
    /// Этапов, по которым решение ещё не наступило: доклада не было,
    /// или норматив после доклада не истёк
    pub pending: usize,
}

fn consequence(kind: DdsFindingKind, lag: Option<i64>) -> Option<String> {
    Some(match kind {
        DdsFindingKind::Ok => return None,
        DdsFindingKind::Late => format!(
            "Статус обновлён через {} с после доклада — всё это время в карточке \
             была устаревшая картина хода работ.",
            lag.unwrap_or_default()
        ),
        DdsFindingKind::NotByFact => "Статус проставлен раньше, чем пришли сведения об этапе: \
             не по факту. Служба 112 и соседние ДДС видят этап, которого, \
             возможно, ещё нет."
            .into(),
        DdsFindingKind::Missed => "Доклад получен, а статус не проставлен: служба 112 и соседние \
             ДДС не видят хода работ."
            .into(),
        DdsFindingKind::NoBrigade => {
            "Карточка принята, но бригада не назначена — на место никто не выехал.".into()
        }
        DdsFindingKind::OpenedLate => match lag {
            Some(lag) => format!(
                "Карточка открыта через {lag} с после поступления: всё это время \
                 служба не знала о происшествии."
            ),
            None => "Карточка так и не открыта: служба не знает о происшествии.".into(),
        },
        DdsFindingKind::FirstRecordLate => match lag {
            Some(lag) => format!(
                "Первая запись сделана через {lag} с после поступления: служба 112 \
                 всё это время не видела, взята ли карточка в работу."
            ),
            None => "Первой записи нет: служба 112 не видит, взята ли карточка в работу.".into(),
        },
        DdsFindingKind::FirstRecordNoText => "Первая запись — статус без текста. Нужны статус \
             и текст: из одного статуса не видно, что служба делает."
            .into(),
    })
}

/// Разбор приёма карточки: открытие и первая запись.
///
/// Пока норматив не истёк и события нет — судить рано, это `pending`.
/// Уложился в норматив — замечания нет: хорошее открытие не должно
/// разбавлять балл за ход работ.
fn grade_intake(i: &Intake, now: DateTime<Utc>) -> (Vec<DdsFinding>, usize) {
    let mut findings = Vec::new();
    let mut pending = 0usize;
    let since = |at: DateTime<Utc>| (at - i.arrived_at).num_seconds();
    let waited = (now - i.arrived_at).num_seconds();

    match i.opened_at {
        Some(at) if since(at) > i.open_norm_sec => findings.push(DdsFinding {
            kind: DdsFindingKind::OpenedLate,
            stage: "Получена службой".into(),
            known_at: Some(i.arrived_at),
            status_at: Some(at),
            lag_sec: Some(since(at)),
            consequence: consequence(DdsFindingKind::OpenedLate, Some(since(at))),
        }),
        Some(_) => {}
        None if waited > i.open_norm_sec => findings.push(DdsFinding {
            kind: DdsFindingKind::OpenedLate,
            stage: "Получена службой".into(),
            known_at: Some(i.arrived_at),
            status_at: None,
            lag_sec: None,
            consequence: consequence(DdsFindingKind::OpenedLate, None),
        }),
        None => pending += 1,
    }

    match &i.first_record {
        Some(r) => {
            if since(r.at) > i.first_record_norm_sec {
                findings.push(DdsFinding {
                    kind: DdsFindingKind::FirstRecordLate,
                    stage: r.status.clone(),
                    known_at: Some(i.arrived_at),
                    status_at: Some(r.at),
                    lag_sec: Some(since(r.at)),
                    consequence: consequence(DdsFindingKind::FirstRecordLate, Some(since(r.at))),
                });
            }
            if !r.with_text {
                findings.push(DdsFinding {
                    kind: DdsFindingKind::FirstRecordNoText,
                    stage: r.status.clone(),
                    known_at: None,
                    status_at: Some(r.at),
                    lag_sec: None,
                    consequence: consequence(DdsFindingKind::FirstRecordNoText, None),
                });
            }
        }
        None if waited > i.first_record_norm_sec => findings.push(DdsFinding {
            kind: DdsFindingKind::FirstRecordLate,
            stage: "Принята".into(),
            known_at: Some(i.arrived_at),
            status_at: None,
            lag_sec: None,
            consequence: consequence(DdsFindingKind::FirstRecordLate, None),
        }),
        None => pending += 1,
    }

    (findings, pending)
}

/// Разбор работы диспетчера по одной карточке своей службы.
pub fn grade(input: &Input) -> DdsGrade {
    let norm = input.update_norm_sec;
    let (mut findings, mut pending) = match &input.intake {
        Some(i) => grade_intake(i, input.now),
        None => (Vec::new(), 0),
    };

    // Принял и не назначил бригаду: единственное, что можно сказать,
    // пока бригады нет. Ход работ без бригады не оценивается
    if input.assigned_at.is_none() {
        if let Some(accepted) = input.accepted_at {
            if (input.now - accepted).num_seconds() > norm {
                findings.push(DdsFinding {
                    kind: DdsFindingKind::NoBrigade,
                    stage: "Принята".into(),
                    known_at: Some(accepted),
                    status_at: None,
                    lag_sec: None,
                    consequence: consequence(DdsFindingKind::NoBrigade, None),
                });
            }
        }
        return finish(findings, pending);
    }

    // Как далеко по ходу работ продвинулась служба в карточке. Статусы
    // идут только вперёд и через этап перепрыгнуть можно: узнав по
    // звонку сразу о прибытии, диспетчер ставит «Прибытие», и
    // «Начало реагирования» после этого уже не поставить. Такой этап
    // пропуском не считается
    let reached = |stage: &str| -> Option<usize> { STAGES.iter().position(|s| *s == stage) };
    let furthest = input
        .statuses
        .iter()
        .filter_map(|s| reached(&s.status))
        .max();
    let refused = input.statuses.iter().any(|s| s.status == REFUSAL);

    for (i, stage) in STAGES.iter().enumerate() {
        let report = input.reports.iter().find(|r| r.stage == *stage);
        let known_at = report.and_then(|r| r.delivered_at);
        let status_at = input
            .statuses
            .iter()
            .find(|s| s.status == *stage)
            .map(|s| s.at);

        match (known_at, status_at) {
            (Some(k), Some(t)) => {
                let lag = (t - k).num_seconds();
                let kind = if lag < 0 {
                    DdsFindingKind::NotByFact
                } else if lag > norm {
                    DdsFindingKind::Late
                } else {
                    DdsFindingKind::Ok
                };
                findings.push(DdsFinding {
                    kind,
                    stage: stage.to_string(),
                    known_at: Some(k),
                    status_at: Some(t),
                    lag_sec: Some(lag),
                    consequence: consequence(kind, Some(lag)),
                });
            }
            // Статус есть, а сведений об этапе у диспетчера не было вовсе
            (None, Some(t)) => findings.push(DdsFinding {
                kind: DdsFindingKind::NotByFact,
                stage: stage.to_string(),
                known_at: None,
                status_at: Some(t),
                lag_sec: None,
                consequence: consequence(DdsFindingKind::NotByFact, None),
            }),
            (Some(k), None) => {
                // Этап перекрыт более поздним статусом — не пропуск
                let covered = furthest.is_some_and(|f| f > i);
                if covered || refused {
                    continue;
                }
                if (input.now - k).num_seconds() > norm {
                    findings.push(DdsFinding {
                        kind: DdsFindingKind::Missed,
                        stage: stage.to_string(),
                        known_at: Some(k),
                        status_at: None,
                        lag_sec: None,
                        consequence: consequence(DdsFindingKind::Missed, None),
                    });
                } else {
                    pending += 1;
                }
            }
            // Ничего не известно и ничего не поставлено — делать было нечего
            (None, None) => pending += 1,
        }
    }

    finish(findings, pending)
}

fn finish(findings: Vec<DdsFinding>, pending: usize) -> DdsGrade {
    if findings.is_empty() {
        return DdsGrade { findings, score: None, pending };
    }
    // Шкала та же, что у оператора 112: доля набранного от максимального
    // штрафа, +1 к знаменателю — сама бригада, чтобы одна ошибка на
    // первом этапе не обнуляла балл
    let penalty: i32 = findings.iter().map(|f| f.kind.weight()).sum();
    let total = findings.len() as i32 + 1;
    let score = (100.0 * (1.0 - penalty as f64 / (total * 3) as f64)).round() as i32;
    DdsGrade { findings, score: Some(score.clamp(0, 100)), pending }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(sec: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + sec, 0).unwrap()
    }

    fn доклад(stage: &str, due: i64, delivered: Option<i64>) -> Report {
        Report { stage: stage.into(), due_at: t(due), delivered_at: delivered.map(t) }
    }

    fn статус(status: &str, at: i64) -> Status {
        Status { status: status.into(), at: t(at) }
    }

    fn вход(reports: Vec<Report>, statuses: Vec<Status>, now: i64) -> Input {
        Input {
            accepted_at: Some(t(0)),
            assigned_at: Some(t(5)),
            reports,
            statuses,
            update_norm_sec: 30,
            intake: None,
            now: t(now),
        }
    }

    fn приём(opened: Option<i64>, first: Option<(i64, bool)>) -> Intake {
        Intake {
            arrived_at: t(0),
            opened_at: opened.map(t),
            first_record: first.map(|(at, with_text)| FirstRecord {
                status: "Принята".into(),
                at: t(at),
                with_text,
            }),
            open_norm_sec: 30,
            first_record_norm_sec: 180,
        }
    }

    fn виды(g: &DdsGrade) -> Vec<DdsFindingKind> {
        g.findings.iter().map(|f| f.kind).collect()
    }

    /// Ответ заказчика 27.09: открыл за 30 секунд, за 3 минуты сделал
    /// первую запись со статусом и текстом — замечаний нет.
    #[test]
    fn приём_в_нормативах_без_замечаний() {
        let mut вх = вход(vec![], vec![], 10);
        вх.assigned_at = None;
        вх.accepted_at = None;
        вх.intake = Some(приём(Some(10), Some((60, true))));
        вх.now = t(200);
        let итог = grade(&вх);
        assert!(итог.findings.is_empty());
        assert_eq!(итог.score, None);
    }

    #[test]
    fn открыл_позже_тридцати_секунд_замечание() {
        let mut вх = вход(vec![], vec![], 100);
        вх.assigned_at = None;
        вх.accepted_at = None;
        вх.intake = Some(приём(Some(45), Some((60, true))));
        let итог = grade(&вх);
        assert_eq!(виды(&итог), vec![DdsFindingKind::OpenedLate]);
        assert_eq!(итог.findings[0].lag_sec, Some(45));
    }

    #[test]
    fn первая_запись_позже_трёх_минут_и_без_текста() {
        let mut вх = вход(vec![], vec![], 300);
        вх.assigned_at = None;
        вх.accepted_at = None;
        вх.intake = Some(приём(Some(5), Some((200, false))));
        let итог = grade(&вх);
        assert_eq!(
            виды(&итог),
            vec![DdsFindingKind::FirstRecordLate, DdsFindingKind::FirstRecordNoText]
        );
    }

    /// Пока нормативы не истекли, отсутствие открытия и записи — не
    /// нарушение, а ожидание.
    #[test]
    fn до_истечения_нормативов_приём_ждёт() {
        let mut вх = вход(vec![], vec![], 20);
        вх.assigned_at = None;
        вх.accepted_at = None;
        вх.intake = Some(приём(None, None));
        let итог = grade(&вх);
        assert!(итог.findings.is_empty());
        assert_eq!(итог.pending, 2);
    }

    #[test]
    fn не_открыл_и_не_записал_после_нормативов() {
        let mut вх = вход(vec![], vec![], 200);
        вх.assigned_at = None;
        вх.accepted_at = None;
        вх.intake = Some(приём(None, None));
        let итог = grade(&вх);
        assert_eq!(
            виды(&итог),
            vec![DdsFindingKind::OpenedLate, DdsFindingKind::FirstRecordLate]
        );
        assert!(итог.findings.iter().all(|f| f.status_at.is_none()));
    }

    #[test]
    fn шкала_складывает_этапы_по_порядку() {
        let шкала = Timeline { start_sec: 20, arrive_sec: 40, work_start_sec: 10, work_sec: 60 };
        assert_eq!(шкала.offsets(), [20, 60, 70, 130]);
    }

    #[test]
    fn статус_после_доклада_в_нормативе_засчитывается() {
        let итог = grade(&вход(
            vec![доклад("Начало реагирования", 20, Some(40))],
            vec![статус("Принята", 1), статус("Начало реагирования", 50)],
            60,
        ));
        let з = &итог.findings[0];
        assert_eq!(з.kind, DdsFindingKind::Ok);
        assert_eq!(з.lag_sec, Some(10));
        assert_eq!(итог.score, Some(100));
    }

    /// Главное: статус раньше доклада — не по факту, даже если этап
    /// к тому моменту действительно наступил.
    #[test]
    fn статус_раньше_доклада_не_по_факту() {
        let итог = grade(&вход(
            vec![доклад("Начало реагирования", 20, Some(45))],
            vec![статус("Начало реагирования", 30)],
            60,
        ));
        assert_eq!(итог.findings[0].kind, DdsFindingKind::NotByFact,
                   "этап наступил в 20, но диспетчер узнал в 45 — ставить в 30 было не по факту");
        assert!(итог.score.unwrap() < 100);
    }

    #[test]
    fn статус_без_всяких_сведений_не_по_факту() {
        let итог = grade(&вход(
            vec![доклад("Прибытие", 100, None)],
            vec![статус("Прибытие", 30)],
            60,
        ));
        let з = итог.findings.iter().find(|f| f.stage == "Прибытие").unwrap();
        assert_eq!(з.kind, DdsFindingKind::NotByFact);
        assert!(з.known_at.is_none());
    }

    #[test]
    fn поздний_статус_отмечается_с_задержкой() {
        let итог = grade(&вход(
            vec![доклад("Прибытие", 40, Some(40))],
            vec![статус("Прибытие", 100)],
            120,
        ));
        let з = &итог.findings[0];
        assert_eq!(з.kind, DdsFindingKind::Late);
        assert_eq!(з.lag_sec, Some(60));
        assert!(з.consequence.as_deref().unwrap().contains("60"));
    }

    #[test]
    fn доклад_без_статуса_после_норматива_пропуск() {
        let итог = grade(&вход(vec![доклад("Прибытие", 40, Some(40))], vec![], 100));
        assert_eq!(итог.findings[0].kind, DdsFindingKind::Missed);
    }

    /// Пока норматив после доклада не истёк, судить рано.
    #[test]
    fn доклад_без_статуса_в_нормативе_ждёт() {
        let итог = grade(&вход(vec![доклад("Прибытие", 40, Some(40))], vec![], 50));
        assert!(итог.findings.is_empty());
        assert_eq!(итог.pending, 4);
        assert_eq!(итог.score, None, "оценивать пока нечего");
    }

    /// Узнав по звонку сразу о прибытии, диспетчер ставит «Прибытие»,
    /// и «Начало реагирования» после этого уже не поставить — это не пропуск.
    #[test]
    fn перекрытый_поздним_статусом_этап_не_пропуск() {
        let итог = grade(&вход(
            vec![
                доклад("Начало реагирования", 20, Some(60)),
                доклад("Прибытие", 50, Some(60)),
            ],
            vec![статус("Прибытие", 65)],
            200,
        ));
        assert!(итог.findings.iter().all(|f| f.kind != DdsFindingKind::Missed), "{:?}", итог.findings);
        assert_eq!(итог.score, Some(100));
    }

    #[test]
    fn принял_и_не_назначил_бригаду() {
        let мало = Input { assigned_at: None, ..вход(vec![], vec![], 20) };
        assert!(grade(&мало).findings.is_empty(), "норматив на назначение ещё не истёк");

        let много = Input { assigned_at: None, ..вход(vec![], vec![], 100) };
        let итог = grade(&много);
        assert_eq!(итог.findings[0].kind, DdsFindingKind::NoBrigade);
        assert!(итог.score.unwrap() < 100);
    }

    #[test]
    fn после_отказа_докладов_не_ждут() {
        let итог = grade(&вход(
            vec![доклад("Прибытие", 40, Some(40))],
            vec![статус("Отказ от выполнения работ", 45)],
            200,
        ));
        assert!(итог.findings.iter().all(|f| f.kind != DdsFindingKind::Missed));
    }

    #[test]
    fn текст_доклада_сообщает_только_этап() {
        assert_eq!(report_text("Прибытие", 2), "Старший бригады 2: прибыли на место.");
    }
}
