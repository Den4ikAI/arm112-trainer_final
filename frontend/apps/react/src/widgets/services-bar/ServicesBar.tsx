import { useState } from 'react';
import { useCatalog } from '@/entities/catalog/model';
import type { Service } from '@/shared/api';
import { Modal } from '@/shared/ui/Modal';
import clsx from '@/shared/lib/clsx';
import s from './ServicesBar.module.scss';

interface Props {
  serviceIds: string[];
  /** Службы, подобранные классификатором автоматически. */
  autoIds: string[];
  onChange: (ids: string[]) => void;
  onSave?: () => void;
  mode: '112' | 'dds';
  children?: React.ReactNode;
}

/**
 * Поиск в окне «Добавьте службы»: по названию с кадра, по заголовку
 * столбца классификатора и по дополнительным словам. Курсант, привыкший
 * к «МЧС» или «скорой», должен найти «Службу 101» и «Службу 103».
 */
const matches = (sv: Service, query: string) => {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return [sv.title, sv.sourceTitle ?? '', ...sv.aliases]
    .some((t) => t.toLowerCase().includes(q));
};

/**
 * Нижняя оранжевая полоса со списком оповещения.
 * Инструкция, раздел 6: в штатной ситуации список формируется автоматически
 * и править его не следует — ручное добавление оставлено для нештатных случаев.
 */
export const ServicesBar = ({ serviceIds, autoIds, onChange, onSave, mode, children }: Props) => {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const services = useCatalog((st) => st.services);
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const serviceShort = useCatalog((st) => st.serviceShort);

  // Автоподобранную службу снять нельзя (инструкция, раздел 6),
  // поэтому наружу отдаём только добавленные вручную
  const manual = serviceIds.filter((id) => !autoIds.includes(id));

  const toggle = (id: string) => {
    if (autoIds.includes(id)) return;
    onChange(manual.includes(id)
      ? manual.filter((x) => x !== id)
      : [...manual, id]);
  };

  const shown = services.filter((sv) => matches(sv, query));

  return (
    <>
      <footer className={clsx(s.bar, mode === 'dds' && s.dds)} aria-label="Список оповещения">
        <span className={s.caption}>Службы:</span>

        <div className={s.list}>
          {serviceIds.map((id) => (
            <span key={id} className={clsx(s.tag, autoIds.includes(id) && s.auto)}
                  data-testid={`service-${id}`}>
              {/* На плашке боевая система пишет короткое имя — «Служба 101»,
                  «Деп. ЖКХ»; полное видно по наведению и в окне добавления */}
              <span title={serviceTitle(id)}>{serviceShort(id)}</span>
              {mode === '112' && !autoIds.includes(id) && (
                <button className={s.tagX} aria-label={`Убрать ${serviceTitle(id)}`}
                        onClick={() => toggle(id)}>×</button>
              )}
            </span>
          ))}
          {children}
        </div>

        {mode === '112' && (
          <>
            <button className={s.add} onClick={() => setOpen(true)} aria-label="Добавить службу">+</button>
            <button className={s.save} onClick={onSave} data-testid="save-card">сохранить</button>
          </>
        )}
      </footer>

      {open && (
        <Modal
          title="Добавьте службы"
          onClose={() => setOpen(false)}
          footer={<button className={s.modalSave} onClick={() => setOpen(false)}>Сохранить и закрыть</button>}
        >
          <input className={s.search} placeholder="Поиск ..." value={query}
                 aria-label="Поиск службы" onChange={(e) => setQuery(e.target.value)} />
          <ul className={s.options}>
            {shown.map((sv) => (
              <li key={sv.id}>
                <button
                  className={clsx(s.option, serviceIds.includes(sv.id) && s.optionOn)}
                  disabled={autoIds.includes(sv.id)}
                  title={autoIds.includes(sv.id) ? 'Подобрана классификатором, снять нельзя' : undefined}
                  onClick={() => toggle(sv.id)}
                >
                  {sv.title}
                </button>
              </li>
            ))}
          </ul>
        </Modal>
      )}
    </>
  );
};
