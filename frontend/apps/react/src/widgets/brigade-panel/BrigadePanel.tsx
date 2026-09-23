import { useCallback, useEffect, useState } from 'react';
import { ApiError, cards, channel, type Brigade, type DdsFindingKind } from '@/shared/api';
import clsx from '@/shared/lib/clsx';
import s from './BrigadePanel.module.scss';

interface Props {
  cardId: string;
  /** Своя служба диспетчера. */
  serviceId: string;
  /** Меняется при каждом перечитывании карточки: новый статус меняет разбор. */
  stamp: unknown;
}

const hhmmss = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit', second: '2-digit' });

const KIND: Record<DdsFindingKind, string> = {
  ok: 'по факту',
  late: 'с опозданием',
  notByFact: 'не по факту',
  missed: 'статус не проставлен',
  noBrigade: 'бригада не назначена',
};

const errorText = (e: unknown) =>
  e instanceof ApiError ? (e.detail ?? 'Не удалось') : 'Нет связи с сервером';

/**
 * Связь диспетчера ДДС со своей бригадой.
 *
 * Заказчик описал два варианта, и работают оба (чат задачи, 21.09.2026,
 * сообщение 691): старший сам звонит в ДДС с докладом, или диспетчер
 * сам набирает старшего и уточняет ход работ. Статусы после этого
 * диспетчер ставит сам, в плашке службы, — по факту полученных сведений.
 * Звука здесь нет: доклад приходит текстом, как входящий звонок.
 */
export const BrigadePanel = ({ cardId, serviceId, stamp }: Props) => {
  const [brigade, setBrigade] = useState<Brigade | null>(null);
  const [choice, setChoice] = useState(1);
  const [answer, setAnswer] = useState('');
  const [incoming, setIncoming] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

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
      setIncoming(String(e.text ?? ''));
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

  const call = () => run(async () => {
    const r = await cards.callBrigade(cardId, serviceId);
    setAnswer(r.text);
    setIncoming('');
    setBrigade(r.brigade);
  });

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
          <button className={s.btn} data-testid="brigade-call" disabled={busy}
                  onClick={() => void call()}>
            Позвонить старшему
          </button>
          {answer && <span className={s.answer} data-testid="brigade-answer">{answer}</span>}
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
            {f.lagSec != null && f.kind !== 'notByFact' && `, через ${f.lagSec} с после доклада`}
            {f.consequence && <div className={s.consequence}>{f.consequence}</div>}
          </div>
        ))}
      </div>
    </section>
  );
};
