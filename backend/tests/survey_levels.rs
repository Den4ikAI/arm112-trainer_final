//! Все уровни признаков опросной карты доступны курсанту.
//!
//! Жалоба с проверки 29.09: «признак второго уровня может не появиться
//! для заполнения». Причина — карты, снятые со скриншотов (101, 104,
//! «Взрыв»): кадр показывает одну открытую ветку, а второй и третий
//! признаки классификатора под остальными вариантами выбрать было нельзя.
//! У 104 после «Запах газа в помещении» не было «Квартира / Кухня / Дом
//! частный», у «Взрыва» после «Здание / Объект» — списка объектов, у 101
//! для «Здание / объект» — объектов вроде АЗС и лечебного учреждения.
//!
//! Проверка ходит по карте так же, как курсант: видимость вопросов и
//! вариантов — по тем же правилам, что в интерфейсе (`showWhen`,
//! `dependsOn`, `optionParents`), — и требует, чтобы до каждой позиции
//! классификатора группы можно было дойти.
//!
//! Запуск: `cargo test --test survey_levels`

use arm112::{config::Config, domain::classify::classify, ingest::survey::materialise};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

fn адрес_базы(config: &Config) -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| config.database_url.clone())
}

async fn база() -> Option<(PgPool, Uuid)> {
    let config = Config::from_env().ok()?;
    let pool = PgPool::connect(&адрес_базы(&config)).await.ok()?;
    sqlx::migrate!("./migrations").run(&pool).await.ok()?;
    let version: Uuid =
        sqlx::query_scalar("SELECT id FROM classifier_versions WHERE is_active")
            .fetch_optional(&pool)
            .await
            .ok()??;
    materialise(&pool, version).await.expect("карты не собрались");
    Some((pool, version))
}

fn норм(s: &str) -> String {
    s.trim().to_lowercase()
}

#[derive(Debug, Clone)]
struct Вопрос {
    id: String,
    зависит_от: Option<String>,
    /// вариант → родители; нет в словаре — показывается всегда
    родители: HashMap<String, Vec<String>>,
    варианты: Vec<String>,
    /// (вопрос, значения) — хоть одно условие
    когда: Vec<(String, Vec<String>)>,
}

struct Карта {
    вопросы: Vec<Вопрос>,
    /// (вопрос, вариант) → признаки (уровень, текст)
    признаки: HashMap<(String, String), Vec<(i16, String)>>,
}

