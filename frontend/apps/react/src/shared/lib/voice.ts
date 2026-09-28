/**
 * Голос в учебном разговоре: запись реплики, распознавание, озвучивание
 * собеседника.
 *
 * Общий для всех учебных разговоров: ДДС со старшим бригады и с
 * вышестоящим начальником (ответ заказчика в чате, 27.09.2026) и
 * оператора 112 с заявителем. Сервер голоса
 * может быть не подключён — тогда `voiceStatus()` отвечает «нет»,
 * а разговор идёт текстом, как без голоса.
 *
 * Звук перекодируется в WAV 16 кГц моно здесь, в браузере: так серверу
 * голоса не нужен ffmpeg, а по сети идёт вчетверо меньше, чем 44,1 кГц.
 */
import { voice, type VoiceRole, type VoiceStatus } from '@/shared/api';

const ASR_RATE = 16_000;
const SOUND_KEY = 'arm112.voice.sound';

// ─── Что подключено ──────────────────────────────────────────────────

let status: Promise<VoiceStatus> | null = null;
const OFF: VoiceStatus = { asr: false, tts: false, voices: [] };

/**
 * Состояние голоса на сервере. Спрашиваем один раз за сеанс: сервер
 * голоса не появляется и не пропадает посреди занятия. Ошибку не
 * запоминаем — вдруг просто моргнула сеть.
 */
export const voiceStatus = (): Promise<VoiceStatus> => {
  if (!status) {
    status = voice.status().catch(() => {
      status = null;
      return OFF;
    });
  }
  return status;
};

/** Микрофон в этом браузере вообще есть. */
export const canRecord = (): boolean =>
  typeof navigator !== 'undefined'
  && !!navigator.mediaDevices?.getUserMedia
  && typeof MediaRecorder !== 'undefined';

/**
 * Почему микрофона нет — для подсказки на неактивной кнопке.
 *
 * Браузер даёт микрофон только защищённой странице: по HTTPS или с
 * localhost. Тренажёр, открытый по http://адрес, микрофона не получит
 * никогда, и прятать кнопку молча значило оставить человека гадать,
 * куда делся голосовой ввод.
 */
export const micBlockedReason = (): string | null => {
  if (canRecord()) return null;
  if (typeof window !== 'undefined' && !window.isSecureContext) {
    return 'Микрофон браузер даёт только по HTTPS — откройте тренажёр по адресу https://…';
  }
  return 'В этом браузере нет записи звука';
};

// ─── Звук вкл/выкл ───────────────────────────────────────────────────

/** Озвучивать ли собеседника. Хранится в браузере; нет хранилища — да. */
export const soundOn = (): boolean => {
  try {
    return localStorage.getItem(SOUND_KEY) !== 'off';
  } catch {
    return true;
  }
};

export const setSoundOn = (on: boolean): void => {
  try {
    localStorage.setItem(SOUND_KEY, on ? 'on' : 'off');
  } catch {
    /* приватный режим: настройка проживёт до перезагрузки */
  }
  if (!on) stopSpeaking();
};

// ─── Запись ──────────────────────────────────────────────────────────

export interface Recording {
  /** Закончить запись и получить WAV 16 кГц моно. */
  stop: () => Promise<Blob>;
  /** Бросить запись без распознавания. */
  cancel: () => void;
}

/** Начать запись с микрофона. Бросает, если доступа к микрофону нет. */
export const startRecording = async (): Promise<Recording> => {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
  });
  const recorder = new MediaRecorder(stream);
  const chunks: Blob[] = [];
  recorder.ondataavailable = (e) => { if (e.data.size) chunks.push(e.data); };
  recorder.start();

  const release = () => stream.getTracks().forEach((t) => t.stop());

  return {
    stop: () => new Promise<Blob>((resolve, reject) => {
      recorder.onstop = async () => {
        release();
        try {
          resolve(await toWav16k(new Blob(chunks, { type: recorder.mimeType })));
        } catch (e) {
          reject(e);
        }
      };
      recorder.stop();
    }),
    cancel: () => {
      recorder.onstop = null;
      if (recorder.state !== 'inactive') recorder.stop();
      release();
    },
  };
};

/** Любой записанный браузером формат → WAV 16 кГц моно, 16 бит. */
const toWav16k = async (blob: Blob): Promise<Blob> => {
  const ctx = new AudioContext();
  let decoded: AudioBuffer;
  try {
    decoded = await ctx.decodeAudioData(await blob.arrayBuffer());
  } finally {
    void ctx.close();
  }
  const length = Math.max(1, Math.ceil(decoded.duration * ASR_RATE));
  const offline = new OfflineAudioContext(1, length, ASR_RATE);
  const src = offline.createBufferSource();
  src.buffer = decoded;
  src.connect(offline.destination);
  src.start();
  const rendered = await offline.startRendering();
  return encodeWav(rendered.getChannelData(0), ASR_RATE);
};

export const encodeWav = (samples: Float32Array, rate: number): Blob => {
  const buf = new ArrayBuffer(44 + samples.length * 2);
  const v = new DataView(buf);
  const text = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) v.setUint8(off + i, s.charCodeAt(i));
  };
  text(0, 'RIFF');
  v.setUint32(4, 36 + samples.length * 2, true);
  text(8, 'WAVE');
  text(12, 'fmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  text(36, 'data');
  v.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const x = Math.max(-1, Math.min(1, samples[i]));
    v.setInt16(44 + i * 2, x < 0 ? x * 0x8000 : x * 0x7fff, true);
  }
  return new Blob([buf], { type: 'audio/wav' });
};

/** Записанное → текст. Пустая строка — ничего не расслышано. */
export const recognize = async (wav: Blob): Promise<string> =>
  (await voice.recognize(wav)).text.trim();

// ─── Озвучивание ─────────────────────────────────────────────────────

/**
 * Голос заявителя — по отчеству из сценария: «-вна», «-чна», «кызы» —
 * женский. Так же сервер выбирает «не понял / не поняла».
 */
export const applicantVoice = (name: string): VoiceRole =>
  name.split(/\s+/).some((w) => /(вна|чна)$/i.test(w) || /^кызы$/i.test(w))
    ? 'applicant_f' : 'applicant_m';

let current: HTMLAudioElement | null = null;
// Доклады повторяются («прибыли на место»): второй раз не качаем
const cache = new Map<string, string>();

export const stopSpeaking = (): void => {
  if (current) {
    current.pause();
    current = null;
  }
};

/**
 * Озвучить реплику собеседника. Звук выключен или голоса нет — тихо
 * ничего не делает: текст реплики и так на экране. Новая реплика
 * обрывает предыдущую — собеседник не говорит сам с собой хором.
 */
export const speak = async (text: string, role: VoiceRole): Promise<void> => {
  if (!text.trim() || !soundOn()) return;
  const st = await voiceStatus();
  if (!st.tts) return;

  const key = `${role}\u0000${text}`;
  let url = cache.get(key);
  if (!url) {
    try {
      url = URL.createObjectURL(await voice.speak(text, role));
    } catch {
      return;
    }
    cache.set(key, url);
  }
  stopSpeaking();
  const audio = new Audio(url);
  current = audio;
  // Браузер может не дать играть без жеста пользователя — это не
  // ошибка разговора, текст на экране остаётся
  await audio.play().catch(() => undefined);
};
