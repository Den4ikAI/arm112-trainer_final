/**
 * Сигналы телефона: вызов, гудки, «занято». Синтезируются в браузере —
 * файлов со звуком тренажёру не нужно.
 *
 * Гудки — 425 Гц, как в телефонной сети России: длинный 1 с через 4 с
 * («вызов абонента») и короткие 0,35 с («занято»). Звонок входящего —
 * двойная трель, чтобы его нельзя было спутать с гудками.
 *
 * Браузер может не дать звука без жеста пользователя — тогда тихо:
 * вызов всё равно виден на экране.
 */
import { soundOn } from './voice';

let ctx: AudioContext | null = null;

const audio = (): AudioContext | null => {
  if (typeof AudioContext === 'undefined') return null;
  if (!ctx) ctx = new AudioContext();
  if (ctx.state === 'suspended') void ctx.resume().catch(() => undefined);
  return ctx;
};

/** Один тон: частоты (Гц), начало и длительность (с) от `at`. */
const beep = (c: AudioContext, out: AudioNode, freqs: number[], at: number, dur: number,
  volume = 0.12) => {
  const gain = c.createGain();
  gain.gain.setValueAtTime(0, at);
  gain.gain.linearRampToValueAtTime(volume, at + 0.01);
  gain.gain.setValueAtTime(volume, at + dur - 0.02);
  gain.gain.linearRampToValueAtTime(0, at + dur);
  gain.connect(out);
  for (const f of freqs) {
    const osc = c.createOscillator();
    osc.frequency.value = f;
    osc.connect(gain);
    osc.start(at);
    osc.stop(at + dur);
  }
};

/** Повторять рисунок, пока не остановят. Возвращает «стоп». */
const loop = (pattern: (c: AudioContext, out: AudioNode, t: number) => void, period: number)
  : (() => void) => {
  const c = soundOn() ? audio() : null;
  if (!c) return () => undefined;
  // Свой выход у каждого сигнала: остановить — значит отключить его,
  // вместе с тем, что уже запланировано наперёд
  const out = c.createGain();
  out.connect(c.destination);
  let next = c.currentTime + 0.05;
  const schedule = () => {
    // Планируем на секунду вперёд: таймеры вкладки в фоне неточны
    while (next < c.currentTime + 1) {
      pattern(c, out, next);
      next += period;
    }
  };
  schedule();
  const timer = window.setInterval(schedule, 250);
  return () => {
    window.clearInterval(timer);
    out.disconnect();
  };
};

/** Входящий вызов: двойная трель раз в 3 секунды. */
export const ringIncoming = () => loop((c, out, t) => {
  for (let i = 0; i < 2; i += 1) {
    const at = t + i * 0.45;
    for (let k = 0; k < 8; k += 1) {
      beep(c, out, [k % 2 ? 1300 : 1050], at + k * 0.05, 0.05, 0.08);
    }
  }
}, 3);

/** Гудки исходящего: «вызов абонента». */
export const ringback = () => loop((c, out, t) => beep(c, out, [425], t, 1), 5);

/** «Занято» или «абонент недоступен»: четыре коротких. */
export const busy = () => {
  const c = soundOn() ? audio() : null;
  if (!c) return;
  for (let i = 0; i < 4; i += 1) beep(c, c.destination, [425], c.currentTime + 0.05 + i * 0.7, 0.35);
};

/** Щелчок: трубку сняли или положили. */
export const click = () => {
  const c = soundOn() ? audio() : null;
  if (!c) return;
  beep(c, c.destination, [1800], c.currentTime + 0.01, 0.03, 0.05);
};
