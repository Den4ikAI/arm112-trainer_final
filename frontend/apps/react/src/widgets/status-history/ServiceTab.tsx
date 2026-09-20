import { useState } from 'react';
import {
  nextStatuses, requiresComment, getService,
  type ResponseEntry, type ResponseStatus, type ServiceResponse,
} from '@arm112/domain';
import clsx from '@/shared/lib/clsx';
import s from './ServiceTab.module.scss';

interface Props {
  serviceId: string;
  response?: ServiceResponse;
  /** Своя служба — только у неё доступен карандаш. */
  own?: boolean;
  onAdd: (entry: ResponseEntry) => void;
}

const hhmm = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit' });
const hhmmss = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit', second: '2-digit' });
const dmy = (iso: string) => new Date(iso).toLocaleDateString('ru-RU');

/**
 * Плашка службы в нижней полосе карточки ДДС.
 * Стрелка раскрывает историю статусов, карандаш открывает строку ввода нового.
 * Список статусов последователен: сначала только «Принята» / «Не принята».
 */
export const ServiceTab = ({ serviceId, response, own, onAdd }: Props) => {
  const [showHistory, setShowHistory] = useState(false);
  const [editing, setEditing] = useState(false);
  const [status, setStatus] = useState<ResponseStatus | ''>('');
  const [orderNo, setOrderNo] = useState('');
  const [comment, setComment] = useState('');
  const [error, setError] = useState('');

  const entries = response?.entries ?? [];
  const last = entries[entries.length - 1];
  const options = nextStatuses(entries);

  const submit = () => {
    if (!status) { setError('Выберите статус'); return; }
    if (requiresComment(status) && !comment.trim()) {
      // Памятка для ДДС: для «Не принята» и «Отказа» комментарий обязателен.
      setError('Для этого статуса комментарий обязателен');
      return;
    }
    onAdd({ status, at: new Date().toISOString(), operator: 'оп. 0', orderNo, comment });
    setStatus(''); setOrderNo(''); setComment(''); setError(''); setEditing(false);
  };

  return (
    <div className={s.wrap}>
      {showHistory && entries.length > 0 && (
        <div className={s.history} data-testid={`history-${serviceId}`}>
          <div className={s.historyHead}>
            {getService(serviceId)?.title}
            <button className={s.histClose} onClick={() => setShowHistory(false)}
                    aria-label="Закрыть историю">×</button>
          </div>
          {entries.map((e, i) => (
            <div key={i} className={s.histRow}>
              <span className={s.op}>{e.operator}</span>
              <span className={s.arrow}>›</span>
              <span className={s.when}>{dmy(e.at)} {hhmmss(e.at)}</span>
              <span className={s.status}>{e.status}</span>
              {e.comment && <><span className={s.arrow}>›</span>
                <span className={s.comment}>{e.comment}</span></>}
            </div>
          ))}
        </div>
      )}

      {editing && (
        <div className={s.editor} data-testid={`status-editor-${serviceId}`}>
          <select className={s.select} value={status} aria-label="Статус"
                  onChange={(e) => setStatus(e.target.value as ResponseStatus)}>
            <option value="">Статус</option>
            {options.map((o) => <option key={o} value={o}>{o}</option>)}
          </select>
          <input className={s.input} placeholder="Номер наряда" value={orderNo}
                 aria-label="Номер наряда" onChange={(e) => setOrderNo(e.target.value)} />
          <input className={s.input} placeholder="Комментарий" value={comment}
                 aria-label="Комментарий" onChange={(e) => setComment(e.target.value)} />
          <button className={s.ok} onClick={submit} aria-label="Сохранить статус">✓</button>
          <button className={s.cancel} onClick={() => setEditing(false)}
                  aria-label="Отменить">×</button>
          {error && <div className={s.error} role="alert">{error}</div>}
        </div>
      )}

      <div className={clsx(s.tab, own && s.own)} data-testid={`service-tab-${serviceId}`}>
        <div className={s.tabTools}>
          <button className={s.tool} onClick={() => setShowHistory((v) => !v)}
                  aria-label="История статусов" aria-expanded={showHistory}>˅</button>
          {own && options.length > 0 && (
            <button className={s.tool} onClick={() => setEditing((v) => !v)}
                    aria-label="Проставить статус" data-testid={`pencil-${serviceId}`}>✎</button>
          )}
        </div>
        <div className={s.tabTitle}>{getService(serviceId)?.title ?? serviceId}</div>
        <div className={s.tabStatus}>
          {last ? `${hhmm(last.at)} ${last.status}` : '—'}
        </div>
      </div>
    </div>
  );
};
