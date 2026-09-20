import type { TimelinePoint } from './types';
import s from './Charts.module.scss';

/**
 * Время заполнения карточки против норматива.
 *
 * Норматив — горизонтальная черта, занятия — точки. Точка выше черты
 * означает превышение; её и красят. Никакого сглаживания: занятия
 * идут разной сложности, и линия между ними означала бы связь,
 * которой нет.
 */
export const TimeChart = ({ points }: { points: TimelinePoint[] }) => {
  if (points.length === 0) {
    return <p className={s.empty}>Занятий за период не было.</p>;
  }

  const W = 720;
  const H = 240;
  const PAD = { top: 16, right: 16, bottom: 28, left: 44 };
  const plotW = W - PAD.left - PAD.right;
  const plotH = H - PAD.top - PAD.bottom;

  const norm = points[0].normSec;
  // Шкала до максимума, но не ниже полутора нормативов: иначе при
  // хороших результатах черта норматива прижимается к потолку
  // и по графику не видно, что запас большой
  const maxSec = Math.max(norm * 1.5, ...points.map((p) => p.elapsedSec));

  const x = (i: number) =>
    PAD.left + (points.length === 1 ? plotW / 2 : (i / (points.length - 1)) * plotW);
  const y = (sec: number) => PAD.top + plotH - (sec / maxSec) * plotH;

  const ticks = [0, norm, Math.round(maxSec)];

  return (
    <div className={s.scroll}>
      <svg
        className={s.svg}
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={`Время заполнения карточки по ${points.length} занятиям против норматива ${norm} секунд`}
      >
        {ticks.map((t) => (
          <g key={t}>
            <line x1={PAD.left} x2={W - PAD.right} y1={y(t)} y2={y(t)}
                  stroke="#dcdcdc" strokeWidth="1" />
            <text x={PAD.left - 6} y={y(t) + 4} textAnchor="end" className={s.axis}>
              {t}
            </text>
          </g>
        ))}

        {/* Норматив — единственная линия, которая что-то значит */}
        <line x1={PAD.left} x2={W - PAD.right} y1={y(norm)} y2={y(norm)}
              stroke="#d32f2f" strokeWidth="1.5" strokeDasharray="6 4" />
        <text x={W - PAD.right} y={y(norm) - 6} textAnchor="end" className={s.normLabel}>
          норматив {norm} с
        </text>

        {points.map((p, i) => (
          <circle
            key={`${p.at}-${i}`}
            cx={x(i)}
            cy={y(Math.min(p.elapsedSec, maxSec))}
            r={3}
            fill={p.overNorm ? '#d32f2f' : '#1565c0'}
            opacity={0.75}
          >
            <title>
              {`${p.fio}: ${p.elapsedSec} с при нормативе ${p.normSec} с, балл ${p.score}`}
            </title>
          </circle>
        ))}

        <text x={PAD.left} y={H - 8} className={s.axis}>раньше</text>
        <text x={W - PAD.right} y={H - 8} textAnchor="end" className={s.axis}>позже</text>
      </svg>
    </div>
  );
};
