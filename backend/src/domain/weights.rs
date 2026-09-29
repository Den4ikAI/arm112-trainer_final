//! Веса оценки: преподаватель решает, что в карточке важнее.
//!
//! Каждое замечание разбора — проверка, пройденная целиком, частично
//! или не пройденная: «угадал» и «лишняя служба» — две трети, «не
//! заполнено» — треть, «неверно» — ноль (BACKEND.md, раздел 7). Проверки
//! разложены по семи группам, у каждой группы вес 0–5. Балл — взвешенная
//! доля пройденного: сумма (вес группы × зачёт проверки) над суммой
//! весов. Группа с весом 0 не учитывается, проверка «не применимо» в
//! разбор не попадает вовсе.
//!
//! При равных весах формула в точности совпадает с прежней, поэтому
//! веса по умолчанию равные: включение весов старые баллы не сдвигает.
//! Понятность текста по умолчанию выключена — её оценивает нейросеть, а
//! итоговый балл модель без решения преподавателя не меняет (раздел 3.9
//! документации); включить её — выбор преподавателя.
//!
//! Критичная ошибка — похожая, но другая улица («Дубнинская» вместо
//! «Дубининской»): бригада уедет по реальному, но чужому адресу.
//! Заказчик на встрече приводил именно такой случай. Такая ошибка
//! ограничивает балл 40 из 100, какими бы ни были остальные.

use crate::domain::brigade::{DdsFinding, DdsFindingKind};
use crate::domain::grade::{Finding, FindingKind};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Потолок балла при критичной ошибке.
pub const CRITICAL_CAP: i32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    /// Время: 30 с на открытие, 3 мин на первую запись, норматив набора карточки
    Time,
    /// Порядок статусов и статусы по докладам бригады
    Statuses,
    /// Комментарии к статусам
    Comments,
    /// Адрес
    Address,
    /// Тип происшествия и службы
    TypeServices,
    /// Полнота карточки: признаки, пострадавшие, «сказал ↔ заполнил»
    Completeness,
    /// Понятность текста — замечания анализа ручного ввода
    Text,
}

/// Веса семи групп, 0–5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Weights {
    pub time: i32,
    pub statuses: i32,
    pub comments: i32,
    pub address: i32,
    pub type_services: i32,
    pub completeness: i32,
    pub text: i32,
}

impl Default for Weights {
    fn default() -> Self {
        Self { time: 3, statuses: 3, comments: 3, address: 3, type_services: 3, completeness: 3, text: 0 }
    }
}

/// Ключи в таблице `settings` — по группе.
pub const KEYS: [(&str, Group); 7] = [
    ("weight.time", Group::Time),
    ("weight.statuses", Group::Statuses),
    ("weight.comments", Group::Comments),
    ("weight.address", Group::Address),
    ("weight.type_services", Group::TypeServices),
    ("weight.completeness", Group::Completeness),
    ("weight.text", Group::Text),
];

impl Weights {
    pub fn of(&self, g: Group) -> i32 {
        match g {
            Group::Time => self.time,
            Group::Statuses => self.statuses,
            Group::Comments => self.comments,
            Group::Address => self.address,
            Group::TypeServices => self.type_services,
            Group::Completeness => self.completeness,
            Group::Text => self.text,
        }
    }

    fn set(&mut self, g: Group, v: i32) {
        let v = v.clamp(0, 5);
        match g {
            Group::Time => self.time = v,
            Group::Statuses => self.statuses = v,
            Group::Comments => self.comments = v,
            Group::Address => self.address = v,
            Group::TypeServices => self.type_services = v,
            Group::Completeness => self.completeness = v,
            Group::Text => self.text = v,
        }
    }

    /// Все веса в пределах 0–5.
    pub fn valid(&self) -> bool {
        KEYS.iter().all(|(_, g)| (0..=5).contains(&self.of(*g)))
    }
}

/// Сохранённые веса. Нет строки — вес по умолчанию.
pub async fn load(pool: &PgPool) -> Weights {
    let rows: Vec<(String, serde_json::Value)> =
        sqlx::query_as("SELECT key, value FROM settings WHERE key LIKE 'weight.%'")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    let mut w = Weights::default();
    for (key, value) in rows {
        if let Some((_, g)) = KEYS.iter().find(|(k, _)| *k == key) {
            if let Some(v) = value.as_f64() {
                w.set(*g, v.round() as i32);
            }
        }
    }
    w
}

/// Группа проверки разбора оператора 112 — по полю.
pub fn group_112(field: &str) -> Group {
    if field.starts_with("address.") {
        Group::Address
    } else if field == "typeIds" || field.starts_with("service.") {
        Group::TypeServices
    } else {
        Group::Completeness
    }
}

/// Зачёт проверки: 1 — пройдена, 0 — нет.
fn credit(weight: i32) -> f64 {
    1.0 - weight as f64 / 3.0
}

/// Замечание анализа текста для балла: не отклонённое преподавателем.
#[derive(Debug, Clone, Copy)]
pub struct TextChecks {
    /// Анализ готов — группа применима
    pub ready: bool,
    /// Замечаний, не отклонённых преподавателем
    pub issues: usize,
}

fn combine(checks: &[(Group, f64)], w: &Weights) -> Option<i32> {
    let (num, den) = checks.iter().fold((0.0, 0.0), |(n, d), (g, c)| {
        let wg = w.of(*g) as f64;
        (n + wg * c, d + wg)
    });
    if den <= 0.0 {
        return None;
    }
    Some(((100.0 * num / den).round() as i32).clamp(0, 100))
}

