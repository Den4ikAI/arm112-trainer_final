import type { Funnel } from './types';
import { KIND_COLOR } from './types';
import s from './Charts.module.scss';

/**
 * Воронка заполнения.
 *
 * Главная диаграмма продукта: она показывает разницу между «выяснил»
 * и «угадал». Обе доли попадают в эталон, но только первая — работа
 * оператора. Если столбец «угадал» сопоставим с «выяснил», группа
 * заполняет карточки по догадке, и балл этого не покажет.
 */
export const FunnelChart = ({ funnel }: { funnel: Funnel }) => {
  const bars = [
    { kind: 'ok', label: 'Выяснил у заявителя', value: funnel.learned },
    { kind: 'guessed', label: 'Угадал', value: funnel.guessed },
    { kind: 'wrong', label: 'Ошибся', value: funnel.wrong },
    { kind: 'missing', label: 'Не заполнил', value: funnel.missing },
  ];

  const total = bars.reduce((sum, b) => sum + b.value, 0);
  if (total === 0) return <p className={s.empty}>Заполненных карточек за период нет.</p>;

  const max = Math.max(...bars.map((b) => b.value));

  return (
    <div className={s.bars}>
      {bars.map((b) => (
        <div key={b.kind} className={s.barRow}>
          <div className={s.barLabel}>{b.label}</div>
          <div className={s.barTrack}>
            <div
              className={s.barFill}
              style={{
                width: `${max ? (b.value / max) * 100 : 0}%`,
                background: KIND_COLOR[b.kind],
              }}
            />
          </div>
          <div className={s.barValue}>
            {b.value}
            <span className={s.barShare}>{Math.round((b.value / total) * 100)}%</span>
          </div>
        </div>
      ))}

      {funnel.learned + funnel.guessed > 0 && (
        <p className={s.note}>
          Из значений, совпавших с эталоном, выяснено у заявителя{' '}
          <strong>
            {Math.round((funnel.learned / (funnel.learned + funnel.guessed)) * 100)}%
          </strong>
          . Остальное курсанты вписали, не спросив.
        </p>
      )}
    </div>
  );
};
