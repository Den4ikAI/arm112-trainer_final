//! Разбор бумажных билетов.
//!
//! Источник — «Билеты-задачи по С112», 32 билета по три вызова. Скан
//! распознан в Markdown-таблицы: номер, ситуация, адрес.
//!
//! Разбираем как есть, ничего не досочиняя. Из ситуации вытаскиваем
//! только то, что там написано буквально: телефон по образцу, ФИО —
//! идущие перед ним заглавные слова. Всё остальное остаётся текстом
//! и уходит модели как вводная.
//!
//! Отдельно выделяем скрытую часть адреса. Авторы билетов печатают её
//! в скобках: заявитель называет ориентир, а настоящий адрес оператор
//! обязан выяснить. Это и есть механика «узнал или угадал», заложенная
//! до нас.

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketCall {
    pub ticket: i32,
    pub call_no: i32,
    /// Описание происшествия дословно из билета
    pub situation: String,
    /// Адрес, который заявитель называет сам
    pub address: String,
    /// Адрес, который оператор должен выяснить. В билете — в скобках
    pub address_hidden: Option<String>,
    pub applicant_name: Option<String>,
    pub applicant_phone: Option<String>,
}

/// Телефон в билетах пишут по-разному: `916-126-34-71`, `9163201283`,
/// `916 896 3254`. Ловим все три вида, не приводя к одному: в карточке
/// оператор вводит то, что услышал.
fn find_phone(text: &str) -> Option<(String, usize)> {
    let bytes: Vec<char> = text.chars().collect();
    let mut best: Option<(String, usize)> = None;

    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut digits = 0;
        let mut j = i;
        while j < bytes.len() {
            let c = bytes[j];
            if c.is_ascii_digit() {
                digits += 1;
                j += 1;
            } else if (c == '-' || c == ' ') && digits > 0 {
                // Разделитель считается частью номера, только если
                // за ним снова цифра
                if bytes.get(j + 1).is_some_and(|n| n.is_ascii_digit()) {
                    j += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if digits >= 10 {
            let raw: String = bytes[start..j].iter().collect();
            best = Some((raw.trim().to_string(), start));
            break;
        }
        i = j.max(i + 1);
    }

    best
}

/// ФИО — заглавные слова непосредственно перед телефоном.
///
/// В билетах это устойчиво: «…пострадавших нет, Иванова Инна
/// Степановна, 916-126-34-71». Берём не больше трёх слов и требуем
/// минимум два: одна заглавная перед номером — это чаще конец
/// предыдущей фразы, чем фамилия.
fn find_name(text: &str, phone_at: usize) -> Option<String> {
    let head: String = text.chars().take(phone_at).collect();
    let head = head.trim_end_matches([',', ' ', '\u{00a0}']);

    let mut words: Vec<&str> = Vec::new();
    for word in head.split_whitespace().rev() {
        let word = word.trim_matches(|c: char| c == ',' || c == '.' || c == '(' || c == ')');
        let first = word.chars().next()?;
        if !first.is_uppercase() || word.chars().count() < 2 {
            break;
        }
        words.push(word);
        if words.len() == 3 {
            break;
        }
    }

    if words.len() < 2 {
        return None;
    }
    words.reverse();
    Some(words.join(" "))
}

/// Скрытая часть адреса. В распознанном тексте она в скобках, иногда
/// ещё и курсивом: `*(при уточнении адреса – г. Королёв, МО)*`.
fn split_address(raw: &str) -> (String, Option<String>) {
    let cleaned = raw.replace('*', "");
    let Some(open) = cleaned.rfind('(') else {
        return (cleaned.trim().to_string(), None);
    };
    let Some(close) = cleaned[open..].find(')') else {
        return (cleaned.trim().to_string(), None);
    };

    let inside = cleaned[open + 1..open + close].trim().to_string();
    let visible = cleaned[..open].trim().trim_end_matches(',').trim().to_string();

    // «точка на карте» и подобное — не адрес, а пояснение
    if inside.chars().filter(|c| c.is_alphanumeric()).count() < 4 {
        return (cleaned.trim().to_string(), None);
    }

    let hidden = inside
        .trim_start_matches("при уточнении адреса")
        .trim_start_matches(['–', '-', ':', ' '])
        .trim()
        .to_string();

    (visible, Some(hidden))
}

pub fn parse(markdown: &str) -> Result<Vec<TicketCall>> {
    let mut calls = Vec::new();
    let mut ticket: Option<i32> = None;

    for line in markdown.lines() {
        let trimmed = line.trim().trim_matches('*').trim();

        if let Some(rest) = trimmed.strip_prefix("БИЛЕТ") {
            if let Ok(n) = rest.trim().parse::<i32>() {
                ticket = Some(n);
                continue;
            }
        }

        // Строка таблицы: | 1 | ситуация | адрес |
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 3 {
            continue;
        }
        let Ok(call_no) = cells[0].parse::<i32>() else { continue };
        let Some(ticket) = ticket else { continue };

        let situation = cells[1].trim().to_string();
        let (address, address_hidden) = split_address(cells[2]);

        let (applicant_phone, applicant_name) = match find_phone(&situation) {
            Some((phone, at)) => (Some(phone), find_name(&situation, at)),
            None => (None, None),
        };

        calls.push(TicketCall {
            ticket,
            call_no,
            situation,
            address,
            address_hidden,
            applicant_name,
            applicant_phone,
        });
    }

    Ok(calls)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
БИЛЕТ 3

| № | Ситуация | Адрес |
|---|----------|-------|
| 1 | Горит крыша частного дома, пострадавших нет, дом не газифицирован, Иванова Инна Степановна, 916-126-34-71 | ул. Станционная, дом 28 *(при уточнении адреса – г. Королёв, МО)* |
| 2 | Громко играет музыка во дворе, Соколов Иван Петрович, 916 896 3254 | Бульвар Маршала Рокоссовского дом 25, двор |
";

    #[test]
    fn разбирает_строку_билета() {
        let calls = parse(SAMPLE).unwrap();
        assert_eq!(calls.len(), 2);

        let first = &calls[0];
        assert_eq!(first.ticket, 3);
        assert_eq!(first.call_no, 1);
        assert!(first.situation.starts_with("Горит крыша частного дома"));
        assert_eq!(first.applicant_name.as_deref(), Some("Иванова Инна Степановна"));
        assert_eq!(first.applicant_phone.as_deref(), Some("916-126-34-71"));
    }

    /// Настоящий адрес напечатан в скобках: заявитель называет
    /// ориентир, оператор обязан выяснить остальное.
    #[test]
    fn скрытая_часть_адреса_отделяется() {
        let calls = parse(SAMPLE).unwrap();
        assert_eq!(calls[0].address, "ул. Станционная, дом 28");
        assert_eq!(calls[0].address_hidden.as_deref(), Some("г. Королёв, МО"));
    }

    #[test]
    fn адрес_без_скобок_остаётся_целым() {
        let calls = parse(SAMPLE).unwrap();
        assert_eq!(calls[1].address, "Бульвар Маршала Рокоссовского дом 25, двор");
        assert!(calls[1].address_hidden.is_none());
    }

    /// Телефоны в билетах записаны тремя разными способами.
    #[test]
    fn телефон_ловится_в_любом_виде() {
        for (text, expected) in [
            ("Иванов Иван Иванович, 916-126-34-71", "916-126-34-71"),
            ("Вызывает мама, 9163201283", "9163201283"),
            ("Соколов Иван Петрович, 916 896 3254", "916 896 3254"),
        ] {
            let (phone, _) = find_phone(text).expect(text);
            assert_eq!(phone, expected);
        }
    }

    /// Номер дома за телефон не принимаем.
    #[test]
    fn короткие_числа_не_телефон() {
        assert!(find_phone("дом 28, подъезд 3, домофон 68").is_none());
    }

    /// Одно заглавное слово перед номером — это обычно конец фразы,
    /// а не фамилия.
    #[test]
    fn одно_слово_за_фио_не_считается() {
        let (_, at) = find_phone("Вызывает мама, 9163201283").unwrap();
        assert_eq!(find_name("Вызывает мама, 9163201283", at), None);
    }
}
