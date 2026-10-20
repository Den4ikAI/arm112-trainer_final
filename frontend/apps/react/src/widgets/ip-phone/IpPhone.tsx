import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ApiError, cards, channel, chief, phone,
  type PhoneCall, type PhoneContact, type PhonePeer, type PhoneView,
} from '@/shared/api';
import { usePhone } from '@/entities/phone/model';
import clsx from '@/shared/lib/clsx';
import { busy as busyTone, click, ringback, ringIncoming } from '@/shared/lib/tones';
import {
  canRecord, micBlockedReason, recognize, setSoundOn, soundOn, speak, startRecording,
  stopSpeaking, type Recording,
} from '@/shared/lib/voice';
import { PhoneLog } from './PhoneLog';
import { mmss } from './time';
import s from './IpPhone.module.scss';

interface Props {
  cardId: string;
  /** Своя служба диспетчера. */
  serviceId: string;
  /** Служба закончила работу: новые вызовы закрыты (сервер — 409). */
  closed: boolean;
  /** Меняется при каждом перечитывании карточки: назначили бригаду,
   *  служба закончила работу — справочник телефона другой. */
  stamp: unknown;
  /** Разговор что-то поменял: доклад бригады получен, начальник принял
   *  доклад — панели карточки перечитываются. */
  onActivity: () => void;
  /** Входящий зазвонил или перестал: кружок телефона на панели
   *  пульсирует и открывает окно телефона сам. */
  onRingingChange?: (ringing: boolean) => void;
}

interface Said {
  who: 'dispatcher' | 'peer';
  text: string;
}

/** Идущий вызов. */
interface Live {
  call: PhoneCall;
  /** `dialing` — гудки исходящего, `ringing` — звонит входящий */
  state: 'dialing' | 'ringing' | 'talking';
  lines: Said[];
  /** Когда сняли трубку — для секундомера. */
  since: number | null;
  /** Доклад старшего, с которым он звонит. */
  report?: string;
}

const errorText = (e: unknown, fallback: string) =>
  e instanceof ApiError ? (e.detail ?? fallback) : fallback;

/**
 * IP-телефон диспетчера ДДС.
 *
 * Обратная связь экспертов 18.10 — «виртуальный IP-телефон для
 * специалиста ДДС»; в ТЗ — «программная эмуляция IP-телефона» и
 * «имитация системой звонка участника событий». Как телефон в АРМ ДДС
 * (инструкция, разделы 2 и 10): статус линии, входящий с АОН, набор
 * номера и телефонная книга, удержание, отключение микрофона, журнал.
 *
 * Абоненты — из справочника службы, который формирует преподаватель
 * (рекомендации финалистам, п. 8): руководитель организации, начальник
 * отдела реагирования, начальник и специалист аварийной бригады и, если
 * преподаватель включил, заявитель по номеру из карточки. Что они
 * говорят, решает сервер (`api::brigade`, `api::chief`, `api::phone`),
 * голос — синтез.
 * Диспетчер говорит в микрофон: нажал — говорит, нажал ещё раз — фраза
 * распознаётся. Нет голоса или микрофона — та же работа текстом.
 * Разговор пишется в журнал вызовов, голос диспетчера — в запись,
 * которую потом слушает преподаватель.
 */
