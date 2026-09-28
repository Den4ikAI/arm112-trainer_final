//! Кросс-энкодер NLI: следует ли гипотеза из посылки.
//!
//! Нужен для сверки описания курсанта с фактами вызова на процессоре.
//! Генеративная модель на ЦП делает это десятки секунд и не видит
//! упущенного (backend/docs/замеры/ОПИСАНИЯ.md: 73 %, из 26 фактов, о
//! которых описание молчит, 16 назвала подтверждёнными). Кросс-энкодер
//! OpenJEV отвечает одним проходом на факт, без генерации, одинаково
//! при повторе: 88–91 % за 2–5 с на описание.
//!
//! Модель работает в отдельном сервере (`nli/server.py`), адрес —
//! `NLI_BASE_URL`. Не задан — сверка идёт языковой моделью, как раньше.
//!
//! Отдельный трейт, а не `LabelScorer`: тот ранжирует варианты одного
//! вопроса, а здесь на каждую пару «посылка — гипотеза» три вероятности,
//! и сравнивать их между гипотезами не нужно.

use super::MlSuite;
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Вероятности по одной гипотезе.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entailment {
    pub contradiction: f32,
    pub entailment: f32,
    pub neutral: f32,
}

/// Итог по гипотезе — что больше всего вероятно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Contradiction,
    Entailment,
    Neutral,
}

impl Entailment {
    pub fn verdict(&self) -> Verdict {
        if self.contradiction >= self.entailment && self.contradiction >= self.neutral {
            Verdict::Contradiction
        } else if self.entailment >= self.neutral {
            Verdict::Entailment
        } else {
            Verdict::Neutral
        }
    }
}

#[async_trait]
pub trait Nli: Send + Sync {
    /// По вероятности на каждую гипотезу, в том же порядке.
    async fn classify(&self, premise: &str, hypotheses: &[String]) -> Result<Vec<Entailment>>;

    /// Подключён ли настоящий кросс-энкодер. Заглушка ничего не
    /// понимает, и её ответ нельзя выдавать за сверку
    fn available(&self) -> bool {
        false
    }

    /// Название модели для подписи в разборе
    fn model(&self) -> String {
        String::new()
    }
}

/// Кросс-энкодера нет.
pub struct NoNli;

#[async_trait]
impl Nli for NoNli {
    async fn classify(&self, _premise: &str, _hypotheses: &[String]) -> Result<Vec<Entailment>> {
        anyhow::bail!("кросс-энкодер не подключён (NLI_BASE_URL не задан)")
    }
}

/// Подключить кросс-энкодер, если задан адрес сервера.
pub fn attach(suite: &mut MlSuite) {
    let Ok(base) = std::env::var("NLI_BASE_URL") else { return };
    let base = base.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return;
    }
    match HttpNli::new(base.clone()) {
        Ok(nli) => {
            tracing::info!(url = %base, "кросс-энкодер NLI подключён");
            suite.nli = Box::new(nli);
        }
        Err(e) => tracing::warn!(error = %e, "кросс-энкодер NLI не подключён"),
    }
}

pub struct HttpNli {
    base: String,
    http: reqwest::Client,
}

impl HttpNli {
    pub fn new(base: String) -> Result<Self> {
        // Описание с десятком фактов на 4B — до десяти секунд на шести
        // потоках; минута — запас на очередь из нескольких карточек
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(3))
            .build()?;
        Ok(Self { base, http })
    }
}

#[derive(serde::Deserialize)]
struct Reply {
    /// [contradiction, entailment, neutral] на каждую гипотезу
    probs: Vec<[f32; 3]>,
}

#[async_trait]
impl Nli for HttpNli {
    async fn classify(&self, premise: &str, hypotheses: &[String]) -> Result<Vec<Entailment>> {
        if hypotheses.is_empty() {
            return Ok(Vec::new());
        }
        let resp = self
            .http
            .post(format!("{}/nli", self.base))
            .json(&serde_json::json!({ "premise": premise, "hypotheses": hypotheses }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("сервер NLI ответил {status}: {}", body.chars().take(300).collect::<String>());
        }
        let reply: Reply = resp.json().await?;
        if reply.probs.len() != hypotheses.len() {
            anyhow::bail!(
                "сервер NLI вернул {} ответов на {} гипотез",
                reply.probs.len(),
                hypotheses.len()
            );
        }
        Ok(reply
            .probs
            .into_iter()
            .map(|[c, e, n]| Entailment { contradiction: c, entailment: e, neutral: n })
            .collect())
    }

    fn available(&self) -> bool {
        true
    }

    fn model(&self) -> String {
        "OpenJEV".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn вердикт_по_наибольшей_вероятности() {
        let e = |c, en, n| Entailment { contradiction: c, entailment: en, neutral: n };
        assert_eq!(e(0.7, 0.2, 0.1).verdict(), Verdict::Contradiction);
        assert_eq!(e(0.1, 0.8, 0.1).verdict(), Verdict::Entailment);
        assert_eq!(e(0.1, 0.2, 0.7).verdict(), Verdict::Neutral);
    }
}
