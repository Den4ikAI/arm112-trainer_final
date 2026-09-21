//! Прогноз результата курсанта и проверка самого прогноза.
//!
//! Критерий финальной экспертизы требует не столько прогноза, сколько
//! его проверки:
//!
//! > проверка прогнозирования, оценка достоверности путем сравнения
//! > реальных данных и прогнозных значений
//!
//! Поэтому устройство такое: прогноз записывается **до** того, как
//! курсант взял вызов, и задним числом не переписывается. Всё, что
//! показывается про точность, посчитано по фактически сделанным
//! прогнозам, а не заявлено.
//!
//! Модель — гребневая регрессия, а не нейросеть. Две причины.
//! Во-первых, объяснимость: на разборе надо показать, из чего сложился
//! прогноз, а не только его величину. Во-вторых, данных мало, и сложная
//! модель на них переобучится — то есть будет врать увереннее.

use serde::{Deserialize, Serialize};

/// Сколько последних разборов берём в признаки.
pub const DEPTH: usize = 5;

/// Ниже этого числа пар модель не обучаем: на трёх точках регрессия
/// описывает шум, а не курсанта.
pub const MIN_TRAINING_PAIRS: usize = 30;

/// Ниже этого числа разборов прогноз помечаем недостоверным.
pub const MIN_HISTORY: usize = 3;

/// Один разбор из истории курсанта — ровно то, что нужно признакам.
#[derive(Debug, Clone, Copy)]
pub struct Past {
    pub score: i32,
    /// Значений, совпавших с эталоном и подтверждённых разговором
    pub learned: i32,
    /// Значений, совпавших с эталоном, но не выясненных
    pub guessed: i32,
    pub over_norm: bool,
    /// Группа классификатора, к которой относился вызов
    pub group_no: Option<i32>,
}

/// Признаки, на которых строится прогноз.
///
/// Всё приведено к шкале примерно 0..1: у гребневой регрессии один
/// штраф на все коэффициенты, и смешивать баллы из сотни с долями
/// из единицы — значит штрафовать их по-разному без причины.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Features {
    /// Средний балл последних разборов, делённый на 100
    pub avg_score: f64,
    /// Куда идёт: разница свежей и старой половины, в долях от 100
    pub trend: f64,
    /// Доля угаданного среди совпавшего с эталоном
    pub guessed_share: f64,
    /// Доля разборов с превышением норматива
    pub over_norm_share: f64,
    /// Опыт, обрезанный сверху: после полусотни вызовов прирост
    /// перестаёт что-либо значить
    pub experience: f64,
    /// Средний балл на этой же группе классификатора
    pub same_topic: f64,
}

impl Features {
    pub const COUNT: usize = 6;

    fn vector(&self) -> [f64; Self::COUNT] {
        [
            self.avg_score,
            self.trend,
            self.guessed_share,
            self.over_norm_share,
            self.experience,
            self.same_topic,
        ]
    }

    /// Человеческие подписи — для объяснения прогноза на экране.
    pub const LABELS: [&'static str; Self::COUNT] = [
        "средний балл",
        "динамика",
        "доля угаданного",
        "просрочки норматива",
        "опыт",
        "балл на такой же теме",
    ];
}

/// Собрать признаки по истории. История подаётся от старых к новым.
///
/// Возвращает `None`, когда разборов слишком мало: прогноз по одному
/// занятию — это не прогноз, и честнее сказать об этом прямо.
pub fn features(history: &[Past], group_no: Option<i32>) -> Option<Features> {
    if history.len() < MIN_HISTORY {
        return None;
    }

    let tail: Vec<&Past> = history.iter().rev().take(DEPTH).collect();
    let n = tail.len() as f64;

    let avg = tail.iter().map(|p| p.score as f64).sum::<f64>() / n;

    // Динамика: свежая половина против старой. Простая разность
    // устойчивее наклона по МНК, когда точек три-пять
    let half = tail.len() / 2;
    let trend = if half == 0 {
        0.0
    } else {
        let fresh: f64 = tail[..half].iter().map(|p| p.score as f64).sum::<f64>() / half as f64;
        let older: f64 =
            tail[half..].iter().map(|p| p.score as f64).sum::<f64>() / (tail.len() - half) as f64;
        fresh - older
    };

    let matched: i32 = tail.iter().map(|p| p.learned + p.guessed).sum();
    let guessed: i32 = tail.iter().map(|p| p.guessed).sum();
    let guessed_share = if matched > 0 { guessed as f64 / matched as f64 } else { 0.0 };

    let late = tail.iter().filter(|p| p.over_norm).count() as f64;

    // Балл на той же теме: если курсант её ещё не встречал, берём
    // общий средний — это честнее, чем ноль
    let same: Vec<f64> = group_no
        .map(|g| {
            history
                .iter()
                .filter(|p| p.group_no == Some(g))
                .map(|p| p.score as f64)
                .collect()
        })
        .unwrap_or_default();
    let same_topic = if same.is_empty() {
        avg
    } else {
        same.iter().sum::<f64>() / same.len() as f64
    };

    Some(Features {
        avg_score: avg / 100.0,
        trend: trend / 100.0,
        guessed_share,
        over_norm_share: late / n,
        experience: (history.len().min(50) as f64) / 50.0,
        same_topic: same_topic / 100.0,
    })
}

