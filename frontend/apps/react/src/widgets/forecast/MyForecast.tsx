import { useEffect, useState } from 'react';
import { forecast, type MyForecast as Запись } from '@/shared/api';
import s from './MyForecast.module.scss';

/**
 * «Предсказали столько — получил столько» для курсанта.
 *
 * Прогноз был записан до того, как курсант взял вызов, и показывается
 * только теперь. Рядом — из чего он сложился: без объяснения это
 * гадание, а с объяснением курсант видит, что именно тянет его вниз.
 */
export const MyForecast = ({ sessionId, actual }: { sessionId: string; actual: number }) => {
  const [запись, setЗапись] = useState<Запись | null>(null);

  useEffect(() => {
    let alive = true;
    void forecast.mine()
      .then((все) => {
        if (alive) setЗапись(все.find((п) => п.sessionId === sessionId) ?? null);
      })
      .catch(() => {});
    return () => { alive = false; };
  }, [sessionId]);

  // Прогноза может не быть — на первых занятиях истории мало.
  // Молчим, а не рисуем прочерк
  if (!запись) return null;

  const разница = actual - запись.predicted;
  const вклады = (запись.features.contributions ?? []).filter((в) => в.points !== 0);

  return (
    <section className={s.wrap} aria-label="Прогноз" data-testid="my-forecast">
      <h2 className={s.title}>Прогноз перед вызовом</h2>

      <div className={s.row}>
        <div className={s.box}>
          <span className={s.value} data-testid="predicted">{запись.predicted}</span>
          <span className={s.label}>предсказали</span>
        </div>
        <div className={s.box}>
          <span className={s.value}>{actual}</span>
          <span className={s.label}>получилось</span>
        </div>
        <div className={s.box}>
          <span className={clsxЗнак(разница)} data-testid="forecast-diff">
            {разница > 0 ? `+${разница}` : разница}
          </span>
          <span className={s.label}>разница</span>
        </div>
      </div>

      <p className={s.when}>
        Прогноз записан {new Date(запись.madeAt).toLocaleString('ru-RU')}, до начала
        вызова, по {запись.basis} предыдущим разборам
        {запись.trained ? '' : '. Модель ещё не обучена — взят средний балл'}.
      </p>

      {вклады.length > 0 && (
        <ul className={s.parts}>
          {вклады.map((в) => (
            <li key={в.label}>
              <span>{в.label}</span>
              <span className={в.points > 0 ? s.plus : s.minus}>
                {в.points > 0 ? `+${в.points}` : в.points}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );

  function clsxЗнак(d: number) {
    return d === 0 ? s.value : d > 0 ? `${s.value} ${s.plus}` : `${s.value} ${s.minus}`;
  }
};
