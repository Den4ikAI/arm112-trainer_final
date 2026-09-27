//! Сравнение версий классификатора.
//!
//! Классификатор живой: между двумя версиями из материалов задачи
//! появились восемь служб и позиция «БПЛА». Для тренажёра это не
//! формальность — по классификатору считается и список оповещения,
//! и эталон при разборе. Подменить его молча значит начать оценивать
//! людей по другим правилам, не сказав им об этом.
//!
//! Поэтому дифф показывает не только что поменялось в справочнике,
//! но и кого это задевает: какие сценарии перестали соответствовать
//! эталону и кто из курсантов не отрабатывал новые позиции.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRef {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PositionRef {
    pub number: String,
    pub group_no: i32,
    pub group_title: String,
    pub sign1: Option<String>,
    pub sign2: Option<String>,
    pub sign3: Option<String>,
    pub outcome_type: Option<String>,
}

/// Что именно поменялось в позиции, которая есть в обеих версиях.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PositionChange {
    pub number: String,
    pub group_no: i32,
    /// Поле → (было, стало)
    pub fields: Vec<FieldChange>,
    /// Службы, добавленные и убранные в списке оповещения этой позиции
    pub services_added: Vec<String>,
    pub services_removed: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: &'static str,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionRef {
    pub id: Uuid,
    pub name: String,
    pub positions_count: i32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub from: VersionRef,
    pub to: VersionRef,
    pub services_added: Vec<ServiceRef>,
    pub services_removed: Vec<ServiceRef>,
    pub positions_added: Vec<PositionRef>,
    pub positions_removed: Vec<PositionRef>,
    pub positions_changed: Vec<PositionChange>,
    pub summary: Summary,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub services_added: usize,
    pub services_removed: usize,
    pub positions_added: usize,
    pub positions_removed: usize,
    pub positions_changed: usize,
    /// Из них: изменились формулировки признаков или итогового типа.
    /// Таких единицы, и именно их имеет смысл вычитывать глазами.
    pub positions_retitled: usize,
    /// Из них: изменился только список оповещения. Таких бывает больше
    /// тысячи — новая служба добавляется сразу во все позиции, — и
    /// просматривать их по одной бессмысленно.
    pub positions_renotified: usize,
}

