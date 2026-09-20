import type { HeatCell } from './types';
import { KIND_COLOR, KIND_LABEL } from './types';
import s from './Charts.module.scss';

/**
 * Тепловая карта ошибок: поле карточки × вид замечания.
 *
 * Насыщенность клетки — доля от самой частой ошибки в таблице,
 * а не абсолютное число: иначе при разном размере групп карты
 * нельзя сравнивать между собой. Само число тоже подписано —
 * по одному цвету точное значение не прочитать, а оценку
 * достоверности диаграмм заказчик проверяет отдельно.
 */
export const Heatmap = ({ cells }: { cells: HeatCell[] }) => {
  if (cells.length === 0) {
    return <p className={s.empty}>Замечаний нет — тепловой карте нечего показывать.</p>;
  }

  const labels = [...new Set(cells.map((c) => c.label))];
  const kinds = [...new Set(cells.map((c) => c.kind))].sort();
  const max = Math.max(...cells.map((c) => c.count));
  const at = (label: string, kind: string) =>
    cells.find((c) => c.label === label && c.kind === kind)?.count ?? 0;

  // Поля с самой большой суммой — наверх: с них и начинают разбор
  const ordered = labels
    .map((l) => ({ label: l, total: kinds.reduce((sum, k) => sum + at(l, k), 0) }))
    .sort((a, b) => b.total - a.total);

  return (
    <div className={s.scroll}>
      <table className={s.heat}>
        <thead>
          <tr>
            <th className={s.heatCorner}>Поле карточки</th>
            {kinds.map((k) => <th key={k}>{KIND_LABEL[k] ?? k}</th>)}
            <th>всего</th>
          </tr>
        </thead>
        <tbody>
          {ordered.map(({ label, total }) => (
            <tr key={label}>
              <th className={s.heatLabel} scope="row">{label}</th>
              {kinds.map((k) => {
                const n = at(label, k);
                return (
                  <td
                    key={k}
                    className={s.heatCell}
                    title={`${label}: ${KIND_LABEL[k] ?? k} — ${n}`}
                    style={{
                      // Прозрачность вместо разных цветов: так видно
                      // и вид замечания, и его частоту одновременно
                      background: n
                        ? `color-mix(in srgb, ${KIND_COLOR[k] ?? '#888'} ${Math.round((n / max) * 100)}%, transparent)`
                        : undefined,
                    }}
                  >
                    {n || ''}
                  </td>
                );
              })}
              <td className={s.heatTotal}>{total}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
};
