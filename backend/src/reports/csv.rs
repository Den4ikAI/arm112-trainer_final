//! Выгрузка в CSV.
//!
//! Два решения, продиктованные тем, что файл откроют в Excel на
//! русской локали:
//!   * разделитель — точка с запятой. С запятой Excel сваливает
//!     всю строку в одну ячейку;
//!   * в начало файла идёт BOM, иначе кириллица читается как «Ð¿Ð¾».

/// Экранирование по RFC 4180: кавычки удваиваются, поле берётся
/// в кавычки, если содержит разделитель, кавычку или перевод строки.
pub fn field(value: &str) -> String {
    if value.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn row<'a>(cells: impl IntoIterator<Item = &'a str>) -> String {
    let line = cells.into_iter().map(field).collect::<Vec<_>>().join(";");
    // CRLF: того же ждёт Excel
    format!("{line}\r\n")
}

/// Готовый файл: BOM плюс строки.
pub fn document(lines: impl IntoIterator<Item = String>) -> String {
    let mut out = String::from("\u{feff}");
    out.extend(lines);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn поле_с_разделителем_берётся_в_кавычки() {
        assert_eq!(field("Москва; Троицк"), "\"Москва; Троицк\"");
        assert_eq!(field("Королёв"), "Королёв");
    }

    #[test]
    fn кавычка_внутри_поля_удваивается() {
        assert_eq!(field("\"пожар\""), "\"\"\"пожар\"\"\"");
    }

    #[test]
    fn перевод_строки_не_ломает_строку_таблицы() {
        assert_eq!(field("первая\nвторая"), "\"первая\nвторая\"");
    }

    /// Без BOM Excel на русской локали покажет «Ð¿Ð¾Ð¶Ð°Ñ€».
    #[test]
    fn документ_начинается_с_bom() {
        let doc = document([row(["а", "б"])]);
        assert!(doc.starts_with('\u{feff}'));
        assert!(doc.ends_with("а;б\r\n"));
    }
}