/// Обученная модель: свободный член и коэффициенты при признаках.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub intercept: f64,
    pub weights: Vec<f64>,
    /// На скольких парах обучена
    pub pairs: usize,
}

impl Model {
    /// Запасной вариант, когда обучаться не на чем: предсказываем
    /// средний балл самого курсанта. Это не заглушка, а осмысленная
    /// базовая линия — модель обязана её превзойти, иначе не нужна.
    pub fn baseline() -> Self {
        let mut weights = vec![0.0; Features::COUNT];
        weights[0] = 1.0; // прогноз равен среднему баллу
        Self { intercept: 0.0, weights, pairs: 0 }
    }

    pub fn trained(&self) -> bool {
        self.pairs >= MIN_TRAINING_PAIRS
    }

    /// Прогнозный балл, 0..100.
    pub fn predict(&self, f: &Features) -> i32 {
        let x = f.vector();
        let mut y = self.intercept;
        for (i, w) in self.weights.iter().enumerate().take(x.len()) {
            y += w * x[i];
        }
        (y * 100.0).round().clamp(0.0, 100.0) as i32
    }

    /// Вклад каждого признака в прогноз, в баллах.
    ///
    /// Нужен не для красоты: прогноз без объяснения преподаватель
    /// не может ни проверить, ни оспорить.
    pub fn contributions(&self, f: &Features) -> Vec<(&'static str, i32)> {
        let x = f.vector();
        Features::LABELS
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let part = self.weights.get(i).copied().unwrap_or(0.0) * x[i];
                (*label, (part * 100.0).round() as i32)
            })
            .collect()
    }
}

const DIM: usize = Features::COUNT + 1; // +1 на свободный член

/// Обучение гребневой регрессией по нормальным уравнениям.
///
/// Гребень, а не обычный МНК: признаки коррелируют (средний балл и
/// балл по теме — почти одно и то же на малой истории), и без штрафа
/// матрица вырождается, а коэффициенты разлетаются в бессмыслицу.
pub fn fit(pairs: &[(Features, i32)], lambda: f64) -> Model {
    if pairs.len() < MIN_TRAINING_PAIRS {
        return Model::baseline();
    }

    // Нормальные уравнения: (XᵀX + λI) w = Xᵀy
    let mut a = [[0.0f64; DIM]; DIM];
    let mut b = [0.0f64; DIM];

    for (f, target) in pairs {
        let mut x = [0.0; DIM];
        x[0] = 1.0;
        x[1..].copy_from_slice(&f.vector());
        let y = *target as f64 / 100.0;

        for i in 0..DIM {
            for j in 0..DIM {
                a[i][j] += x[i] * x[j];
            }
            b[i] += x[i] * y;
        }
    }

    // Свободный член не штрафуем: иначе прогноз тянет к нулю,
    // а не к среднему
    for i in 1..DIM {
        a[i][i] += lambda;
    }

    match solve(a, b) {
        Some(w) => Model {
            intercept: w[0],
            weights: w[1..].to_vec(),
            pairs: pairs.len(),
        },
        None => Model::baseline(),
    }
}

/// Метод Гаусса с выбором ведущего элемента по столбцу.
fn solve(mut a: [[f64; DIM]; DIM], mut b: [f64; DIM]) -> Option<[f64; DIM]> {
    for step in 0..DIM {
        let pivot = (step..DIM)
            .max_by(|&i, &j| a[i][step].abs().partial_cmp(&a[j][step].abs()).unwrap())?;
        if a[pivot][step].abs() < 1e-12 {
            return None; // вырожденная: обучаться не на чем
        }
        a.swap(step, pivot);
        b.swap(step, pivot);

        for row in (step + 1)..DIM {
            let m = a[row][step] / a[step][step];
            for col in step..DIM {
                a[row][col] -= m * a[step][col];
            }
            b[row] -= m * b[step];
        }
    }

    let mut w = [0.0; DIM];
    for step in (0..DIM).rev() {
        let mut sum = b[step];
        for col in (step + 1)..DIM {
            sum -= a[step][col] * w[col];
        }
        w[step] = sum / a[step][step];
    }
    Some(w)
}

