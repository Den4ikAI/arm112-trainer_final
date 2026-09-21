import { useEffect, useState } from 'react';
import { ApiError, forecast, type CheckedForecast, type ForecastAccuracy } from '@/shared/api';
import s from './ForecastBlock.module.scss';

/**
 * Достоверность прогнозов по группе.
 *
 * Критерий финальной экспертизы просит «оценку достоверности путем
 * сравнения реальных данных и прогнозных значений». Поэтому здесь
 * не прогнозы, а то, насколько они сбылись, — и рядом базовая линия,
 * без которой величина ошибки ни о чём не говорит.
 *
 * Пока прогнозов мало, честно пишем об этом вместо красивого числа.
 */
export const ForecastBlock = ({ groupId }: { groupId: string }) => {
  const [точность, setТочность] = useState<ForecastAccuracy | null>(null);
  const [лента, setЛента] = useState<CheckedForecast[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!groupId) return;
    let alive = true;
    void forecast.group(groupId)
      .then((r) => {
        if (!alive) return;
        setТочность(r.accuracy);
        setЛента(r.checked);
        setError(null);
      })
      .catch((e) => {
        if (alive) {
          setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Прогнозы недоступны');
        }
      });
    return () => { alive = false; };
  }, [groupId]);

  if (error) return <p className={s.empty}>{error}</p>;
  if (!точность) return null;

  // Меньше пяти проверок — это не оценка достоверности, а совпадение
  const мало = точность.checked < 5;
  const лучшеБазовой =
    точность.meanError !== null && точность.baselineError !== null
      ? точность.baselineError - точность.meanError
      : null;

  return (
    <section className={s.wrap} aria-label="Достоверность прогноза"
             data-testid="forecast-block">
      <h2 className={s.title}>Прогноз и факт</h2>

      {точность.checked === 0 && (
        <p className={s.empty} data-testid="forecast-empty">
          Проверенных прогнозов пока нет. Прогноз записывается, когда курсант
          берёт вызов, и сверяется с фактом после разбора.
        </p>
      )}

      {точность.checked > 0 && (
        <>
          <div className={s.metrics}>
            <div className={s.metric}>
              <span className={s.value} data-testid="forecast-checked">
                {точность.checked}
              </span>
              <span className={s.label}>прогнозов проверено</span>
            </div>
            <div className={s.metric}>
              <span className={s.value} data-testid="forecast-error">
                {точность.meanError?.toFixed(1)}
              </span>
              <span className={s.label}>средняя ошибка, баллов</span>
            </div>
            <div className={s.metric}>
              <span className={s.value}>
                {точность.hitRate === null ? '—' : `${Math.round(точность.hitRate * 100)} %`}
              </span>
              <span className={s.label}>попаданий в ±{точность.tolerance}</span>
            </div>
            <div className={s.metric}>
              <span className={s.value}>{точность.baselineError?.toFixed(1) ?? '—'}</span>
              <span className={s.label}>ошибка без модели</span>
            </div>
          </div>

          {/* Само по себе «ошибка 8 баллов» ничего не значит: видно
              только рядом с тем, что даёт средний балл курсанта */}
          {лучшеБазовой !== null && (
            <p className={s.verdict} data-testid="forecast-verdict">
              {лучшеБазовой > 0.5
                ? `Модель точнее простого среднего на ${лучшеБазовой.toFixed(1)} балла.`
                : 'Модель пока не точнее простого среднего — данных мало.'}
            </p>
          )}

          {мало && (
            <p className={s.warn}>
              Проверок мало: по {точность.checked} случаям о достоверности
              судить рано.
            </p>
          )}

          <table className={s.table} data-testid="forecast-table">
            <thead>
              <tr>
                <th>Курсант</th><th>Предсказали</th><th>Получил</th>
                <th>Разница</th><th>Прогноз сделан</th>
              </tr>
            </thead>
            <tbody>
              {лента.slice(0, 12).map((c) => {
                const разница = c.actual - c.predicted;
                return (
                  <tr key={c.sessionId}>
                    <td>{c.fio}</td>
                    <td>{c.predicted}</td>
                    <td>{c.actual}</td>
                    <td className={Math.abs(разница) <= точность.tolerance ? s.hit : s.miss}>
                      {разница > 0 ? `+${разница}` : разница}
                    </td>
                    {/* Время фиксации — часть доказательства: прогноз
                        сделан до вызова, а не подогнан после */}
                    <td className={s.when}>
                      {new Date(c.madeAt).toLocaleString('ru-RU')}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
};