async fn карта(pool: &PgPool, тип: &str) -> Карта {
    let строки: Vec<(String, String, Option<String>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT id, kind::text, depends_on, show_when FROM survey_questions
          WHERE type_id = $1 ORDER BY sort_order, derived, id",
    )
    .bind(тип)
    .fetch_all(pool)
    .await
    .unwrap();
    let mut вопросы = Vec::new();
    for (id, _kind, dep, when) in строки {
        let опции: Vec<(String, Option<Vec<String>>)> = sqlx::query_as(
            "SELECT value, parents FROM survey_options
              WHERE type_id = $1 AND question_id = $2 ORDER BY sort_order",
        )
        .bind(тип)
        .bind(&id)
        .fetch_all(pool)
        .await
        .unwrap();
        let когда = when
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .map(|w| {
                (
                    w["question"].as_str().unwrap_or_default().to_string(),
                    w["values"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                )
            })
            .collect();
        вопросы.push(Вопрос {
            id,
            зависит_от: dep,
            родители: опции.iter().filter_map(|(v, p)| p.clone().map(|p| (v.clone(), p))).collect(),
            варианты: опции.into_iter().map(|(v, _)| v).collect(),
            когда,
        });
    }
    let map: Vec<(String, String, i16, String)> = sqlx::query_as(
        "SELECT question_id, option_value, sign_level, sign_text FROM sign_mapping WHERE type_id = $1",
    )
    .bind(тип)
    .fetch_all(pool)
    .await
    .unwrap();
    let mut признаки: HashMap<(String, String), Vec<(i16, String)>> = HashMap::new();
    for (q, v, l, t) in map {
        признаки.entry((q, v)).or_default().push((l, t));
    }
    Карта { вопросы, признаки }
}

/// Видимые варианты вопроса при текущих ответах — как `видимые` в
/// SurveyCardView.tsx. `None` — вопрос скрыт.
fn видимые(к: &Карта, ответы: &BTreeMap<String, String>, q: &Вопрос) -> Option<Vec<String>> {
    // showWhen: условие выполнено, только если виден и сам вопрос условия
    if !q.когда.is_empty() {
        let ок = q.когда.iter().any(|(вопрос, значения)| {
            let Some(ответ) = ответы.get(вопрос) else { return false };
            let виден = к
                .вопросы
                .iter()
                .find(|x| &x.id == вопрос)
                .map(|x| видимые(к, ответы, x).is_some())
                .unwrap_or(false);
            виден && значения.contains(ответ)
        });
        if !ок {
            return None;
        }
    }
    let варианты: Vec<String> = match &q.зависит_от {
        None => q.варианты.clone(),
        Some(dep) => {
            let родитель = ответы.get(dep);
            q.варианты
                .iter()
                .filter(|v| match q.родители.get(*v) {
                    None => true,
                    Some(p) => родитель.is_some_and(|r| p.contains(r)),
                })
                .cloned()
                .collect()
        }
    };
    if q.зависит_от.is_some() && варианты.is_empty() {
        return None;
    }
    Some(варианты)
}

/// Все наборы признаков (L1, L2, L3), до которых можно дойти, отвечая не
/// больше чем на `глубина` вопросов с признаками.
fn достижимые(к: &Карта, глубина: usize) -> BTreeSet<(String, String, String)> {
    // Вопросы с выбором: вариант без признака тоже может открыть другой
    // вопрос («Сработала пожарная сигнализация» открывает объект транспорта)
    let с_признаками: Vec<&Вопрос> = к.вопросы.iter().filter(|q| !q.варианты.is_empty()).collect();
    let mut итог = BTreeSet::new();
    let mut стек: Vec<BTreeMap<String, String>> = vec![BTreeMap::new()];
    while let Some(ответы) = стек.pop() {
        // Признаки этого набора ответов — по уровням
        let mut уровни: [BTreeSet<String>; 3] = Default::default();
        for (q, v) in &ответы {
            for (l, t) in к.признаки.get(&(q.clone(), v.clone())).into_iter().flatten() {
                уровни[(*l as usize) - 1].insert(норм(t));
            }
        }
        for a in &уровни[0] {
            let b: Vec<String> = if уровни[1].is_empty() { vec![String::new()] } else { уровни[1].iter().cloned().collect() };
            let c: Vec<String> = if уровни[2].is_empty() { vec![String::new()] } else { уровни[2].iter().cloned().collect() };
            for x in &b {
                for y in &c {
                    итог.insert((a.clone(), x.clone(), y.clone()));
                }
            }
        }
        if ответы.len() >= глубина {
            continue;
        }
        for q in &с_признаками {
            if ответы.contains_key(&q.id) {
                continue;
            }
            let Some(варианты) = видимые(к, &ответы, q) else { continue };
            for v in варианты {
                // Вариант без признака и без вопросов, которые он открывает,
                // ничего не меняет — не ветвимся на нём
                let открывает = к.вопросы.iter().any(|x| {
                    x.когда.iter().any(|(w, vals)| w == &q.id && vals.contains(&v))
                        || x.зависит_от.as_deref() == Some(q.id.as_str())
                });
                if !к.признаки.contains_key(&(q.id.clone(), v.clone())) && !открывает {
                    continue;
                }
                let mut дальше = ответы.clone();
                дальше.insert(q.id.clone(), v);
                стек.push(дальше);
            }
        }
    }
    итог
}

/// Для каждого типа с картой — до каждой позиции его группы классификатора
/// можно дойти: первый, второй и третий признаки выбираются в карте.
#[tokio::test]
async fn до_каждой_позиции_классификатора_можно_дойти_по_карте() {
    let Some((pool, v)) = база().await else { return };

    let типы: Vec<(String, i32)> = sqlx::query_as(
        "SELECT t.id, t.group_no FROM incident_types t
          WHERE t.group_no IS NOT NULL
            AND EXISTS (SELECT 1 FROM survey_questions q WHERE q.type_id = t.id)
          ORDER BY t.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(типы.len() >= 40, "карт должно быть больше сорока: {}", типы.len());

    let mut пропуски: Vec<String> = Vec::new();
    for (тип, группа) in &типы {
        let к = карта(&pool, тип).await;
        let позиции: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT sign1, sign2, sign3 FROM classifier_positions
              WHERE version_id = $1 AND group_no = $2
                AND sign1 IS NOT NULL AND sign1 <> '' AND sign1 <> 'Не отображается оператору 112'",
        )
        .bind(v)
        .bind(группа)
        .fetch_all(&pool)
        .await
        .unwrap();
        // Глубина 4: у 101 до листа — «Где», признак пожара, объект и
        // внутридомовой объект
        let можно = достижимые(&к, 4);
        for (s1, s2, s3) in позиции {
            let ключ = (
                норм(&s1),
                s2.as_deref().map(норм).unwrap_or_default(),
                s3.as_deref().map(норм).unwrap_or_default(),
            );
            if !можно.contains(&ключ) {
                пропуски.push(format!("{тип}: {} / {} / {}", ключ.0, ключ.1, ключ.2));
            }
        }
    }
    assert!(
        пропуски.is_empty(),
        "до {} позиций не дойти по карте:\n{}",
        пропуски.len(),
        пропуски.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}

fn ответы(пары: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
    пары.iter().map(|(q, v)| (q.to_string(), vec![v.to_string()])).collect()
}

/// Вопрос второго уровня с данной подписью и вариантом-родителем.
async fn дописанный(pool: &PgPool, тип: &str, подпись: &str) -> String {
    sqlx::query_scalar("SELECT id FROM survey_questions WHERE type_id = $1 AND label = $2")
        .bind(тип)
        .bind(подпись)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|_| panic!("у {тип} нет вопроса «{подпись}»"))
}

/// 104: после «Запах газа в помещении» есть второй уровень («Квартира
/// помещение») и третий («Открыто»), и классификатор определяет позицию.
#[tokio::test]
async fn у_104_после_первого_признака_есть_второй_и_третий() {
    let Some((pool, v)) = база().await else { return };
    let вариант = "Запах газа в помещении (в квартире, в доме)";
    let p2 = дописанный(&pool, "104", &format!("Признак 2 ({вариант})")).await;
    let p3 = дописанный(&pool, "104", &format!("Признак 3 ({вариант})")).await;

    let r = classify(&pool, v, &["104".into()], &ответы(&[
        ("gas-sign", вариант),
        (&p2, "Квартира помещение"),
        (&p3, "Открыто"),
    ]))
    .await
    .unwrap();
    assert!(r.reason.is_none(), "классификатор не должен жаловаться: {:?}", r.reason);
    assert!(r.outcome.is_some(), "итоговый тип должен определиться");
    assert!(!r.services.is_empty(), "службы должны подобраться");
}

/// «Взрыв»: «Здание / Объект» раскрывается списком объектов классификатора.
#[tokio::test]
async fn у_взрыва_здание_раскрывается_объектами() {
    let Some((pool, v)) = база().await else { return };
    let p2 = дописанный(&pool, "explosion", "Признак 2 (Здание / Объект)").await;
    let r = classify(&pool, v, &["explosion".into()], &ответы(&[
        ("where-blast", "Здание / Объект"),
        (&p2, "Лечебное заведение"),
    ]))
    .await
    .unwrap();
    assert!(r.outcome.is_some(), "итоговый тип должен определиться: {:?}", r.reason);
}

/// 101: «Здание / объект» — объекты классификатора, а не вопросы про
/// жилой дом; «Дом частный» при «Здание / объект» не показывается.
#[tokio::test]
async fn у_101_здание_объект_это_объекты_а_не_жилой_дом() {
    let Some((pool, v)) = база().await else { return };
    let p2 = дописанный(&pool, "101", "Признак 2 (Здание / объект)").await;

    let к = карта(&pool, "101").await;
    let mut выбор = BTreeMap::new();
    выбор.insert("where".to_string(), "Здание / объект".to_string());
    let дом = к.вопросы.iter().find(|q| q.id == "house-kind").unwrap();
    assert!(видимые(&к, &выбор, дом).is_none(), "вопрос про жилой дом виден при «Здание / объект»");
    let объект = к.вопросы.iter().find(|q| q.id == p2).unwrap();
    let варианты = видимые(&к, &выбор, объект).expect("объекты не видны");
    assert!(варианты.iter().any(|x| норм(x) == "лечебное"), "нет «лечебное»: {варианты:?}");
    assert!(!варианты.iter().any(|x| норм(x) == "опасный"), "«опасный» задаёт свой вариант «Где»");

    let r = classify(&pool, v, &["101".into()], &ответы(&[
        ("where", "Здание / объект"),
        ("fire-sign", "Открытое пламя / Дым"),
        (&p2, "лечебное"),
    ]))
    .await
    .unwrap();
    assert!(r.outcome.is_some(), "итоговый тип должен определиться: {:?}", r.reason);
    assert!(r.outcome.unwrap().to_lowercase().contains("лечеб"), "должен быть пожар в лечебном");
}
