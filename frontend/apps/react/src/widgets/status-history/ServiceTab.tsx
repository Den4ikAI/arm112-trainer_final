import { useState } from 'react';
import { ApiError, type ServiceResponses } from '@/shared/api';
import clsx from '@/shared/lib/clsx';
import s from './ServiceTab.module.scss';

interface Props {
  service: ServiceResponses;
  /** Своя служба — только у неё доступен карандаш. */
  own?: boolean;
  onAdd: (body: { status: string; orderNo?: string; comment?: string }) => Promise<void>;
}

const hhmm = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit' });
const hhmmss = (iso: string) => new Date(iso).toLocaleTimeString('ru-RU',
  { hour: '2-digit', minute: '2-digit', second: '2-digit' });
const dmy = (iso: string) => new Date(iso).toLocaleDateString('ru-RU');

/** Текст ошибки по коду сервера — коды перечислены в BACKEND.md. */
const errorText = (e: unknown): string => {
  if (!(e instanceof ApiError)) return 'Нет связи с сервером';
  if (e.code === 'comment_required') return 'Для этого статуса комментарий обязателен';
  if (e.code === 'status_not_allowed') {
    return e.allowed?.length
      ? `Сейчас доступно: ${e.allowed.join(', ')}`
      : 'Карточка закрыта: работы уже завершены';
  }
  if (e.code === 'forbidden') return 'Статус ставит только своя служба';
  return e.detail ?? 'Не удалось проставить статус';
};

/**
 * Плашка службы в нижней полосе карточки ДДС.
 * Стрелка раскрывает историю статусов, карандаш открывает строку ввода нового.
 *
 * Список доступных статусов приходит с сервера: там же он и проверяется.
 * Интерфейс его не вычисляет — иначе две реализации одного правила
 * рано или поздно разойдутся.
 */
export const ServiceTab = ({ service, own, onAdd }: Props) => {
  const [showHistory, setShowHistory] = useState(false);
  const [editing, setEditing] = useState(false);
  const [status, setStatus] = useState('');
  const [orderNo, setOrderNo] = useState('');
  const [comment, setComment] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const { serviceId, title, entries, allowedNext } = service;
  const last = entries[entries.length - 1];

  const submit = async () => {
    if (!status) { setError('Выберите статус'); return; }
    setBusy(true);
    try {
      await onAdd({ status, orderNo: orderNo || undefined, comment: comment || undefined });
      setStatus(''); setOrderNo(''); setComment(''); setError(''); setEditing(false);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.wrap}>
      {showHistory && entries.length > 0 && (
        <div className={s.history} data-testid={`history-${serviceId}`}>
          <div className={s.historyHead}>
            {title}
            <button className={s.histClose} onClick={() => setShowHistory(false)}
                    aria-label="Закрыть историю">×</button>
          </div>
          {entries.map((e) => (
            <div key={e.id} className={s.histRow}>
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
                  onChange={(e) => setStatus(e.target.value)}>
            <option value="">Статус</option>
            {allowedNext.map((o) => <option key={o} value={o}>{o}</option>)}
          </select>
          <input className={s.input} placeholder="Номер наряда" value={orderNo}
                 aria-label="Номер наряда" onChange={(e) => setOrderNo(e.target.value)} />
          <input className={s.input} placeholder="Комментарий" value={comment}
                 aria-label="Комментарий" onChange={(e) => setComment(e.target.value)} />
          <button className={s.ok} onClick={() => void submit()} disabled={busy}
                  aria-label="Сохранить статус">✓</button>
          <button className={s.cancel} onClick={() => setEditing(false)}
                  aria-label="Отменить">×</button>
          {error && <div className={s.error} role="alert">{error}</div>}
        </div>
      )}

      <div className={clsx(s.tab, own && s.own)} data-testid={`service-tab-${serviceId}`}>
        <div className={s.tabTools}>
          <button className={s.tool} onClick={() => setShowHistory((v) => !v)}
                  aria-label="История статусов" aria-expanded={showHistory}>˅</button>
          {own && allowedNext.length > 0 && (
            // Пока статус уходит на сервер, карандаш не трогаем: клик
            // по нему закрыл бы редактор прямо во время отправки
            <button className={s.tool} onClick={() => setEditing((v) => !v)} disabled={busy}
                    aria-label="Проставить статус" data-testid={`pencil-${serviceId}`}>✎</button>
          )}
        </div>
        <div className={s.tabTitle}>{title}</div>
        <div className={s.tabStatus}>
          {last ? `${hhmm(last.at)} ${last.status}` : '—'}
        </div>
      </div>
    </div>
  );
};
