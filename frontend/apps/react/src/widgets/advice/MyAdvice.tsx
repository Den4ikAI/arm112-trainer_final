import { useEffect, useState } from 'react';
import { advice, type Advice } from '@/shared/api';
import s from './Advice.module.scss';

/**
 * Рекомендации курсанту в разборе.
 *
 * Две части. Сверху — вердикт по прошлым рекомендациям, которые
 * проверил именно этот разбор: была ли у ошибки возможность
 * повториться, и повторилась ли. Снизу — рекомендации на следующий
 * вызов: ошибки, которые повторяются, и что с ними делать.
 *
 * На первых занятиях рекомендаций нет — ошибка ещё не успела стать
 * привычкой. Тогда блок не рисуется вовсе.
 */
export const MyAdvice = ({ sessionId }: { sessionId: string }) => {
  const [все, setВсе] = useState<Advice[] | null>(null);

  useEffect(() => {
    let alive = true;
    void advice.mine()
      .then((r) => { if (alive) setВсе(r); })
      .catch(() => { if (alive) setВсе([]); });
    return () => { alive = false; };
  }, [sessionId]);

  if (!все) return null;

  const проверены = все.filter((a) => a.checkSessionId === sessionId);
  const открыты = все.filter((a) => a.checkedAt === null);
  if (проверены.length === 0 && открыты.length === 0) return null;

  return (
    <section className={s.wrap} aria-label="Рекомендации" data-testid="my-advice">
      {проверены.length > 0 && (
        <>
          <h2 className={s.title}>Прошлые рекомендации</h2>
          <ul className={s.list}>
            {проверены.map((a) => (
              <li key={a.id} className={s.item} data-testid="advice-verdict">
                <span className={a.worked ? s.ok : s.bad}>
                  {a.worked ? 'выполнена' : 'не выполнена'}
                </span>
                <span>{a.text}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      {открыты.length > 0 && (
        <>
          <h2 className={s.title}>На следующий вызов</h2>
          <ul className={s.list}>
            {открыты.map((a) => (
              <li key={a.id} className={s.item} data-testid="advice-open">
                <span className={s.muted}>
                  {a.occurrences} из {a.windowSize}
                </span>
                <span>{a.text}</span>
              </li>
            ))}
          </ul>
          {/* Откуда рекомендация и когда её проверят — чтобы курсант
              понимал, что это не общий совет, а его собственная ошибка */}
          <p className={s.note}>
            Рекомендация выдаётся, когда ошибка повторилась за последние разборы,
            и проверяется в первом следующем вызове, где её можно повторить.
          </p>
        </>
      )}
    </section>
  );
};
