//! Запись разговора IP-телефона одним файлом.
//!
//! ТЗ: «MP3 или WAV для аудиозаписей учебных вызовов». Реплики
//! разговора приходят кусками разного вида: голос диспетчера — WAV
//! 16 кГц моно с микрофона, голос собеседника — WAV синтеза (44,1 кГц).
//! Здесь они сводятся в один WAV 16 кГц моно 16 бит с короткой паузой
//! между репликами. Кодеков не нужно: только PCM.

/// Частота записи разговора: телефонная речь, как у распознавания.
pub const RATE: u32 = 16_000;

/// Пауза между репликами, мс.
const GAP_MS: u32 = 400;

/// Звук одной реплики: моно, `f32` в [-1, 1].
#[derive(Debug, Clone)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<f32>,
}

/// Разобрать WAV: PCM 8/16/24/32 бит или float32, любое число каналов.
/// Кривой файл — `None`, а не паника: запись собирается из того, что
/// прислал браузер.
pub fn parse(wav: &[u8]) -> Option<Pcm> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |o: usize| u16::from_le_bytes([wav[o], wav[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes([wav[o], wav[o + 1], wav[o + 2], wav[o + 3]]);

    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut pos = 12;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        if id == b"fmt " && size >= 16 && body + 16 <= wav.len() {
            let mut format = u16_at(body);
            // WAVE_FORMAT_EXTENSIBLE: настоящий формат — в подформате
            if format == 0xFFFE && size >= 26 && body + 26 <= wav.len() {
                format = u16_at(body + 24);
            }
            fmt = Some((format, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt?;
            if channels == 0 || rate == 0 {
                return None;
            }
            // Браузер, бывает, пишет размер 0 или больше файла у потоковой
            // записи — берём то, что есть
            let end = if size == 0 { wav.len() } else { (body + size).min(wav.len()) };
            let data = &wav[body..end];
            let samples = decode(data, format, bits)?;
            let channels = channels as usize;
            let mono = samples
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                .collect();
            return Some(Pcm { rate, samples: mono });
        }
        // Куски выравниваются по чётной границе
        pos = body + size + (size & 1);
    }
    None
}

fn decode(data: &[u8], format: u16, bits: u16) -> Option<Vec<f32>> {
    Some(match (format, bits) {
        (1, 8) => data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
        (1, 16) => data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
            .collect(),
        (1, 24) => data
            .chunks_exact(3)
            .map(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0)
            .collect(),
        (1, 32) => data
            .chunks_exact(4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0)
            .collect(),
        (3, 32) => data
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
        _ => return None,
    })
}

/// Пересчитать частоту линейной интерполяцией. Для речи в записи
/// разговора этого хватает: это не студийный звук, а телефон.
pub fn resample(pcm: &Pcm, rate: u32) -> Vec<f32> {
    if pcm.rate == rate || pcm.samples.is_empty() {
        return pcm.samples.clone();
    }
    let out_len = (pcm.samples.len() as u64 * rate as u64 / pcm.rate as u64) as usize;
    let step = pcm.rate as f64 / rate as f64;
    let last = pcm.samples.len() - 1;
    (0..out_len)
        .map(|i| {
            let x = i as f64 * step;
            let j = (x as usize).min(last);
            let k = (j + 1).min(last);
            let t = (x - j as f64) as f32;
            pcm.samples[j] * (1.0 - t) + pcm.samples[k] * t
        })
        .collect()
}

/// Свести реплики в один разговор: по порядку, с паузой между ними.
pub fn join(parts: &[Pcm]) -> Vec<f32> {
    let gap = vec![0.0f32; (RATE * GAP_MS / 1000) as usize];
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(&gap);
        }
        out.extend(resample(p, RATE));
    }
    out
}

/// WAV 16 бит моно.
pub fn encode(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &x in samples {
        let v = (x.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn тон(rate: u32, ms: u32) -> Vec<f32> {
        (0..rate * ms / 1000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn wav_читается_обратно_тем_же_звуком() {
        let звук = тон(RATE, 250);
        let pcm = parse(&encode(&звук, RATE)).expect("свой же WAV не разобран");
        assert_eq!(pcm.rate, RATE);
        assert_eq!(pcm.samples.len(), звук.len());
        let ошибка = pcm.samples.iter().zip(&звук).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(ошибка < 1e-3, "искажение {ошибка}");
    }

    #[test]
    fn стерео_сводится_в_моно() {
        // Левый канал 0,5, правый −0,5 — в моно тишина
        let mut wav = encode(&[0.0; 4], 8000);
        wav[22] = 2; // каналов
        let data = 44;
        for (i, v) in [16384i16, -16384, 16384, -16384].iter().enumerate() {
            wav[data + i * 2..data + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        let pcm = parse(&wav).unwrap();
        assert_eq!(pcm.samples.len(), 2);
        assert!(pcm.samples.iter().all(|s| s.abs() < 1e-4));
    }

    #[test]
    fn синтез_44_кгц_сводится_к_16_по_длительности() {
        let синтез = Pcm { rate: 44_100, samples: тон(44_100, 1000) };
        let микрофон = Pcm { rate: RATE, samples: тон(RATE, 500) };
        let разговор = join(&[микрофон, синтез]);
        // 0,5 с + пауза 0,4 с + 1 с
        let мс = разговор.len() as u32 * 1000 / RATE;
        assert!((1890..=1910).contains(&мс), "длительность {мс} мс");
    }

    #[test]
    fn кривой_файл_не_разбирается() {
        assert!(parse(b"").is_none());
        assert!(parse(b"RIFF\0\0\0\0WAVEdata").is_none());
        assert!(parse(b"ID3 mp3, \xd0\xb0 \xd0\xbd\xd0\xb5 wav").is_none());
        // 12-битный PCM нам не встречается — честный отказ
        let mut wav = encode(&[0.0; 4], RATE);
        wav[34] = 12;
        assert!(parse(&wav).is_none());
    }
}
