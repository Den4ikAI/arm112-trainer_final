import { useEffect, useRef, useState, type FormEvent } from 'react';
import { ApiError, type AskFreeResult, type ScenarioPublic, type VoiceStatus } from '@/shared/api';
import clsx from '@/shared/lib/clsx';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import { fmt } from '@/shared/lib/useTimer';
import {
  applicantVoice, canRecord, micBlockedReason, recognize, setSoundOn, soundOn, speak, startRecording,
  stopSpeaking, voiceStatus, type Recording,
} from '@/shared/lib/voice';
import s from './TrainerHud.module.scss';

interface Props {
  scenario: ScenarioPublic;
  revealed: string[];
  transcript: Array<{ who: string; text: string; atSec: number }>;
  elapsedSec: number;
  onAsk: (factId: string) => void;
  /** Вопрос своими словами — текстом или распознанный с голоса. */
  onAskFree: (text: string) => Promise<AskFreeResult | null>;
}

type Mode = 'buttons' | 'free';
const MODE_KEY = 'arm112.hud.mode';

const loadMode = (): Mode => {
  try {
    return localStorage.getItem(MODE_KEY) === 'free' ? 'free' : 'buttons';
  } catch {
    return 'buttons';
  }
};

const saveMode = (m: Mode) => {
  try {
    localStorage.setItem(MODE_KEY, m);
  } catch {
    /* приватный режим: выбор проживёт до перезагрузки */
  }
};

const errorText = (e: unknown) =>
  e instanceof ApiError ? (e.detail ?? 'Не удалось') : 'Нет связи с сервером';

/**
 * Панель обучающегося — единственный элемент, которого НЕТ в боевой системе.
 *
 * Здесь реализован ключевой приём тренажёра: заявитель не выкладывает всё сразу.
 * Часть фактов он сообщает только в ответ на конкретный вопрос — ровно так,
 * как это заложено в бумажных билетах, где настоящий адрес напечатан в скобках,
 * а заявитель называет лишь ориентир. Поле, заполненное без вопроса,
 * при разборе будет помечено как «угадал», а не «выяснил».
 *
 * Спросить можно кнопкой из списка или своими словами — напечатать или
 * сказать в микрофон (заказчик 27.09: разговор с заявителем — «будет
 * плюсом»). Свой вопрос тренажёр сопоставляет с фактами вызова; ответ
 * заявителя всегда тот, что записан в сценарии, а при подключённом
 * голосе ещё и звучит.
 */