/// Насколько прогнозам можно верить.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Accuracy {
    /// Сколько прогнозов уже проверено фактом
    pub checked: usize,
    /// Средняя абсолютная ошибка в баллах
    pub mean_error: Option<f64>,
    /// Доля попаданий в допуск, 0..1
    pub hit_rate: Option<f64>,
    /// Ширина допуска в баллах
    pub tolerance: i32,
    /// Та же ошибка у базовой линии — чтобы видеть, стоит ли модель
    /// своих сложностей
    pub baseline_error: Option<f64>,
}

/// Достоверность по парам «прогноз — факт».
///
/// `baseline` — что предсказала бы простая базовая линия на тех же
/// случаях. Без этого сравнения величина ошибки ни о чём не говорит:
/// хорошо это или плохо — восемь баллов, — видно только рядом с тем,
/// что даёт средний балл курсанта.
pub fn accuracy(pairs: &[(i32, i32)], baseline: &[i32], tolerance: i32) -> Accuracy {
    if pairs.is_empty() {
        return Accuracy {
            checked: 0,
            mean_error: None,
            hit_rate: None,
            tolerance,
            baseline_error: None,
        };
    }

    let n = pairs.len() as f64;
    let error = pairs.iter().map(|(p, a)| (p - a).abs() as f64).sum::<f64>() / n;
    let hits = pairs.iter().filter(|(p, a)| (p - a).abs() <= tolerance).count() as f64;

    let baseline_error = if baseline.len() == pairs.len() {
        Some(
            baseline
                .iter()
                .zip(pairs.iter())
                .map(|(b, (_, a))| (b - a).abs() as f64)
                .sum::<f64>()
                / n,
        )
    } else {
        None
    };

    let округлить = |v: f64| (v * 10.0).round() / 10.0;

    Accuracy {
        checked: pairs.len(),
        mean_error: Some(округлить(error)),
        hit_rate: Some((hits / n * 100.0).round() / 100.0),
        tolerance,
        baseline_error: baseline_error.map(округлить),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn разбор(score: i32, угадано: i32, просрочка: bool) -> Past {
        Past {
            score,
            learned: 10 - угадано,
            guessed: угадано,
            over_norm: просрочка,
            group_no: Some(1),
        }
    }

    #[test]
    fn короткая_история_прогноза_не_даёт() {
        let мало = vec![разбор(70, 2, false), разбор(75, 1, false)];
        assert!(features(&мало, None).is_none(),
                "по двум занятиям прогноз делать нельзя");
    }

    #[test]
    fn признаки_видят_рост_и_падение() {
        let растёт = vec![разбор(50, 0, false), разбор(60, 0, false),
                          разбор(70, 0, false), разбор(80, 0, false)];
        let падает: Vec<Past> = растёт.iter().rev().copied().collect();

        let вверх = features(&растёт, None).unwrap();
        let вниз = features(&падает, None).unwrap();

        assert!(вверх.trend > 0.0, "рост должен давать положительную динамику");
        assert!(вниз.trend < 0.0, "падение — отрицательную");
        // Средний балл один и тот же: динамика ловит именно порядок
        assert!((вверх.avg_score - вниз.avg_score).abs() < 1e-9);
    }

    #[test]
    fn доля_угаданного_считается_от_совпавшего() {
        let история = vec![разбор(70, 5, false), разбор(70, 5, false), разбор(70, 5, false)];
        let п = features(&история, None).unwrap();
        // По пять угаданных из десяти совпавших в каждом разборе
        assert!((п.guessed_share - 0.5).abs() < 1e-9, "{}", п.guessed_share);
    }

    #[test]
    fn балл_по_незнакомой_теме_берётся_общий() {
        let история = vec![разбор(60, 0, false), разбор(70, 0, false), разбор(80, 0, false)];
        let п = features(&история, Some(99)).unwrap();
        assert!((п.same_topic - п.avg_score).abs() < 1e-9,
                "на незнакомой теме опираемся на общий средний");
    }

    #[test]
    fn базовая_линия_предсказывает_средний_балл() {
        let история = vec![разбор(60, 0, false), разбор(70, 0, false), разбор(80, 0, false)];
        let п = features(&история, None).unwrap();
        assert_eq!(Model::baseline().predict(&п), 70);
    }

    /// Обучение на слишком малом наборе не выдаёт себя за модель.
    #[test]
    fn на_малом_наборе_остаёмся_на_базовой() {
        let история = vec![разбор(60, 0, false), разбор(70, 0, false), разбор(80, 0, false)];
        let п = features(&история, None).unwrap();
        let пары: Vec<(Features, i32)> = (0..5).map(|_| (п, 70)).collect();

        let м = fit(&пары, 0.1);
        assert!(!м.trained(), "пять пар — это не обучение");
        assert_eq!(м.pairs, 0);
    }

    /// Модель должна находить зависимость, которой в базовой линии нет.
    ///
    /// Собираем набор, где балл определяется не средним, а долей
    /// угаданного: базовая линия на нём беспомощна, регрессия — нет.
    #[test]
    fn регрессия_обходит_базовую_линию_там_где_есть_что_ловить() {
        let mut пары = Vec::new();
        let mut базовая_ошибка = 0.0;
        let mut прогнозы = Vec::new();

        for i in 0..60 {
            let угадано = i % 10;                       // 0..9 из десяти
            let история = vec![разбор(70, угадано, false); 5];
            let п = features(&история, None).unwrap();
            // Истинная зависимость: чем больше угадано, тем хуже итог
            let факт = 90 - угадано * 5;
            пары.push((п, факт));
            прогнозы.push((п, факт));
            базовая_ошибка += (Model::baseline().predict(&п) - факт).abs() as f64;
        }

        let модель = fit(&пары, 0.01);
        assert!(модель.trained(), "шестьдесят пар — достаточно");

        let ошибка: f64 = прогнозы
            .iter()
            .map(|(п, ф)| (модель.predict(п) - ф).abs() as f64)
            .sum();

        assert!(ошибка < базовая_ошибка / 2.0,
                "регрессия должна быть заметно точнее базовой: {ошибка} против {базовая_ошибка}");
    }

    #[test]
    fn достоверность_без_проверенных_прогнозов_молчит() {
        let пусто = accuracy(&[], &[], 10);
        assert_eq!(пусто.checked, 0);
        assert!(пусто.mean_error.is_none(), "не из чего считать — нечего и показывать");
        assert!(пусто.hit_rate.is_none());
    }

    #[test]
    fn достоверность_считает_ошибку_и_попадания() {
        //                прогноз, факт
        let пары = [(70, 71), (60, 75), (80, 78), (50, 52)];
        let базовая = [65, 65, 65, 65];
        let д = accuracy(&пары, &базовая, 5);

        assert_eq!(д.checked, 4);
        // Ошибки: 1, 15, 2, 2 → среднее 5
        assert_eq!(д.mean_error, Some(5.0));
        // В допуск ±5 попали три из четырёх
        assert_eq!(д.hit_rate, Some(0.75));
        // Базовая: 6, 10, 13, 13 → среднее 10,5
        assert_eq!(д.baseline_error, Some(10.5));
    }

    #[test]
    fn прогноз_не_вылезает_за_шкалу() {
        let история = vec![разбор(100, 0, false); 5];
        let п = features(&история, None).unwrap();
        let завышенная = Model { intercept: 5.0, weights: vec![5.0; Features::COUNT], pairs: 99 };
        assert_eq!(завышенная.predict(&п), 100);

        let заниженная = Model { intercept: -5.0, weights: vec![0.0; Features::COUNT], pairs: 99 };
        assert_eq!(заниженная.predict(&п), 0);
    }

    #[test]
    fn вклады_складываются_в_прогноз() {
        let история = vec![разбор(60, 2, true), разбор(70, 3, false), разбор(80, 1, false)];
        let п = features(&история, None).unwrap();
        let м = Model { intercept: 0.1, weights: vec![0.5, 0.2, -0.3, -0.1, 0.05, 0.4], pairs: 99 };

        let сумма: i32 = м.contributions(&п).iter().map(|(_, в)| в).sum();
        let свободный = (м.intercept * 100.0).round() as i32;
        // Округление по частям даёт расхождение в пару баллов — важно,
        // что объяснение сходится с прогнозом, а не что оно точное
        assert!((сумма + свободный - м.predict(&п)).abs() <= 3,
                "вклады должны объяснять прогноз: {} против {}",
                сумма + свободный, м.predict(&п));
    }
}
