//! Генерация устойчивых идентификаторов из русских названий служб.
//!
//! Идентификатор должен быть детерминированным: повторный импорт того же
//! файла обязан дать те же id, иначе связи в базе поедут.

/// Транслитерация кириллицы. Таблица простая, задача не в красоте,
/// а в стабильности результата.
fn translit_char(c: char) -> &'static str {
    match c.to_ascii_lowercase() {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d",
        'е' => "e", 'ё' => "e", 'ж' => "zh", 'з' => "z", 'и' => "i",
        'й' => "j", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n",
        'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t",
        'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "c", 'ч' => "ch",
        'ш' => "sh", 'щ' => "sch", 'ъ' => "", 'ы' => "y", 'ь' => "",
        'э' => "e", 'ю' => "yu", 'я' => "ya",
        _ => "",
    }
}

/// Префиксы, которые не несут смысла в названии службы.
const NOISE: [&str; 2] = ["классификатор ", "классификатор."];

pub fn service_id(title: &str) -> String {
    let mut s = title.to_lowercase();
    s = s.replace('\n', " ");
    for n in NOISE {
        if let Some(rest) = s.strip_prefix(n) {
            s = rest.to_string();
        }
    }

    let mut out = String::new();
    let mut prev_dash = true; // не начинаем с дефиса

    for ch in s.chars() {
        let piece: String = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase().to_string()
        } else if ch.is_alphabetic() {
            translit_char(ch).to_string()
        } else {
            String::new()
        };

        if piece.is_empty() {
            // разделитель: схлопываем подряд идущие
            if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.push_str(&piece);
            prev_dash = false;
        }
    }

    let trimmed = out.trim_matches('-').to_string();
    // Ограничиваем длину: в базе id это ключ, слишком длинные неудобны
    if trimmed.len() > 48 {
        trimmed.chars().take(48).collect::<String>().trim_matches('-').to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn транслитерация_убирает_шумный_префикс() {
        assert_eq!(service_id("Классификатор МЧС"), "mchs");
        assert_eq!(service_id("Классификатор МОСГАЗ"), "mosgaz");
    }

    #[test]
    fn схлопывает_разделители() {
        assert_eq!(service_id("Деп. ЖКХ"), "dep-zhkh");
        assert_eq!(service_id("Мос.Без."), "mos-bez");
    }

    #[test]
    fn переносы_строк_не_ломают_идентификатор() {
        assert_eq!(service_id("ЦУКБ.БПЛА\nМинистерство обороны"),
                   "cukb-bpla-ministerstvo-oborony");
    }

    #[test]
    fn результат_детерминированный() {
        let a = service_id("ГКУ Организатор перевозок");
        let b = service_id("ГКУ Организатор перевозок");
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }
}
