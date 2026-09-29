import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError, cards, channel, type Brigade, type DdsFindingKind, type VoiceStatus } from '@/shared/api';
import clsx from '@/shared/lib/clsx';
import {
  canRecord, micBlockedReason, recognize, setSoundOn, soundOn, speak, startRecording, stopSpeaking,
  voiceStatus, type Recording,
} from '@/shared/lib/voice';
import s from './BrigadePanel.module.scss';

interface Props {
  cardId: string;
  /** Своя служба диспетчера. */
  serviceId: string;
  /** Меняется при каждом перечитывании карточки: новый статус меняет разбор. */
  stamp: unknown;
  /** Служба закончила работу («Работы завершены» или «Отказ»): звонить
   *  старшему незачем, сервер такой звонок отклоняет. */
  closed?: boolean;
}

const hhmmss = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit', second: '2-digit' });

const KIND: Record<DdsFindingKind, string> = {
  ok: 'по факту',
  late: 'с опозданием',
  notByFact: 'не по факту',
  missed: 'статус не проставлен',
  noBrigade: 'бригада не назначена',
  openedLate: 'карточка открыта позже 30 с',
  firstRecordLate: 'первая запись позже 3 мин',
  firstRecordNoText: 'первая запись без текста',
};

/** Замечания по приёму карточки считаются от её поступления, а не от доклада. */
const INTAKE: DdsFindingKind[] = ['openedLate', 'firstRecordLate', 'firstRecordNoText'];

const errorText = (e: unknown) =>
  e instanceof ApiError ? (e.detail ?? 'Не удалось') : 'Нет связи с сервером';

/**
 * Связь диспетчера ДДС со своей бригадой.
 *
 * Заказчик описал два варианта, и работают оба (чат задачи, 21.09.2026,
 * сообщение 691): старший сам звонит в ДДС с докладом, или диспетчер
 * сам набирает старшего и уточняет ход работ. Статусы после этого
 * диспетчер ставит сам, в плашке службы, — по факту полученных сведений.
 *
 * Разговор голосом, если подключён сервер голоса: диспетчер говорит
 * в микрофон, реплика распознаётся и уходит вместе со звонком, ответ
 * старшего и его доклады озвучиваются. Нет голоса или микрофона — всё
 * то же текстом: голос дополняет занятие, а не держит его.
 */