/// Похожая, но другая строка: одна-две буквы разницы.
fn similar(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().to_lowercase().replace('ё', "е");
    let (a, b) = (norm(a), norm(b));
    if a == b || a.chars().count() < 5 {
        return false;
    }
    let d = levenshtein(&a, &b);
    (1..=2).contains(&d)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Есть ли в разборе критичная ошибка.
pub fn critical_112(findings: &[Finding]) -> bool {
    findings.iter().any(|f| {
        f.kind == FindingKind::Wrong
            && f.field == "address.street"
            && matches!((&f.expected, &f.actual), (Some(e), Some(a)) if similar(e, a))
    })
}

/// Балл попытки оператора 112.
pub fn score_112(findings: &[Finding], over_norm: bool, text: TextChecks, w: &Weights) -> i32 {
    let mut checks: Vec<(Group, f64)> = findings
        .iter()
        .map(|f| (group_112(&f.field), credit(f.kind.weight())))
        .collect();
    // Норматив набора карточки — отдельная проверка группы «Время»
    checks.push((Group::Time, if over_norm { credit(2) } else { 1.0 }));
    // Тип происшествия выбран — проверка, пройденная всегда: она и
    // держит одну ошибку в короткой карточке от обнуления балла
    checks.push((Group::TypeServices, 1.0));
    if text.ready {
        checks.push((Group::Text, 1.0));
        checks.extend(std::iter::repeat_n((Group::Text, 0.0), text.issues));
    }
    let score = combine(&checks, w).unwrap_or(100);
    if critical_112(findings) { score.min(CRITICAL_CAP) } else { score }
}

/// Группа проверки разбора диспетчера ДДС — по виду замечания.
pub fn group_dds(kind: DdsFindingKind) -> Group {
    match kind {
        DdsFindingKind::OpenedLate | DdsFindingKind::FirstRecordLate | DdsFindingKind::Late => Group::Time,
        DdsFindingKind::FirstRecordNoText => Group::Comments,
        DdsFindingKind::Ok | DdsFindingKind::NotByFact | DdsFindingKind::Missed
        | DdsFindingKind::NoBrigade => Group::Statuses,
    }
}

/// Балл диспетчера ДДС. `None` — оценивать пока нечего.
pub fn score_dds(findings: &[DdsFinding], w: &Weights) -> Option<i32> {
    if findings.is_empty() {
        return None;
    }
    let mut checks: Vec<(Group, f64)> = findings
        .iter()
        .map(|f| (group_dds(f.kind), credit(f.kind.weight())))
        .collect();
    // Бригада направлена — проверка, пройденная всегда (как в прежней
    // формуле: одна ошибка на первом этапе не обнуляет балл)
    checks.push((Group::Statuses, 1.0));
    Some(combine(&checks, w).unwrap_or(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn замечание(kind: FindingKind, field: &str, e: &str, a: &str) -> Finding {
        Finding {
            kind,
            field: field.into(),
            label: field.into(),
            expected: Some(e.into()),
            actual: Some(a.into()),
            consequence: None,
        }
    }

    /// При равных весах балл — ровно прежний: 100·(1 − штраф/(3·(n+2))).
    #[test]
    fn равные_веса_дают_прежний_балл() {
        let f = vec![
            замечание(FindingKind::Wrong, "answers.a", "x", "y"),
            замечание(FindingKind::Guessed, "address.house", "5", "5"),
            замечание(FindingKind::Ok, "service.mchs", "", ""),
        ];
        let прежний = (100.0 * (1.0 - (3 + 1 + 2) as f64 / (3.0 * 5.0))).round() as i32;
        assert_eq!(score_112(&f, true, TextChecks { ready: false, issues: 0 }, &Weights::default()), прежний);
    }

    #[test]
    fn вес_группы_меняет_балл() {
        let f = vec![замечание(FindingKind::Wrong, "address.house", "5", "7")];
        let без = TextChecks { ready: false, issues: 0 };
        let обычный = score_112(&f, false, без, &Weights::default());
        let адрес_важнее = score_112(&f, false, без, &Weights { address: 5, ..Weights::default() });
        let адрес_не_важен = score_112(&f, false, без, &Weights { address: 0, ..Weights::default() });
        assert!(адрес_важнее < обычный && обычный < адрес_не_важен, "{адрес_важнее} {обычный} {адрес_не_важен}");
        assert_eq!(адрес_не_важен, 100);
    }

    #[test]
    fn похожая_улица_ограничивает_балл_сорока() {
        let f = vec![
            замечание(FindingKind::Wrong, "address.street", "Дубининская", "Дубнинская"),
            замечание(FindingKind::Ok, "answers.a", "x", "x"),
            замечание(FindingKind::Ok, "answers.b", "x", "x"),
            замечание(FindingKind::Ok, "answers.c", "x", "x"),
        ];
        let w = Weights { address: 0, ..Weights::default() };
        assert_eq!(score_112(&f, false, TextChecks { ready: false, issues: 0 }, &w), CRITICAL_CAP);
        // Совсем другая улица — ошибка, но не «похожая»
        let другая = vec![замечание(FindingKind::Wrong, "address.street", "Дубининская", "Тверская")];
        assert!(!critical_112(&другая));
    }

    #[test]
    fn нулевой_вес_текста_не_трогает_балл() {
        let f = vec![замечание(FindingKind::Ok, "answers.a", "x", "x")];
        let без = score_112(&f, false, TextChecks { ready: false, issues: 0 }, &Weights::default());
        let с_анализом = score_112(&f, false, TextChecks { ready: true, issues: 3 }, &Weights::default());
        assert_eq!(без, с_анализом);
        let учитывать = Weights { text: 5, ..Weights::default() };
        assert!(score_112(&f, false, TextChecks { ready: true, issues: 3 }, &учитывать) < без);
    }
}