export const IpPhone = ({ cardId, serviceId, closed, stamp, onActivity, onRingingChange }: Props) => {
  const config = usePhone((st) => st.config);
  // Обработчики событий и таймеров живут дольше одной отрисовки
  const cfg = useRef(config);
  useEffect(() => { cfg.current = config; }, [config]);
  const [view, setView] = useState<PhoneView | null>(null);
  const [live, setLiveState] = useState<Live | null>(null);
  const [dialed, setDialed] = useState('');
  const [text, setText] = useState('');
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [recording, setRecording] = useState<Recording | null>(null);
  const [hold, setHold] = useState(false);
  const [muted, setMuted] = useState(false);
  const [sound, setSound] = useState(soundOn);
  const [tab, setTab] = useState<'book' | 'log'>('book');
  const [now, setNow] = useState(0);

  // Состояние вызова нужно и обработчикам, созданным раньше: события
  // сервера, таймеры звонка. Держим копию в ref
  const liveRef = useRef<Live | null>(null);
  // Наружу — только факт «звонит входящий»; колбэк из ref, он нужен
  // обработчикам, созданным раньше
  const ringingCb = useRef(onRingingChange);
  useEffect(() => { ringingCb.current = onRingingChange; });
  const setLive = (next: Live | null | ((prev: Live | null) => Live | null)) => {
    const value = typeof next === 'function' ? next(liveRef.current) : next;
    const rang = liveRef.current?.state === 'ringing';
    liveRef.current = value;
    setLiveState(value);
    const rings = value?.state === 'ringing';
    if (rang !== rings) ringingCb.current?.(rings);
  };
  const tone = useRef<(() => void) | null>(null);
  const timer = useRef<number | null>(null);
  const queue = useRef<string[]>([]);
  const chiefSeen = useRef(0);
  const holdRef = useRef(false);
  const recRef = useRef<Recording | null>(null);

  const stopTone = () => {
    tone.current?.();
    tone.current = null;
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = null;
  };

  // Справочник нужен обработчику входящих, созданному раньше
  const bookRef = useRef<PhoneContact[]>([]);
  useEffect(() => { bookRef.current = view?.phonebook ?? []; }, [view]);

  const reload = useCallback(async () => {
    try {
      setView(await phone.view(cardId));
    } catch {
      /* журнал — справка; без него телефон работает */
    }
  }, [cardId]);

  useEffect(() => { void reload(); }, [reload, stamp]);

  // Секундомер разговора
  useEffect(() => {
    if (live?.state !== 'talking') return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [live?.state]);

  // ─── Реплики ───────────────────────────────────────────────────

  /** Через какой модуль говорит абонент: бригада — по этапам работ,
   *  начальник отдела — доклад с разбором, остальные — ответы телефона. */
  const engineOf = (peer: PhonePeer): 'brigade' | 'chief' | 'phone' => {
    if (peer === 'brigade' || peer === 'brigade_head' || peer === 'brigade_specialist') return 'brigade';
    if (peer === 'chief' || peer === 'response_head') return 'chief';
    return 'phone';
  };
  const speakerOf = (peer: PhonePeer) => (peer === 'brigade_specialist' ? 'specialist' : 'head');

  /** Собеседник сказал: на экран, в журнал и вслух. Доклад бригады
   *  начинается с «Старший бригады 2:» — по телефону он и так на экране.
   *  `stored` — сервер уже записал реплику в журнал сам. */
  const peerSays = (call: PhoneCall, raw: string, stored = false) => {
    const said = raw.replace(/^(Старший|Специалист) бригады\s*№?\s*\d+\s*:\s*/i, '').trim();
    if (!said) return;
    setLive((l) => (l && l.call.id === call.id ? { ...l, lines: [...l.lines, { who: 'peer', text: said }] } : l));
    if (!stored) void phone.line(call.id, 'peer', said).catch(() => undefined);
    if (!holdRef.current) void speak(said, call.voice);
  };

  /** Абонент кладёт трубку сам, договорив. */
  const peerHangsUp = (callId: string, why: string) => {
    window.setTimeout(() => {
      if (liveRef.current?.call.id === callId) void hangUp(why);
    }, 2500);
  };

  /** Новые реплики начальника из его разговора. */
  const chiefLines = (call: PhoneCall, lines: { who: string; text: string }[]) => {
    for (const l of lines.slice(chiefSeen.current)) {
      if (l.who === 'chief') peerSays(call, l.text);
    }
    chiefSeen.current = lines.length;
  };

  // ─── Вызов ─────────────────────────────────────────────────────

  const hangUp = async (why?: string) => {
    const l = liveRef.current;
    if (!l) return;
    stopTone();
    stopSpeaking();
    recRef.current?.cancel();
    recRef.current = null;
    setRecording(null);
    setLive(null);
    setHold(false);
    holdRef.current = false;
    setMuted(false);
    setWaiting(false);
    click();
    if (why) setNote(why);
    // Доклад начальнику кончается, когда кладут трубку: начальник
    // разбирает то, что успели доложить
    if (engineOf(l.call.peer) === 'chief' && l.state === 'talking') {
      await chief.finish(cardId).catch(() => undefined);
    }
    await phone.end(l.call.id).catch(() => undefined);
    await reload();
    onActivity();
    // Пока говорили, звонил старший — теперь его очередь
    const next = queue.current.shift();
    if (next !== undefined) window.setTimeout(() => void incoming(next), 1200);
  };

  const connect = async (call: PhoneCall) => {
    stopTone();
    click();
    try {
      const answered = await phone.answer(call.id);
      setLive((l) => (l ? { ...l, call: answered, state: 'talking', since: Date.now() } : l));
      setWaiting(true);
      const engine = engineOf(call.peer);
      if (engine === 'brigade') {
        // Старший (или специалист) называет, что у бригады сейчас
        const r = await cards.callBrigade(cardId, serviceId, undefined, speakerOf(call.peer));
        peerSays(answered, r.text);
      } else if (engine === 'chief') {
        const v = await chief.start(cardId);
        // Начатый раньше доклад продолжается: начальник повторяет
        // последнее, что сказал, а не весь разговор
        chiefSeen.current = Math.max(0, v.lines.length - 1);
        chiefLines(answered, v.lines);
      } else {
        // Руководитель организации, заявитель: сняли трубку
        const r = await phone.reply(answered.id);
        peerSays(answered, r.text, true);
      }
      onActivity();
    } catch (e) {
      setError(errorText(e, 'Связь прервалась'));
      busyTone();
      void hangUp();
    } finally {
      setWaiting(false);
    }
  };

  const dial = async (contact: PhoneContact) => {
    if (liveRef.current) return;
    setError(null);
    setNote(null);
    if (!contact.available) {
      busyTone();
      setError(`${contact.title}: ${contact.note ?? 'недоступен'}`);
      return;
    }
    try {
      const call = await phone.dial(cardId, contact.peer, 'out');
      setLive({ call, state: 'dialing', lines: [], since: null });
      tone.current = ringback();
      timer.current = window.setTimeout(() => void connect(call),
        Math.max(0, (cfg.current?.answerSec ?? 2) * 1000));
    } catch (e) {
      busyTone();
      setError(errorText(e, 'Вызов не прошёл'));
    }
  };

  const dialNumber = () => {
    const number = dialed.trim();
    if (!number) return;
    // Номер заявителя в карточке — с пробелами и скобками, набирают цифрами
    const contact = view?.phonebook.find((c) => c.number.replace(/\D/g, '') === number);
    if (!contact) {
      busyTone();
      setError(`Номер ${number} не обслуживается`);
      return;
    }
    setDialed('');
    void dial(contact);
  };

  /** Звонит старший бригады с докладом. */
  const incoming = async (report: string) => {
    if (liveRef.current) {
      // Линия занята: вторая линия ждёт, пока положат трубку
      queue.current.push(report);
      setNote('Вторая линия: звонит старший бригады — ответите после разговора');
      return;
    }
    try {
      const call = await phone.dial(cardId, 'brigade_head', 'in');
      setLive({ call, state: 'ringing', lines: [], since: null, report });
      setNote(null);
      tone.current = ringIncoming();
      timer.current = window.setTimeout(() => {
        if (liveRef.current?.call.id !== call.id || liveRef.current.state !== 'ringing') return;
        void hangUp('Пропущенный вызов старшего бригады — его доклад в списке бригады');
      }, (cfg.current?.ringSec ?? 20) * 1000);
    } catch {
      /* служба закончила работу или бригады уже нет — звонить некому */
    }
  };

  const accept = async () => {
    const l = liveRef.current;
    if (!l || l.state !== 'ringing') return;
    stopTone();
    click();
    try {
      const answered = await phone.answer(l.call.id);
      setLive({ ...l, call: answered, state: 'talking', since: Date.now() });
      peerSays(answered, l.report ?? '');
      onActivity();
    } catch (e) {
      setError(errorText(e, 'Вызов уже не звонит'));
      setLive(null);
      void reload();
    }
  };

  const decline = async () => {
    const l = liveRef.current;
    if (!l) return;
    stopTone();
    setLive(null);
    await phone.end(l.call.id, 'declined').catch(() => undefined);
    void reload();
  };

  // Старший бригады звонит сам: события сервера по карточке. Обработчик
  // берём из ref — подписка одна на карточку, а не на каждую отрисовку
  const onReport = useRef(incoming);
  useEffect(() => { onReport.current = incoming; });
  useEffect(() => channel.on((e) => {
    // Звонит начальник бригады — если преподаватель оставил его в справочнике
    if (e.type === 'brigade.report' && e.cardId === cardId && e.serviceId === serviceId
        && bookRef.current.some((c) => c.peer === 'brigade_head')) {
      void onReport.current(String(e.text ?? ''));
    }
  }), [cardId, serviceId]);

  // Ушли с карточки посреди вызова — кладём трубку
  useEffect(() => () => {
    stopTone();
    stopSpeaking();
    recRef.current?.cancel();
    const l = liveRef.current;
    if (l) void phone.end(l.call.id).catch(() => undefined);
  }, []);

  // ─── Диспетчер говорит ─────────────────────────────────────────

  const say = async (said: string, wav?: Blob) => {
    const l = liveRef.current;
    const t = said.trim();
    if (!l || l.state !== 'talking' || !t) return;
    setError(null);
    setLive({ ...l, lines: [...l.lines, { who: 'dispatcher', text: t }] });
    setWaiting(true);
    try {
      const line = await phone.line(l.call.id, 'dispatcher', t);
      if (wav && cfg.current?.record) void phone.lineAudio(l.call.id, line.id, wav).catch(() => undefined);
      const engine = engineOf(l.call.peer);
      if (engine === 'brigade') {
        const r = await cards.callBrigade(cardId, serviceId, t, speakerOf(l.call.peer));
        peerSays(l.call, r.text);
      } else if (engine === 'chief') {
        const v = await chief.say(cardId, t);
        chiefLines(l.call, v.lines);
        // «Принял» — начальник кладёт трубку сам
        if (v.finishedAt) peerHangsUp(l.call.id, 'Начальник принял доклад и положил трубку');
      } else {
        const r = await phone.reply(l.call.id, t);
        peerSays(l.call, r.text, true);
        if (r.finished) peerHangsUp(l.call.id, `${l.call.title} принял доклад и положил трубку`);
      }
      onActivity();
    } catch (e) {
      setError(errorText(e, 'Собеседник не расслышал — повторите'));
    } finally {
      setWaiting(false);
    }
  };

  const talk = async () => {
    if (!recording) {
      try {
        stopSpeaking();
        const rec = await startRecording();
        recRef.current = rec;
        setRecording(rec);
        setError(null);
      } catch {
        setError('Нет доступа к микрофону — говорите текстом');
      }
      return;
    }
    const rec = recording;
    recRef.current = null;
    setRecording(null);
    setWaiting(true);
    try {
      const wav = await rec.stop();
      const heard = await recognize(wav);
      if (!heard) {
        setError('Не расслышано — повторите');
        setWaiting(false);
        return;
      }
      await say(heard, wav);
    } catch (e) {
      setError(errorText(e, 'Распознать не вышло — скажите текстом'));
      setWaiting(false);
    }
  };

  const toggleHold = () => {
    const on = !holdRef.current;
    holdRef.current = on;
    setHold(on);
    if (on) stopSpeaking();
  };

  if (!config?.enabled || !view) return null;

  const voiceTalk = config.asr && canRecord();
  const micBlocked = config.asr ? micBlockedReason() : null;
  const talking = live?.state === 'talking';
  const since = live?.since ? Math.max(0, (now - live.since) / 1000) : 0;
  const status = live
    ? live.state === 'talking' ? 'разговор' : live.state === 'ringing' ? 'входящий вызов' : 'вызов…'
    : 'доступен';

  return (
    <section className={s.phone} aria-label="IP-телефон" data-testid="ip-phone">
      <header className={s.head}>
        <span className={s.name}>IP-телефон</span>
        <span className={s.ext} data-testid="phone-extension">доб. {view.extension}</span>
        <span className={clsx(s.status, live ? s.busy : s.free)} data-testid="phone-status">
          ● {status}
        </span>
        {config.tts && (
          <label className={s.sound}>
            <input type="checkbox" checked={sound} data-testid="phone-sound"
                   onChange={(e) => { setSound(e.target.checked); setSoundOn(e.target.checked); }} />
            звук
          </label>
        )}
      </header>

      {live && (
        <div className={clsx(s.display, live.state === 'ringing' && s.ringing)} data-testid="phone-display">
          <div className={s.line1}>
            {live.state === 'ringing' && <span>Входящий · АОН </span>}
            {live.state === 'dialing' && <span>Вызов · </span>}
            <strong data-testid="phone-number">{live.call.number}</strong>
            {talking && <span className={s.timer} data-testid="phone-timer">{mmss(since)}</span>}
          </div>
          <div className={s.line2}>{live.call.title}{hold && ' · на удержании'}{muted && ' · микрофон выкл.'}</div>
        </div>
      )}

      {live?.state === 'ringing' && (
        <div className={s.row}>
          <button className={s.accept} data-testid="phone-accept" onClick={() => void accept()}>
            Принять
          </button>
          <button className={s.reject} data-testid="phone-decline" onClick={() => void decline()}>
            Отклонить
          </button>
        </div>
      )}

      {live && live.state !== 'ringing' && (
        <>
          {live.lines.length > 0 && (
            <ol className={s.talk} data-testid="phone-lines">
              {live.lines.map((l, i) => (
                <li key={i} className={l.who === 'peer' ? s.peerLine : s.myLine}>
                  <span className={s.who}>{l.who === 'peer' ? live.call.title : 'Вы'}:</span> {l.text}
                </li>
              ))}
              {waiting && talking && <li className={s.muted}>…</li>}
            </ol>
          )}
          {talking && (
            <form className={s.row} onSubmit={(e) => { e.preventDefault(); const t = text; setText(''); void say(t); }}>
              <input className={s.input} value={text} name="phone-say" placeholder="Сказать текстом…"
                     aria-label="Реплика" disabled={waiting || hold || !!recording}
                     onChange={(e) => setText(e.target.value)} data-testid="phone-text" />
              <button className={s.btn} type="submit" data-testid="phone-send"
                      disabled={waiting || hold || !!recording || !text.trim()}>
                Сказать
              </button>
            </form>
          )}
          <div className={s.row}>
            {talking && voiceTalk && (
              <button className={clsx(s.btn, s.mic, recording && s.recOn)} data-testid="phone-talk"
                      disabled={(waiting && !recording) || hold || muted} aria-pressed={!!recording}
                      title="Нажмите и говорите, нажмите ещё раз — фраза уйдёт собеседнику"
                      onClick={() => void talk()}>
                {recording ? '■ Закончить фразу' : '🎤 Говорить'}
              </button>
            )}
            {talking && micBlocked && (
              <button className={s.btn} disabled title={micBlocked} data-testid="phone-talk-blocked">
                🎤 Говорить
              </button>
            )}
            {talking && (
              <>
                <button className={clsx(s.btn, hold && s.on)} aria-pressed={hold} data-testid="phone-hold"
                        onClick={toggleHold}>Удержание</button>
                <button className={clsx(s.btn, muted && s.on)} aria-pressed={muted} data-testid="phone-mute"
                        onClick={() => { recRef.current?.cancel(); recRef.current = null; setRecording(null); setMuted((m) => !m); }}>
                  Микрофон выкл.
                </button>
              </>
            )}
            <button className={s.reject} data-testid="phone-hangup" onClick={() => void hangUp()}>
              {talking ? 'Положить трубку' : 'Отбой'}
            </button>
          </div>
        </>
      )}

      {!live && (
        <>
          <div className={s.tabs} role="tablist">
            <button role="tab" aria-selected={tab === 'book'} className={clsx(s.tab, tab === 'book' && s.tabOn)}
                    data-testid="phone-tab-book" onClick={() => setTab('book')}>Телефонная книга</button>
            <button role="tab" aria-selected={tab === 'log'} className={clsx(s.tab, tab === 'log' && s.tabOn)}
                    data-testid="phone-tab-log" onClick={() => setTab('log')}>
              Журнал вызовов{view.calls.length > 0 && ` (${view.calls.length})`}
            </button>
          </div>
          {tab === 'book' ? (
            <>
              <ul className={s.book}>
                {view.phonebook.map((c) => (
                  <li key={c.peer} className={s.contact}>
                    <span>
                      {c.title}
                      <span className={s.number}> {c.number}</span>
                      {c.note && <span className={s.muted}> — {c.note}</span>}
                    </span>
                    <button className={s.call} data-testid={`phone-call-${c.peer}`}
                            disabled={!c.available || (closed && engineOf(c.peer) === 'brigade')}
                            title={c.note ?? `Позвонить: ${c.number}`}
                            onClick={() => void dial(c)}>
                      📞 Вызов
                    </button>
                  </li>
                ))}
              </ul>
              <form className={s.row} onSubmit={(e) => { e.preventDefault(); dialNumber(); }}>
                <input className={s.input} value={dialed} inputMode="numeric" placeholder="Номер"
                       aria-label="Набрать номер" data-testid="phone-dial"
                       onChange={(e) => setDialed(e.target.value.replace(/[^\d]/g, '').slice(0, 11))} />
                <button className={s.btn} type="submit" disabled={!dialed} data-testid="phone-dial-go">
                  Набрать
                </button>
              </form>
            </>
          ) : (
            <PhoneLog calls={view.calls} />
          )}
        </>
      )}

      {note && <p className={s.note} role="status" data-testid="phone-note">{note}</p>}
      {error && <p className={s.error} role="alert">{error}</p>}
      {!config.tts && (
        <p className={s.muted}>Голос не подключён — собеседник отвечает текстом.</p>
      )}
    </section>
  );
};
