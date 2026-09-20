import { useMemo, useState } from 'react';
import { useCatalog } from '@/entities/catalog/model';
import { Chip } from '@/shared/ui/Chip';
import s from './IncidentPanel.module.scss';

interface Props {
  selected: string[];
  onToggle: (typeId: string) => void;
}

/**
 * Блок «Что случилось?» (Alt+T).
 * Два способа выбора из инструкции 4.2: плашки часто используемых типов
 * и поиск по строке. Поиск нечувствителен к порядку слов и знакам препинания.
 */
export const IncidentPanel = ({ selected, onToggle }: Props) => {
  const [query, setQuery] = useState('');
  const types = useCatalog((st) => st.types);
  const typeTitle = useCatalog((st) => st.typeTitle);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase().replace(/[-,]/g, ' ');
    if (!q) return [];
    const words = q.split(/\s+/).filter(Boolean);
    return types.filter((t) => {
      const title = t.title.toLowerCase();
      return words.every((w) => title.includes(w));
    }).slice(0, 8);
  }, [query, types]);

  const hasSelection = selected.length > 0;

  return (
    <section className={s.wrap} aria-label="Что случилось">
      <label className={s.searchLabel} htmlFor="type-search">
        {hasSelection ? 'добавить тип происшествия' : 'Введите тип происшествия'}
      </label>

      {!hasSelection && <div className={s.bigTitle}>что случилось?</div>}

      <input
        id="type-search"
        className={s.search}
        value={query}
        placeholder={hasSelection ? 'добавить тип происшествия' : ''}
        onChange={(e) => setQuery(e.target.value)}
        autoComplete="off"
      />

      {matches.length > 0 && (
        <ul className={s.dropdown} role="listbox">
          {matches.map((t) => (
            <li key={t.id}>
              <button
                className={s.option}
                role="option"
                aria-selected={selected.includes(t.id)}
                onClick={() => { onToggle(t.id); setQuery(''); }}
              >
                {t.title}
              </button>
            </li>
          ))}
        </ul>
      )}

      {hasSelection && (
        <div className={s.chosen}>
          {selected.map((id) => (
            <Chip key={id} onClick={() => onToggle(id)} title="Убрать тип">
              {typeTitle(id)}
            </Chip>
          ))}
        </div>
      )}

      {!hasSelection && (
        <>
          <div className={s.frequent}>
            {types.filter((t) => t.frequent).map((t) => (
              <Chip key={t.id} onClick={() => onToggle(t.id)}>{t.title}</Chip>
            ))}
          </div>
          <div className={s.significant}>Значимые типы происшествий:</div>
        </>
      )}
    </section>
  );
};