export const TrainerHud = ({ scenario, revealed, transcript, elapsedSec, onAsk, onAskFree }: Props) => {
  const [collapsed, setCollapsed] = useState(false);
  const [mode, setMode] = useState<Mode>(loadMode);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [voiceSt, setVoiceSt] = useState<VoiceStatus | null>(null);
  const [recording, setRecording] = useState<Recording | null>(null);
  const [sound, setSound] = useState(soundOn);
  const over = elapsedSec > scenario.normSec;
  const asked = revealed.length;
  const total = scenario.questions.length;
  const role = applicantVoice(scenario.applicantName);

  useEffect(() => {
    let alive = true;
    void voiceStatus().then((st) => { if (alive) setVoiceSt(st); });
    return () => { alive = false; };
  }, []);

  // Ушли с карточки — запись и голос обрываем
  const live = useRef<Recording | null>(null);
  useEffect(() => () => {
    live.current?.cancel();
    stopSpeaking();
  }, []);

  // Озвучиваем только то, что прозвучало при нас: после F5 восстановленный
  // разговор целиком заново не зачитываем. Свежий вызов — одна легенда,
  // её и говорим: заявитель начинает разговор
  const spoken = useRef<number | null>(null);
  useEffect(() => {
    if (spoken.current === null) {
      spoken.current = transcript.length === 1 ? 0 : transcript.length;
    }
    for (let i = spoken.current; i < transcript.length; i++) {
      if (transcript[i].who === 'заявитель') void speak(transcript[i].text, role);
    }
    spoken.current = transcript.length;
  }, [transcript, role]);

  const switchMode = (m: Mode) => {
    setMode(m);
    saveMode(m);
    setError('');
  };

  const toggleSound = (on: boolean) => {
    setSound(on);
    setSoundOn(on);
  };

  const askFree = async (question: string) => {
    const q = question.trim();
    if (!q) return;
    setBusy(true);
    try {
      await onAskFree(q);
      setText('');
      setError('');
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void askFree(text);
  };

  // Нажал — говорит, нажал ещё раз — вопрос распознаётся и уходит.
  // Не расслышали — ничего не спрошено, можно повторить
  const talk = async () => {
    if (!recording) {
      try {
        stopSpeaking();
        const rec = await startRecording();
        live.current = rec;
        setRecording(rec);
        setError('');
      } catch {
        setError('Нет доступа к микрофону — напечатайте вопрос');
      }
      return;
    }
    const rec = recording;
    live.current = null;
    setRecording(null);
    setBusy(true);
    let heard = '';
    try {
      heard = await recognize(await rec.stop());
    } catch (e) {
      setBusy(false);
      setError(errorText(e) + ' — напечатайте вопрос');
      return;
    }
    setBusy(false);
    if (!heard) {
      setError('Не расслышано — повторите');
      return;
    }
    await askFree(heard);
  };

  const voiceTalk = !!voiceSt?.asr && canRecord();
  const micBlocked = voiceSt?.asr ? micBlockedReason() : null;

  return (
    <aside className={clsx(s.hud, collapsed && s.collapsed)} aria-label="Панель обучающегося">
      <header className={s.head}>
        <div>
          <div className={s.badge}>{scenarioTitle(scenario)}</div>
          <div className={s.sub}>{scenario.applicantName}, {scenario.applicantPhone}</div>
        </div>
        <div className={s.headRight}>
          {voiceSt?.tts && (
            <label className={s.sound}>
              <input type="checkbox" data-testid="hud-sound" checked={sound}
                     onChange={(e) => toggleSound(e.target.checked)} />
              звук
            </label>
          )}
          <button className={s.toggle} onClick={() => setCollapsed((v) => !v)}
                  aria-expanded={!collapsed}>
            {collapsed ? '▸' : '▾'}
          </button>
        </div>
      </header>

      {!collapsed && (
        <div className={s.body}>
          <div className={clsx(s.norm, over && s.normOver)}>
            <span>Норматив {fmt(scenario.normSec)}</span>
            <strong data-testid="hud-elapsed">{fmt(elapsedSec)}</strong>
          </div>

          <div className={s.sectionTitle}>Разговор</div>
          <ol className={s.transcript} data-testid="transcript">
            {transcript.map((line, i) => (
              <li key={i} className={clsx(s.line, line.who === 'оператор' && s.mine)}>
                <span className={s.who}>{line.who}</span>
                <span className={s.text}>{line.text}</span>
              </li>
            ))}
          </ol>

          <div className={s.sectionTitle}>
            Задать вопрос <span className={s.counter}>{asked} / {total}</span>
          </div>

          <div className={s.modes} role="tablist" aria-label="Как спрашивать">
            <button role="tab" aria-selected={mode === 'buttons'} data-testid="hud-mode-buttons"
                    className={clsx(s.modeBtn, mode === 'buttons' && s.modeOn)}
                    onClick={() => switchMode('buttons')}>
              Из списка
            </button>
            <button role="tab" aria-selected={mode === 'free'} data-testid="hud-mode-free"
                    className={clsx(s.modeBtn, mode === 'free' && s.modeOn)}
                    onClick={() => switchMode('free')}>
              Своими словами
            </button>
          </div>

          {mode === 'buttons' ? (
            <div className={s.questions}>
              {scenario.questions.map((q) => (
                <button
                  key={q.factId}
                  className={clsx(s.question, revealed.includes(q.factId) && s.asked)}
                  disabled={revealed.includes(q.factId)}
                  data-testid={`ask-${q.key}`}
                  onClick={() => onAsk(q.factId)}
                >
                  {q.question}
                </button>
              ))}
            </div>
          ) : (
            <form className={s.free} onSubmit={submit}>
              <input
                className={s.input} data-testid="hud-free-input" value={text}
                placeholder="Спросите заявителя" aria-label="Вопрос заявителю"
                disabled={busy || !!recording} maxLength={300}
                onChange={(e) => setText(e.target.value)}
              />
              <div className={s.freeRow}>
                <button type="submit" className={s.send} data-testid="hud-free-send"
                        disabled={busy || !!recording || !text.trim()}>
                  Спросить
                </button>
                {voiceTalk && (
                  <button type="button" className={clsx(s.send, recording && s.live)}
                          data-testid="hud-talk" disabled={busy} aria-pressed={!!recording}
                          title="Спросить голосом: нажмите, говорите, нажмите ещё раз"
                          onClick={() => void talk()}>
                    {recording ? '■ Закончить' : '🎤 Голосом'}
                  </button>
                )}
                {micBlocked && (
                  <button type="button" className={s.send} disabled title={micBlocked}
                          data-testid="hud-talk-blocked">🎤 Голосом</button>
                )}
              </div>
              {busy && <div className={s.hint} role="status">Заявитель слушает…</div>}
            </form>
          )}

          {error && <div className={s.error} role="alert" data-testid="hud-error">{error}</div>}
        </div>
      )}
    </aside>
  );
};