async fn version_ref(pool: &PgPool, id: Uuid) -> Result<VersionRef> {
    let (name, positions_count): (String, i32) =
        sqlx::query_as("SELECT name, positions_count FROM classifier_versions WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("версия классификатора не найдена"))?;
    Ok(VersionRef { id, name, positions_count })
}

pub async fn diff(pool: &PgPool, from: Uuid, to: Uuid) -> Result<Diff> {
    let from_ref = version_ref(pool, from).await?;
    let to_ref = version_ref(pool, to).await?;

    // Служба «появилась», если её нет ни в одной позиции старой версии.
    // Сама таблица services общая для всех версий: там накапливаются
    // все, кто когда-либо встречался.
    //
    // Название — заголовок столбца из файла, а не то, что видит курсант:
    // сравниваются файлы классификатора, и администратор сверяет список
    // с самим XLSX и с условием задачи
    let services_added: Vec<ServiceRef> = sqlx::query_as(
        "SELECT s.id, COALESCE(s.source_title, s.title) AS title FROM services s
          WHERE EXISTS (SELECT 1 FROM classifier_position_services ps
                          JOIN classifier_positions p ON p.id = ps.position_id
                         WHERE p.version_id = $2 AND ps.service_id = s.id)
            AND NOT EXISTS (SELECT 1 FROM classifier_position_services ps
                              JOIN classifier_positions p ON p.id = ps.position_id
                             WHERE p.version_id = $1 AND ps.service_id = s.id)
          ORDER BY 2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let services_removed: Vec<ServiceRef> = sqlx::query_as(
        "SELECT s.id, COALESCE(s.source_title, s.title) AS title FROM services s
          WHERE EXISTS (SELECT 1 FROM classifier_position_services ps
                          JOIN classifier_positions p ON p.id = ps.position_id
                         WHERE p.version_id = $1 AND ps.service_id = s.id)
            AND NOT EXISTS (SELECT 1 FROM classifier_position_services ps
                              JOIN classifier_positions p ON p.id = ps.position_id
                             WHERE p.version_id = $2 AND ps.service_id = s.id)
          ORDER BY 2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let positions_added: Vec<PositionRef> = sqlx::query_as(
        "SELECT number, group_no, group_title, sign1, sign2, sign3, outcome_type
           FROM classifier_positions p
          WHERE p.version_id = $2
            AND NOT EXISTS (SELECT 1 FROM classifier_positions q
                             WHERE q.version_id = $1 AND q.number = p.number)
          ORDER BY number",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let positions_removed: Vec<PositionRef> = sqlx::query_as(
        "SELECT number, group_no, group_title, sign1, sign2, sign3, outcome_type
           FROM classifier_positions p
          WHERE p.version_id = $1
            AND NOT EXISTS (SELECT 1 FROM classifier_positions q
                             WHERE q.version_id = $2 AND q.number = p.number)
          ORDER BY number",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    let positions_changed = changed_positions(pool, from, to).await?;

    let summary = Summary {
        services_added: services_added.len(),
        services_removed: services_removed.len(),
        positions_added: positions_added.len(),
        positions_removed: positions_removed.len(),
        positions_changed: positions_changed.len(),
        positions_retitled: positions_changed.iter().filter(|p| !p.fields.is_empty()).count(),
        positions_renotified: positions_changed.iter().filter(|p| p.fields.is_empty()).count(),
    };

    Ok(Diff {
        from: from_ref,
        to: to_ref,
        services_added,
        services_removed,
        positions_added,
        positions_removed,
        positions_changed,
        summary,
    })
}

/// Позиции, которые есть в обеих версиях, но чем-то отличаются.
///
/// Сравниваем и признаки, и список оповещения: исправленная опечатка
/// в названии признака для курсанта почти безобидна, а исчезнувшая
/// из позиции служба означает, что по этой позиции больше никто
/// не поедет.
async fn changed_positions(pool: &PgPool, from: Uuid, to: Uuid) -> Result<Vec<PositionChange>> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String, i32,
        Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
        Option<String>, Option<String>, Option<String>, Option<String>, Option<String>,
        Vec<String>, Vec<String>,
    )> = sqlx::query_as(
        "WITH a AS (
            SELECT p.id, p.number, p.group_no, p.sign1, p.sign2, p.sign3,
                   p.outcome_type, p.group_title
              FROM classifier_positions p WHERE p.version_id = $1
         ), b AS (
            SELECT p.id, p.number, p.group_no, p.sign1, p.sign2, p.sign3,
                   p.outcome_type, p.group_title
              FROM classifier_positions p WHERE p.version_id = $2
         )
         SELECT b.number, b.group_no,
                a.sign1, b.sign1, a.sign2, b.sign2,
                a.sign3, b.sign3, a.outcome_type, b.outcome_type,
                a.group_title, b.group_title,
                COALESCE((SELECT array_agg(DISTINCT ps.service_id ORDER BY ps.service_id)
                            FROM classifier_position_services ps
                           WHERE ps.position_id = b.id
                             AND ps.service_id NOT IN (
                                 SELECT service_id FROM classifier_position_services
                                  WHERE position_id = a.id)), '{}') AS added,
                COALESCE((SELECT array_agg(DISTINCT ps.service_id ORDER BY ps.service_id)
                            FROM classifier_position_services ps
                           WHERE ps.position_id = a.id
                             AND ps.service_id NOT IN (
                                 SELECT service_id FROM classifier_position_services
                                  WHERE position_id = b.id)), '{}') AS removed
           FROM a JOIN b ON b.number = a.number
          WHERE a.sign1 IS DISTINCT FROM b.sign1
             OR a.sign2 IS DISTINCT FROM b.sign2
             OR a.sign3 IS DISTINCT FROM b.sign3
             OR a.outcome_type IS DISTINCT FROM b.outcome_type
             OR a.group_title IS DISTINCT FROM b.group_title
             OR EXISTS (SELECT service_id FROM classifier_position_services
                         WHERE position_id = a.id
                        EXCEPT
                        SELECT service_id FROM classifier_position_services
                         WHERE position_id = b.id)
             OR EXISTS (SELECT service_id FROM classifier_position_services
                         WHERE position_id = b.id
                        EXCEPT
                        SELECT service_id FROM classifier_position_services
                         WHERE position_id = a.id)
          ORDER BY b.number",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(number, group_no, s1a, s1b, s2a, s2b, s3a, s3b, oa, ob, ga, gb, added, removed)| {
            let mut fields = Vec::new();
            let mut push = |field, before: Option<String>, after: Option<String>| {
                if before != after {
                    fields.push(FieldChange { field, before, after });
                }
            };
            push("sign1", s1a, s1b);
            push("sign2", s2a, s2b);
            push("sign3", s3a, s3b);
            push("outcomeType", oa, ob);
            push("groupTitle", ga, gb);

            PositionChange {
                number,
                group_no,
                fields,
                services_added: added,
                services_removed: removed,
            }
        })
        .collect())
}

