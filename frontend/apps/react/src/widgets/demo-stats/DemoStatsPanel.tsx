import { useEffect, useState } from 'react';
import { ApiError, demoStats, type DemoStats } from '@/shared/api';
import s from './DemoStatsPanel.module.scss';

/**
 * Демонстрационная статистика: заполнить программу историей обучения
 * или очистить до «голой» программы.
 *
 * Панели преподавателя и администратора (аналитика, ход занятий,
 * рекомендации, журнал звонков) на свежем стенде пустые — показать их
 * нечем. Отсюда одной кнопкой: заполнить историей двух групп за
 * последние недели или убрать всю статистику учеников. Учётные записи
 * и учебный материал при этом остаются.
 */
export const DemoStatsPanel = () => {
  const [state, setState] = useState<DemoStats | null>(null);
  const [busy, setBusy] = useState<'fill' | 'clear' | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    demoStats.view()
      .then((v) => { if (alive) setState(v); })
      .catch(() => { if (alive) setError('Не удалось узнать состояние статистики'); });
    return () => { alive = false; };
  }, []);

  const run = async (what: 'fill' | 'clear') => {
    if (what === 'clear' && !window.confirm(
      'Удалить всю статистику учеников? Учётные записи, группы и учебный материал останутся.')) return;
    setBusy(what);
    setNote(null);
    setError(null);
    try {
      const v = what === 'fill' ? await demoStats.fill() : await demoStats.clear();
      setState(v);
      setNote(what === 'fill'
        ? `Готово: ${v.lessons} занятий, ${v.attempts} попыток, ${v.calls} звонков, ${v.advice} рекомендаций`
        : 'Статистика удалена — программа чистая');
    } catch (e) {
      setError(e instanceof ApiError && e.detail ? e.detail : 'Не получилось, попробуйте ещё раз');
    } finally {
      setBusy(null);
    }
  };

  const filled = !!state && state.attempts > 0;

  return (
    <section className={s.panel} data-testid="demo-stats-panel" aria-label="Демонстрационная статистика">
      <div className={s.head}>
        <h2 className={s.title}>Демонстрационная статистика</h2>
        <span className={filled ? s.on : s.off} data-testid="demo-stats-state">
          {!state ? '…' : filled
            ? `заполнена: ${state.lessons} занятий · ${state.students} курсантов · ${state.attempts} попыток`
            : 'пусто — чистая программа'}
        </span>
      </div>
      <p className={s.text}>
        Заполняет программу историей обучения двух групп диспетчеров ДДС за последние недели:
        разный уровень у групп и у каждого курсанта, занятия в разные дни и часы, звонки,
        рекомендации и экспертные оценки. Баллы и рекомендации считает сам тренажёр.
        Нужна, чтобы показать аналитику, ход занятий и журналы; «Очистить» оставляет
        голую программу с учётными записями и учебным материалом.
      </p>
      <div className={s.actions}>
        <button className={s.fill} disabled={!!busy || state?.sample === false}
                data-testid="demo-stats-fill" onClick={() => void run('fill')}>
          {busy === 'fill' ? 'Заполняю…' : filled ? 'Заполнить заново' : 'Заполнить статистикой'}
        </button>
        <button className={s.clear} disabled={!!busy || !filled}
                data-testid="demo-stats-clear" onClick={() => void run('clear')}>
          {busy === 'clear' ? 'Очищаю…' : 'Очистить статистику'}
        </button>
      </div>
      {state?.sample === false && (
        <p className={s.muted}>Образца статистики на этом стенде нет — заполнять нечем.</p>
      )}
      {note && <p className={s.note} role="status" data-testid="demo-stats-note">{note}</p>}
      {error && <p className={s.error} role="alert">{error}</p>}
    </section>
  );
};