export const BrigadePanel = ({ cardId, serviceId, stamp, closed = false }: Props) => {
  const [brigade, setBrigade] = useState<Brigade | null>(null);
  const [choice, setChoice] = useState(1);
  const [answer, setAnswer] = useState('');
  const [incoming, setIncoming] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [voiceSt, setVoiceSt] = useState<VoiceStatus | null>(null);
  const [recording, setRecording] = useState<Recording | null>(null);
  const [said, setSaid] = useState('');
  const [sound, setSound] = useState(soundOn);

  useEffect(() => {
    let alive = true;
    void voiceStatus().then((st) => { if (alive) setVoiceSt(st); });
    return () => { alive = false; };
  }, []);

  // Ушли с карточки — запись и озвучивание обрываем. Запись держим
  // ещё и в ref: очистка эффекта по `recording` сработала бы и при
  // обычной остановке и оборвала бы её посреди распознавания
  const live = useRef<Recording | null>(null);
  useEffect(() => () => {
    live.current?.cancel();
    stopSpeaking();
  }, []);

  const reload = useCallback(async () => {
    try {
      setBrigade(await cards.brigade(cardId, serviceId));
    } catch (e) {
      setError(errorText(e));
    }
  }, [cardId, serviceId]);

  useEffect(() => { void reload(); }, [reload, stamp]);

  // Входящий звонок старшего. Текст показываем сразу из события, а
  // список докладов и разбор всё равно перечитываются через REST
  useEffect(() => channel.on((e) => {
    if (e.type === 'brigade.report' && e.cardId === cardId && e.serviceId === serviceId) {
      const text = String(e.text ?? '');
      setIncoming(text);
      void speak(text, 'brigade');
      void reload();
    }
  }), [cardId, serviceId, reload]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      setError('');
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const assign = () => run(async () => {
    setBrigade(await cards.assignBrigade(cardId, serviceId, choice));
  });

  const call = (spoken?: string) => run(async () => {
    const r = await cards.callBrigade(cardId, serviceId, spoken);
    setAnswer(r.text);
    setIncoming('');
    setBrigade(r.brigade);
    void speak(r.text, 'brigade');
  });

  // Нажал — говорит, нажал ещё раз — реплика распознаётся и звонок
  // уходит вместе с ней. Не расслышали — звонка нет, можно повторить
  const talk = async () => {
    if (!recording) {
      try {
        stopSpeaking();
        const rec = await startRecording();
        live.current = rec;
        setRecording(rec);
        setError('');
      } catch {
        setError('Нет доступа к микрофону — позвоните кнопкой');
      }
      return;
    }
    const rec = recording;
    live.current = null;
    setRecording(null);
    setBusy(true);
    let text = '';
    try {
      text = await recognize(await rec.stop());
    } catch (e) {
      setBusy(false);
      setError(errorText(e) + ' — позвоните кнопкой');
      return;
    }
    setBusy(false);
    if (!text) {
      setError('Не расслышано — повторите');
      return;
    }
    setSaid(text);
    await call(text);
  };

  const toggleSound = (on: boolean) => {
    setSound(on);
    setSoundOn(on);
  };

  const voiceTalk = !!voiceSt?.asr && canRecord();
  const micBlocked = voiceSt?.asr ? micBlockedReason() : null;

  if (!brigade) return null;
  const { assignment, reports, debrief } = brigade;

  return (
    <section className={s.panel} data-testid="brigade-panel" aria-label="Бригада службы">
      <div className={s.head}>
        Бригада
        {assignment && (
          <span className={s.assigned}>
            {' '}№ {assignment.brigadeNo}, направлена в {hhmmss(assignment.assignedAt)}
          </span>
        )}
        {voiceSt?.tts && (
          <label className={s.sound}>
            <input type="checkbox" data-testid="brigade-sound" checked={sound}
                   onChange={(e) => toggleSound(e.target.checked)} />
            звук
          </label>
        )}
      </div>

      {!assignment && (brigade.canAssign ? (
        <div className={s.row}>
          <select className={s.select} data-testid="brigade-select" value={choice}
                  aria-label="Учебная бригада"
                  onChange={(e) => setChoice(Number(e.target.value))}>
            {Array.from({ length: brigade.brigadeCount }, (_, i) => i + 1).map((n) => (
              <option key={n} value={n}>Бригада {n}</option>
            ))}
          </select>
          <button className={s.btn} data-testid="brigade-assign" disabled={busy}
                  onClick={() => void assign()}>
            Направить бригаду
          </button>
        </div>
      ) : (
        <p className={s.muted}>Бригаду направляют после статуса «Принята».</p>
      ))}

      {assignment && (
        <div className={s.row}>
          <button className={s.btn} data-testid="brigade-call"
                  disabled={busy || !!recording || closed}
                  title={closed ? 'Служба закончила работу по карточке' : undefined}
                  onClick={() => void call()}>
            Позвонить старшему
          </button>
          {voiceTalk && (
            <button className={clsx(s.btn, recording && s.live)} data-testid="brigade-talk"
                    disabled={busy || (closed && !recording)} aria-pressed={!!recording}
                    title="Сказать старшему голосом: нажмите, говорите, нажмите ещё раз"
                    onClick={() => void talk()}>
              {recording ? '■ Закончить' : '🎤 Сказать голосом'}
            </button>
          )}
          {micBlocked && (
            <button className={s.btn} disabled title={micBlocked}
                    data-testid="brigade-talk-blocked">🎤 Сказать голосом</button>
          )}
        </div>
      )}

      {assignment && (said || answer) && (
        <div className={s.dialog}>
          {said && <div data-testid="brigade-said"><span className={s.who}>Вы:</span> {said}</div>}
          {answer && <div className={s.answer} data-testid="brigade-answer">{answer}</div>}
        </div>
      )}

      {incoming && (
        <div className={s.incoming} data-testid="brigade-incoming" role="status">
          Входящий звонок. {incoming}
        </div>
      )}

      {reports.length > 0 && (
        <ul className={s.reports}>
          {reports.map((r) => (
            <li key={r.stage} data-testid="brigade-report">
              {r.deliveredAt && <span className={s.when}>{hhmmss(r.deliveredAt)}</span>}
              {r.text}
              <span className={s.via}>
                {r.channel === 'incoming' ? ' — старший позвонил сам'
                  : r.channel === 'outgoing' ? ' — по вашему звонку' : ' — ещё не доложено'}
              </span>
            </li>
          ))}
        </ul>
      )}

      {error && <p className={s.error}>{error}</p>}

      <div className={s.debrief}>
        <div data-testid="dds-score">
          Разбор диспетчера:{' '}
          <strong>{debrief.score == null ? 'оценивать пока нечего' : `${debrief.score} из 100`}</strong>
          {debrief.pending > 0 && <span className={s.muted}> · этапов впереди: {debrief.pending}</span>}
        </div>
        {debrief.findings.map((f) => (
          <div key={`${f.kind}-${f.stage}`} data-testid="dds-finding"
               className={clsx(s.finding, f.kind === 'ok' ? s.good : s.bad)}>
            <strong>{f.stage}</strong> — {KIND[f.kind]}
            {f.lagSec != null && f.kind !== 'notByFact'
              && `, через ${f.lagSec} с после ${INTAKE.includes(f.kind) ? 'поступления' : 'доклада'}`}
            {f.consequence && <div className={s.consequence}>{f.consequence}</div>}
          </div>
        ))}
      </div>
    </section>
  );
};
