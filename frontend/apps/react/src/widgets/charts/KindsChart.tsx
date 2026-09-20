import type { KindCount } from './types';
import { KIND_COLOR, KIND_LABEL } from './types';
import s from './Charts.module.scss';

/**
 * Распределение замечаний по видам.
 *
 * Столбцы, а не круговая диаграмма: доли тут сравнивают между собой,
 * а на круге это делается хуже — глаз плохо сопоставляет углы.
 */
export const KindsChart = ({ kinds }: { kinds: KindCount[] }) => {
  if (kinds.length === 0) return <p className={s.empty}>Замечаний нет.</p>;

  const sorted = [...kinds].sort((a, b) => b.count - a.count);
  const total = sorted.reduce((sum, k) => sum + k.count, 0);
  const max = Math.max(...sorted.map((k) => k.count));

  return (
    <div className={s.bars}>
      {sorted.map((k) => (
        <div key={k.kind} className={s.barRow}>
          <div className={s.barLabel}>{KIND_LABEL[k.kind] ?? k.kind}</div>
          <div className={s.barTrack}>
            <div className={s.barFill}
                 style={{ width: `${(k.count / max) * 100}%`,
                          background: KIND_COLOR[k.kind] ?? '#888' }} />
          </div>
          <div className={s.barValue}>
            {k.count}
            <span className={s.barShare}>{Math.round((k.count / total) * 100)}%</span>
          </div>
        </div>
      ))}
    </div>
  );
};