// ─── Кого задевает смена версии ──────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    /// Сценарии, чей эталон перестал совпадать с новой версией
    pub scenarios_drifted: Vec<ScenarioDrift>,
    /// Курсанты, не отрабатывавшие новые позиции
    pub students_uncovered: Vec<StudentCoverage>,
    /// Новые позиции, которых нет ни в одном сценарии
    pub positions_untrained: Vec<PositionRef>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioDrift {
    pub id: Uuid,
    pub ticket: Option<i32>,
    pub call_no: Option<i32>,
    pub legend: String,
    /// Что даёт новая версия
    pub services_now: Vec<String>,
    /// Что записано в эталоне
    pub services_expected: Vec<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StudentCoverage {
    pub user_id: Uuid,
    pub fio: String,
    pub sessions: i64,
}

/// Что изменится, если сделать версию `to` активной.
///
/// Считается до переключения: преподаватель должен увидеть цену
/// решения заранее, а не обнаружить потом, что половина сценариев
/// оценивает не по тому справочнику.
pub async fn impact(pool: &PgPool, from: Uuid, to: Uuid) -> Result<Impact> {
    let d = diff(pool, from, to).await?;

    // Сценарий «поплыл», если движок на новой версии даёт не тот
    // список служб, что записан в его эталоне
    let scenarios: Vec<(Uuid, Option<i32>, Option<i32>, String, serde_json::Value)> =
        sqlx::query_as(
            "SELECT id, ticket, call_no, legend, expected FROM scenarios
              WHERE status <> 'archived' ORDER BY ticket, call_no",
        )
        .fetch_all(pool)
        .await?;

    let mut scenarios_drifted = Vec::new();
    // Какие позиции классификатора вообще встречаются в сценариях:
    // собираем из тех же прогонов, отдельный запрос тут не нужен
    let mut covered: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (id, ticket, call_no, legend, expected) in scenarios {
        let type_ids: Vec<String> = expected
            .get("typeIds")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let answers = expected
            .get("answers")
            .map(crate::api::sessions::answers_to_map)
            .unwrap_or_default();

        let result = crate::domain::classify::classify(pool, to, &type_ids, &answers).await?;
        covered.extend(result.positions.iter().cloned());
        let mut services_now: Vec<String> =
            result.services.iter().map(|s| s.service_id.clone()).collect();
        services_now.sort();

        let mut services_expected: Vec<String> = expected
            .get("services")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        services_expected.sort();

        if services_now != services_expected {
            scenarios_drifted.push(ScenarioDrift {
                id, ticket, call_no, legend, services_now, services_expected,
            });
        }
    }

    // Новые позиции, на которых тренироваться не на чем: ни один
    // сценарий к ним не приводит
    let positions_untrained: Vec<PositionRef> = d
        .positions_added
        .into_iter()
        .filter(|p| !covered.contains(&p.number))
        .collect();

    // Курсанты, которым новые позиции не попадались. Пока сценариев
    // на них нет вовсе, это все — и это правильный ответ: переаттестацию
    // проходить некому и не на чем
    let students_uncovered: Vec<StudentCoverage> = if positions_untrained.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            "SELECT u.id AS user_id, u.fio,
                    (SELECT count(*) FROM sessions s WHERE s.user_id = u.id) AS sessions
               FROM users u
              WHERE u.role = 'student' AND u.is_active
              ORDER BY u.fio",
        )
        .fetch_all(pool)
        .await?
    };

    Ok(Impact { scenarios_drifted, students_uncovered, positions_untrained })
}
