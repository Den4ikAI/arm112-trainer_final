import { useEffect, useState } from 'react';
import { ApiError, advice, ddsAdvice, type AdviceEfficacy, type CheckedAdvice } from '@/shared/api';
import s from './Advice.module.scss';

const процент = (v: number | null) => (v === null ? '—' : `${Math.round(v * 100)} %`);

/**
 * Сработали ли рекомендации в группе.
 *
 * Критерий финальной экспертизы — «проверка рекомендаций сервиса
 * и реальных рекомендаций». Поэтому здесь не список советов, а то,
 * насколько они сбылись, и рядом базовая линия: как часто те же
 * повторяющиеся ошибки уходят без рекомендации. Без неё «сработало
 * 60 %» ничего не значит — может, ошибки уходят и сами.
 *
 * `source` — чьи рекомендации: операторов 112 или диспетчеров ДДС.
 * Устройство проверки у них одно, и блок один.
 */
export const AdviceBlock = ({ groupId, source = 'op112' }: { groupId: string; source?: 'op112' | 'dds' }) => {
  const [э, setЭ] = useState<AdviceEfficacy | null>(null);
  const [лента, setЛента] = useState<CheckedAdvice[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!groupId) return;
    let alive = true;
    void (source === 'dds' ? ddsAdvice.group(groupId) : advice.group(groupId))
      .then((r) => {
        if (!alive) return;
        setЭ(r.efficacy);
        setЛента(r.recent);
        setError(null);
      })
      .catch((e) => {
        if (alive) {
          setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Рекомендации недоступны');
        }
      });
    return () => { alive = false; };
  }, [groupId, source]);

  if (error) return <p className={s.empty}>{error}</p>;
  if (!э) return null;

  const разница = э.rate !== null && э.baselineRate !== null ? э.rate - э.baselineRate : null;

  return (
    <section className={s.wrap} aria-label="Рекомендации"
             data-testid={source === 'dds' ? 'advice-block-dds' : 'advice-block'}>
      <h2 className={s.title}>
        {source === 'dds' ? 'Рекомендации диспетчерам ДДС и их исполнение' : 'Рекомендации и их исполнение'}
      </h2>

      {э.checked === 0 && (
        <p className={s.empty} data-testid="advice-empty">
          {source === 'dds'
            ? 'Проверенных рекомендаций пока нет. Рекомендация выдаётся по замечанию разбора закрытой карточки и проверяется следующей карточкой, где ошибку можно повторить.'
            : 'Проверенных рекомендаций пока нет. Рекомендация выдаётся, когда ошибка повторилась, и проверяется следующим вызовом, где её можно повторить.'}
        </p>
      )}

      {э.checked > 0 && (
        <>
          <div className={s.metrics}>
            <div className={s.metric}>
              <span className={s.value} data-testid="advice-checked">
                {э.worked} из {э.checked}
              </span>
              <span className={s.label}>рекомендаций сработали</span>
            </div>
            <div className={s.metric}>
              <span className={s.value}>{процент(э.rate)}</span>
              <span className={s.label}>доля сработавших</span>
            </div>
            <div className={s.metric}>
              <span className={s.value}>{процент(э.baselineRate)}</span>
              <span className={s.label}>
                без рекомендации ошибка уходит сама
                {э.baselineChecked > 0 ? ` (по ${э.baselineChecked})` : ''}
              </span>
            </div>
          </div>

          {разница !== null && (
            <p className={s.verdict} data-testid="advice-verdict-group">
              {разница > 0.05
                ? `С рекомендацией ошибка уходит чаще, чем без неё, на ${Math.round(разница * 100)} п.п.`
                : 'Пока не видно, что рекомендации помогают сильнее, чем практика сама по себе.'}
            </p>
          )}

          {э.tooFew && (
            <p className={s.note}>
              Проверок мало: по {э.checked} случаям о доле судить рано.
            </p>
          )}

          {/* Смещение базовой линии называем вслух: в неё попадают
              более редкие ошибки, их и исправить проще */}
          <p className={s.note}>
            Базовая линия — повторяющиеся ошибки тех же курсантов, на которые
            рекомендации не было. Рекомендации получают самые частые ошибки,
            поэтому сравнение скорее в пользу базовой линии.
          </p>

          <table className={s.table}>
            <thead>
              <tr><th>Курсант</th><th>Рекомендация</th><th>Итог</th><th>Проверена</th></tr>
            </thead>
            <tbody>
              {лента.slice(0, 12).map((a, i) => (
                <tr key={`${a.checkedAt}-${i}`}>
                  <td>{a.fio}</td>
                  <td>{a.text}</td>
                  <td className={a.worked ? s.ok : s.bad}>
                    {a.worked ? 'сработала' : 'не сработала'}
                  </td>
                  <td className={s.muted}>{new Date(a.checkedAt).toLocaleString('ru-RU')}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
};
