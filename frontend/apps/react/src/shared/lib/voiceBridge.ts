/**
 * Тонкая прослойка к голосу: распознавание речи диспетчера и озвучка
 * собеседника.
 *
 * Сервер голоса — отдельная работа; договорённость о контракте такая:
 * `POST /api/voice/asr` принимает WAV 16 кГц моно и отдаёт `{ text }`,
 * `POST /api/voice/tts` принимает `{ text, voice }` и отдаёт audio/wav.
 * Голоса собеседников — `brigade` и `chief`. Нет голоса (404 — маршрута
 * нет, 503 — модель не поднята) — всё работает текстом, а прослойка
 * запоминает это до перезагрузки и больше сервер не дёргает.
 */
import { getToken } from '@/shared/api';

export type Voice = 'brigade' | 'chief';

let unavailable = false;

/** Голос недоступен: звать сервер бесполезно. */
export const voiceUnavailable = () => unavailable;

const authHeaders = (): Record<string, string> => {
  const t = getToken();
  return t ? { Authorization: `Bearer ${t}` } : {};
};

const gone = (status: number) => status === 404 || status === 501 || status === 503;

/** Озвучить реплику. `false` — голоса нет, реплика остаётся текстом. */
export const speak = async (text: string, voice: Voice): Promise<boolean> => {
  if (unavailable || !text.trim()) return false;
  try {
    const res = await fetch('/api/voice/tts', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...authHeaders() },
      body: JSON.stringify({ text, voice }),
    });
    if (!res.ok) {
      if (gone(res.status)) unavailable = true;
      return false;
    }
    const url = URL.createObjectURL(await res.blob());
    const audio = new Audio(url);
    audio.onended = () => URL.revokeObjectURL(url);
    await audio.play();
    return true;
  } catch {
    return false;
  }
};

/** Распознать запись. `null` — голоса нет или распознать не вышло. */
export const transcribe = async (wav: Blob): Promise<string | null> => {
  if (unavailable) return null;
  try {
    const res = await fetch('/api/voice/asr', {
      method: 'POST',
      headers: { 'Content-Type': 'audio/wav', ...authHeaders() },
      body: wav,
    });
    if (!res.ok) {
      if (gone(res.status)) unavailable = true;
      return null;
    }
    const data = (await res.json()) as { text?: string };
    return data.text?.trim() || null;
  } catch {
    return null;
  }
};

// ─── Запись с микрофона в WAV 16 кГц моно ─────────────────────────────

const RATE = 16_000;

/** PCM16 WAV из моно-сэмплов. Заголовок — 44 байта по стандарту RIFF. */
const encodeWav = (samples: Float32Array): Blob => {
  const buf = new ArrayBuffer(44 + samples.length * 2);
  const v = new DataView(buf);
  const str = (o: number, s: string) => [...s].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
  str(0, 'RIFF');
  v.setUint32(4, 36 + samples.length * 2, true);
  str(8, 'WAVE');
  str(12, 'fmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, RATE, true);
  v.setUint32(28, RATE * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  str(36, 'data');
  v.setUint32(40, samples.length * 2, true);
  samples.forEach((x, i) => {
    const c = Math.max(-1, Math.min(1, x));
    v.setInt16(44 + i * 2, c < 0 ? c * 0x8000 : c * 0x7fff, true);
  });
  return new Blob([buf], { type: 'audio/wav' });
};

/** Сжатую запись браузера — в моно 16 кГц: так ждёт распознавание. */
const toWav = async (recorded: Blob): Promise<Blob> => {
  const ctx = new AudioContext();
  try {
    const decoded = await ctx.decodeAudioData(await recorded.arrayBuffer());
    const length = Math.ceil(decoded.duration * RATE);
    const offline = new OfflineAudioContext(1, Math.max(length, 1), RATE);
    const src = offline.createBufferSource();
    src.buffer = decoded;
    src.connect(offline.destination);
    src.start();
    const rendered = await offline.startRendering();
    return encodeWav(rendered.getChannelData(0));
  } finally {
    void ctx.close();
  }
};

/** Запись с микрофона: `start` — снять трубку, `stop` — WAV. */
export class Recorder {
  private rec: MediaRecorder | null = null;
  private chunks: Blob[] = [];
  private stream: MediaStream | null = null;

  /** `false` — микрофона нет или доступ не дали. */
  async start(): Promise<boolean> {
    if (unavailable || !navigator.mediaDevices?.getUserMedia) return false;
    try {
      this.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch {
      return false;
    }
    this.chunks = [];
    this.rec = new MediaRecorder(this.stream);
    this.rec.ondataavailable = (e) => { if (e.data.size) this.chunks.push(e.data); };
    this.rec.start();
    return true;
  }

  async stop(): Promise<Blob | null> {
    const rec = this.rec;
    if (!rec) return null;
    const done = new Promise<void>((resolve) => { rec.onstop = () => resolve(); });
    rec.stop();
    await done;
    this.stream?.getTracks().forEach((t) => t.stop());
    this.rec = null;
    this.stream = null;
    if (!this.chunks.length) return null;
    try {
      return await toWav(new Blob(this.chunks, { type: rec.mimeType }));
    } catch {
      return null;
    }
  }
}
