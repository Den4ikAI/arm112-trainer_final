import { useMemo, useState } from 'react';
import type { Incident, LibraryItem } from '@/shared/api';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import s from './LessonMaterialPicker.module.scss';

interface Props {
  mode: 'card112' | 'dds';
  /** Утверждённые вызовы 112 */
  scenarios: LibraryItem[];
  /** Утверждённые происшествия ДДС */
  incidents: Incident[];
  /** Выбранные поимённо, в порядке выбора */
  picked: string[];
  onChange: (ids: string[]) => void;
}

/**
 * Материал занятия: какие вызовы получит оператор 112 или какие
 * происшествия — диспетчер ДДС. Доступно всё утверждённое, в том числе
 * собранное преподавателем вручную и по описанию (п. 6 замечаний).
 *
 * Карточки, заполненные операторами 112, здесь не предлагаются: к
 * диспетчерам попадает только то, что выдал преподаватель (п. 10).
 */
export const LessonMaterialPicker = ({ mode, scenarios, incidents, picked, onChange }: Props) => {
  const [query, setQuery] = useState('');

  const items = useMemo(() => {
    const all = mode === 'card112'
      ? scenarios.map((sc) => ({ id: sc.id, title: scenarioTitle(sc), text: sc.outcome ?? sc.legend }))
      : incidents.map((inc) => ({
          id: inc.id,
          title: inc.title ?? `Происшествие № ${inc.number}`,
          text: inc.outcomeType ?? inc.description,
        }));
    const q = query.trim().toLowerCase();
    return q ? all.filter((i) => `${i.title} ${i.text}`.toLowerCase().includes(q)) : all;
  }, [mode, scenarios, incidents, query]);

  const toggle = (id: string) =>
    onChange(picked.includes(id) ? picked.filter((v) => v !== id) : [...picked, id]);

  const что = mode === 'card112' ? 'вызовов 112' : 'происшествий ДДС';

  return (
    <fieldset className={s.box} data-testid="lesson-material">
      <legend>{mode === 'card112' ? 'Вызовы 112 по порядку' : 'Происшествия ДДС по порядку'}</legend>
      <input className={s.search} value={query} onChange={(e) => setQuery(e.target.value)}
             placeholder="Найти по теме" aria-label="Найти материал" />
      <div className={s.list}>
        {items.length === 0 && (
          <p className={s.empty}>
            Утверждённых {что} нет — соберите их в разделе «Учебный материал».
          </p>
        )}
        {items.map((i) => {
          const номер = picked.indexOf(i.id);
          return (
            <label key={i.id} className={s.item}>
              <input type="checkbox" checked={номер >= 0}
                     data-testid={`lesson-pick-${i.id}`}
                     onChange={() => toggle(i.id)} />
              <span className={s.num}>{номер >= 0 ? `${номер + 1}.` : ''}</span>
              <span>
                <span className={s.title}>{i.title}</span>
                <span className={s.text}>{i.text?.slice(0, 80)}</span>
              </span>
            </label>
          );
        })}
      </div>
      <p className={s.hint}>
        {picked.length === 0
          ? mode === 'card112'
            ? 'Не выбрано — система подберёт по категориям'
            : 'Не выбрано — выберите происшествия, которые получат диспетчеры'
          : `Выбрано: ${picked.length}`}
      </p>
    </fieldset>
  );
};
