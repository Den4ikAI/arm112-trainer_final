import { useEffect, useState } from 'react';
import { ApiError, phone, type PhoneCall } from '@/shared/api';
import clsx from '@/shared/lib/clsx';
import { mmss } from './time';
import s from './IpPhone.module.scss';

const hhmmss = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit', second: '2-digit' });


/** Чем кончился вызов — словами журнала. */
const outcomeText = (c: PhoneCall) => {
  switch (c.outcome) {
    case 'ended': return `разговор ${mmss(c.durationSec ?? 0)}`;
    case 'missed': return c.direction === 'in' ? 'пропущен' : 'без ответа';
    case 'declined': return 'сброшен';
    default: return 'идёт';
  }
};

/** Запись разговора: качается по нажатию, играет здесь же, сохраняется файлом. */
const Recording = ({ call }: { call: PhoneCall }) => {
  const [url, setUrl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);

  if (url) {
    const name = `vyzov-${call.number}-${call.startedAt.slice(0, 19).replace(/[-:T]/g, '')}.wav`;
    return (
      <span className={s.rec}>
        <audio src={url} controls autoPlay data-testid="phone-recording-audio" />
        <a href={url} download={name} className={s.link}>WAV</a>
      </span>
    );
  }
  return (
    <span className={s.rec}>
      <button type="button" className={s.link} disabled={busy} data-testid="phone-recording"
              onClick={() => {
                setBusy(true);
                setError(null);
                void phone.recording(call.id)
                  .then((b) => setUrl(URL.createObjectURL(b)))
                  .catch((e) => setError(e instanceof ApiError ? (e.detail ?? 'записи нет') : 'записи нет'))
                  .finally(() => setBusy(false));
              }}>
        {busy ? 'готовлю запись…' : '▶ запись'}
      </button>
      {error && <span className={s.muted}> {error}</span>}
    </span>
  );
};

/**
 * Журнал вызовов: кто, кому, когда, чем кончилось, что прозвучало и
 * запись разговора. Диспетчер видит свои вызовы, преподаватель — всех
 * (`showDispatcher`).
 */
export const PhoneLog = ({ calls, showDispatcher = false }: {
  calls: PhoneCall[];
  showDispatcher?: boolean;
}) => {
  if (!calls.length) return <p className={s.muted}>Вызовов по этой карточке не было.</p>;
  return (
    <ol className={s.log} data-testid="phone-log">
      {calls.map((c) => (
        <li key={c.id} data-testid="phone-log-row"
            className={clsx(s.logRow, (c.outcome === 'missed' || c.outcome === 'declined') && s.missed)}>
          <div className={s.logHead}>
            <span className={s.dir} title={c.direction === 'in' ? 'входящий' : 'исходящий'}>
              {c.direction === 'in' ? '↙' : '↗'}
            </span>
            <span className={s.when}>{hhmmss(c.startedAt)}</span>
            <span>{c.title} · {c.number}</span>
            <span className={s.outcome} data-testid="phone-log-outcome">{outcomeText(c)}</span>
            {c.recording && <Recording call={c} />}
          </div>
          {showDispatcher && <div className={s.muted}>Диспетчер: {c.dispatcher}</div>}
          {c.lines.length > 0 && (
            <details className={s.transcript}>
              <summary>разговор, реплик: {c.lines.length}</summary>
              {c.lines.map((l) => (
                <div key={l.id} className={l.who === 'peer' ? s.peerLine : s.myLine}>
                  <span className={s.who}>{l.who === 'peer' ? c.title : 'Диспетчер'}:</span> {l.text}
                  {l.hasAudio && <span className={s.muted} title="голос сохранён"> 🎙</span>}
                </div>
              ))}
            </details>
          )}
        </li>
      ))}
    </ol>
  );
};
