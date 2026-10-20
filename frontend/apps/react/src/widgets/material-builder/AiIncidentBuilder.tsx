import { useState } from 'react';
import { ApiError, ddsIncidents, type Incident } from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import s from './MaterialBuilder.module.scss';

interface Props {
  onDone: (last: Incident | null) => Promise<void> | void;
  onCancel: () => void;
}

/** Службы, с которыми учебный центр работает чаще: наверх списка. */
const ЧАЩЕ = ['mchs', 'smp', 'mosgaz', 'codd', 'moslift', 'dep-zhkh', 'mvd', 'mosvodokanal'];

/**
 * Происшествие от ИИ: преподаватель называет только службу.
 *
 * Обратная связь экспертов 18.10 — «показать формирование карточек для
 * специалиста ДДС искусственным интеллектом». Система берёт позицию
 * классификатора, по которой служба выезжает, восстанавливает ответы
 * опросной карты и проверяет их классификатором; модель пишет заявителя
 * и описание. Получается черновик — проверить и утвердить.
 */
export const AiIncidentBuilder = ({ onDone, onCancel }: Props) => {
  const services = useCatalog((st) => st.services);
  const [service, setService] = useState('mchs');
  const [count, setCount] = useState(1);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<Incident[]>([]);
  const [error, setError] = useState<string | null>(null);

  const ordered = [
    ...ЧАЩЕ.map((id) => services.find((sv) => sv.id === id)).filter((sv) => !!sv),
    ...services.filter((sv) => !ЧАЩЕ.includes(sv.id)),
  ];

  const go = async () => {
    setBusy(true);
    setError(null);
    setDone([]);
    let last: Incident | null = null;
    try {
      // По одному: на процессоре каждое — до двух минут, и прогресс
      // должен быть виден, а не одно долгое «готовлю…»
      for (let i = 0; i < count; i += 1) {
        last = await ddsIncidents.generate(service);
        const got = last;
        setDone((d) => [...d, got]);
      }
      await onDone(last);
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не получилось');
      if (last) await onDone(last);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className={s.form} data-testid="ai-builder"
          onSubmit={(e) => { e.preventDefault(); void go(); }}>
      <h2 className={s.title}>Происшествие от ИИ для диспетчера службы</h2>
      <p className={s.hint}>
        Достаточно выбрать службу. Система возьмёт случайную позицию классификатора,
        по которой служба выезжает, восстановит ответы опросной карты и проверит их
        классификатором — службы и итоговый тип модель не выдумывает. Модель напишет
        заявителя и описание, адрес — из учебного набора. Получится черновик:
        проверьте, при необходимости поправьте и утвердите.
      </p>
      <div className={s.row}>
        <label className={s.field}>
          <span>Служба</span>
          <select value={service} onChange={(e) => setService(e.target.value)}
                  aria-label="Служба" data-testid="ai-service" disabled={busy}>
            {ordered.map((sv) => (
              <option key={sv.id} value={sv.id}>{sv.shortTitle || sv.title}</option>
            ))}
          </select>
        </label>
        <label className={s.field}>
          <span>Сколько происшествий</span>
          <select value={count} onChange={(e) => setCount(Number(e.target.value))}
                  aria-label="Сколько происшествий" data-testid="ai-count" disabled={busy}>
            {[1, 2, 3, 4, 5].map((n) => <option key={n} value={n}>{n}</option>)}
          </select>
        </label>
      </div>
      {error && <p className={s.error} role="alert">{error}</p>}
      {done.length > 0 && (
        <ol className={s.note} data-testid="ai-done">
          {done.map((inc) => (
            <li key={inc.id}>№ {inc.number} · {inc.title ?? inc.outcomeType ?? 'без темы'}</li>
          ))}
        </ol>
      )}
      <div className={s.actions}>
        <button className={s.primary} type="submit" disabled={busy} data-testid="ai-submit">
          {busy ? `модель готовит… ${done.length} из ${count}` : 'Собрать'}
        </button>
        <button className={s.plain} type="button" onClick={onCancel} disabled={busy}>Отмена</button>
        {busy && (
          <span className={s.note}>На видеокарте — секунды, на процессоре — до двух минут на каждое.</span>
        )}
      </div>
    </form>
  );
};
