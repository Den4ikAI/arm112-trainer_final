//! Голос: распознавание и синтез речи через сервер голоса (`voice/`).
//!
//! Сервер голоса — отдельный процесс на Python: модели (GigaAM v3,
//! TeraTTSv2) идут в ONNX и работают на процессоре. Адрес задаёт
//! `VOICE_BASE_URL`. Не задан — остаются заглушки, и тренажёр ведёт
//! разговор текстом: голос дополняет обучение, а не держит его.

use super::*;
use std::time::Duration;

/// Подключить голос, если задан адрес сервера.
pub fn attach(suite: &mut MlSuite) {
    let Ok(base) = std::env::var("VOICE_BASE_URL") else { return };
    let base = base.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return;
    }
    match VoiceClient::new(base.clone()) {
        Ok(client) => {
            tracing::info!(url = %base, "голос подключён");
            suite.asr = Box::new(VoiceAsr(client.clone()));
            suite.tts = Box::new(VoiceTts(client));
        }
        Err(e) => tracing::warn!(error = %e, "голос не подключён"),
    }
}

#[derive(Clone)]
pub struct VoiceClient {
    base: String,
    http: reqwest::Client,
}

impl VoiceClient {
    pub fn new(base: String) -> Result<Self> {
        // Реплика — до минуты звука, распознаётся за доли секунды; синтез
        // фразы — около секунды. Полминуты хватает с запасом даже под
        // нагрузкой, а дольше курсант ждать не станет
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(3))
            .build()?;
        Ok(Self { base, http })
    }

    async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("сервер голоса ответил {status}: {body}")
    }
}

pub struct VoiceAsr(pub VoiceClient);

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsrReply {
    text: String,
    #[serde(default)]
    duration_ms: u32,
}

#[async_trait]
impl Asr for VoiceAsr {
    async fn transcribe(&self, audio: &[u8]) -> Result<Transcript> {
        let resp = self
            .0
            .http
            .post(format!("{}/asr", self.0.base))
            .header(reqwest::header::CONTENT_TYPE, "audio/wav")
            .body(audio.to_vec())
            .send()
            .await?;
        let reply: AsrReply = VoiceClient::check(resp).await?.json().await?;
        // Сервер отдаёт фразу целиком: разбивка по словам курсанту
        // в звонке не нужна, а время реплики — нужно
        Ok(Transcript {
            segments: vec![Segment {
                text: reply.text.clone(),
                start_ms: 0,
                end_ms: reply.duration_ms,
                conf: 1.0,
            }],
            text: reply.text,
        })
    }

    fn available(&self) -> bool {
        true
    }
}

pub struct VoiceTts(pub VoiceClient);

#[async_trait]
impl Tts for VoiceTts {
    async fn synthesize(&self, text: &str, voice: &str) -> Result<Audio> {
        let wav = self.wav(text, voice).await?;
        Ok(Audio {
            audio_ref: format!("voice://{voice}/{}", super::mock::stable_hash(text)),
            duration_ms: wav_duration_ms(&wav),
        })
    }

    async fn wav(&self, text: &str, voice: &str) -> Result<Vec<u8>> {
        let resp = self
            .0
            .http
            .post(format!("{}/tts", self.0.base))
            .json(&serde_json::json!({ "text": text, "voice": voice }))
            .send()
            .await?;
        Ok(VoiceClient::check(resp).await?.bytes().await?.to_vec())
    }

    fn available(&self) -> bool {
        true
    }
}

/// Длительность WAV по заголовку. Кривой заголовок — ноль, а не ошибка:
/// длительность нужна для подписи, не для воспроизведения
pub fn wav_duration_ms(wav: &[u8]) -> u32 {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return 0;
    }
    let byte_rate = u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]);
    if byte_rate == 0 {
        return 0;
    }
    let data = (wav.len() - 44) as u64;
    (data * 1000 / byte_rate as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, seconds: u32) -> Vec<u8> {
        let data = rate * 2 * seconds;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.resize(out.len() + data as usize, 0);
        out
    }

    #[test]
    fn длительность_берётся_из_заголовка_wav() {
        assert_eq!(wav_duration_ms(&wav(16_000, 2)), 2000);
        assert_eq!(wav_duration_ms(&wav(44_100, 1)), 1000);
    }

    #[test]
    fn кривой_wav_даёт_ноль_а_не_панику() {
        assert_eq!(wav_duration_ms(b"not a wav"), 0);
        assert_eq!(wav_duration_ms(&[0u8; 60]), 0);
    }
}
