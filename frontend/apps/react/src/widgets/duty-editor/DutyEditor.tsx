import { useState } from 'react';
import { admin, ApiError, type Duty } from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import s from './DutyEditor.module.scss';

/**
 * Рабочее место обучающегося: оператор 112 или диспетчер ДДС своей службы.
 *
 * Организатор, чат 18.09: обучающегося «могут направить на практику в
 * одно или другое место в зависимости от результатов». Направляет
 * преподаватель или администратор; действует сразу, без перевхода.
 */
export const DutyEditor = ({ userId, login, duty, serviceId, onChanged }: {
  userId: string;
  login: string;
  duty: Duty | null;
  serviceId: string | null;
  onChanged: () => void;
}) => {
  const services = useCatalog((st) => st.services);
  const [next, setNext] = useState<Duty>(duty ?? 'op112');
  const [service, setService] = useState(serviceId ?? '');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const changed = next !== duty || (next === 'dds' && service !== (serviceId ?? ''));

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      await admin.setDuty(userId, next, next === 'dds' ? service || null : null);
      onChanged();
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не удалось изменить');
    } finally {
      setBusy(false);
    }
  };

  return (
    <span className={s.wrap} data-testid={`duty-${login}`}>
      <select value={next} aria-label={`Рабочее место ${login}`}
              onChange={(e) => setNext(e.target.value as Duty)}>
        <option value="op112">Оператор 112</option>
        <option value="dds">Диспетчер ДДС</option>
      </select>
      {next === 'dds' && (
        <select value={service} aria-label={`Служба ${login}`}
                onChange={(e) => setService(e.target.value)}>
          <option value="">выберите службу</option>
          {/* Службы окна «Добавьте службы» боевой системы; уже назначенная
              остаётся в списке, даже если скрыта */}
          {services.filter((sv) => sv.listed || sv.id === service).map((sv) => (
            <option key={sv.id} value={sv.id}>{sv.shortTitle || sv.title}</option>
          ))}
        </select>
      )}
      {changed && (
        <button className={s.apply} disabled={busy || (next === 'dds' && !service)}
                data-testid={`duty-apply-${login}`} onClick={() => void apply()}>
          {busy ? '…' : 'применить'}
        </button>
      )}
      {error && <span className={s.error} role="alert">{error}</span>}
    </span>
  );
};
