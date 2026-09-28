import { useState } from 'react';
import { useCatalog } from '@/entities/catalog/model';
import type { Service } from '@/shared/api';
import { Modal } from '@/shared/ui/Modal';
import clsx from '@/shared/lib/clsx';
import { stubProps } from '@/shared/ui/Stub';
import { orderServices } from '@/shared/lib/serviceOrder';
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
 * Кнопки правого края полосы боевой системы. В тренажёре они без
 * действия (см. раздел «Ограничения» документации), но стоят на своих
 * местах, чтобы экран был узнаваем. Контуры нарисованы по кадру.
 */
const ICONS: Array<[string, string]> = [
  ['Связь с другим происшествием', 'M8 12l-2 2a3 3 0 01-4-4l2-2M12 8l2-2a3 3 0 014 4l-2 2M7 7L5 5M13 13l2 2'],
  ['Секундомер', 'M10 18a7 7 0 100-14 7 7 0 000 14zM10 7v4l2 2M8 1h4M10 1v3'],
  ['Остановить', 'M6 11V5a1.5 1.5 0 013 0v5V3.5a1.5 1.5 0 013 0V10V5a1.5 1.5 0 013 0v7a6 6 0 01-6 6h-1a5 5 0 01-4-2l-3-4a1.5 1.5 0 012.3-2z'],
  ['Тревога', 'M10 3a5 5 0 015 5v4l2 3H3l2-3V8a5 5 0 015-5zM8 17a2 2 0 004 0M3 5l-1-1M17 5l1-1'],
  ['Сообщение', 'M2 3h16v11H8l-4 3v-3H2zM10 6v4M10 12v.5'],
  ['Закрыть карточку', 'M4 4l12 12M16 4L4 16'],
];

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

  const { ordered, main } = orderServices(serviceIds, serviceShort);

  return (
    <>
      <footer className={clsx(s.bar, mode === 'dds' && s.dds)} aria-label="Список оповещения">
        <span className={s.caption}>Службы:</span>

        {/* Вкладки по кадру боевой системы: трубка слева, крестик справа,
            короткое имя; не влезающие в ряд встают рядом выше */}
        <div className={s.list}>
          {ordered.map((id) => {
            const auto = autoIds.includes(id);
            return (
              <span key={id} className={s.tag} data-testid={`service-${id}`}
                    title={serviceTitle(id)}>
                <svg className={s.phone} viewBox="0 0 12 12" width="11" height="11" aria-hidden="true">
                  <path d="M2.5 1l2 2.2-1 1.2c.6 1.3 1.8 2.5 3.1 3.1l1.2-1 2.2 2-1.1 1.5C5.3 10 2 6.7 1 3.1z"
                        fill="currentColor" />
                </svg>
                {mode === '112' && (
                  // Подобранную классификатором службу снять нельзя
                  // (инструкция, раздел 6) — крестик есть, но неактивен
                  <button className={s.tagX} aria-label={`Убрать ${serviceTitle(id)}`}
                          disabled={auto}
                          title={auto ? 'Подобрана классификатором, снять нельзя' : undefined}
                          onClick={() => toggle(id)}>×</button>
                )}
                {/* На плашке боевая система пишет короткое имя — «Служба 101»,
                    «Деп. ЖКХ»; полное видно по наведению и в окне добавления */}
                <span className={clsx(s.tagName, id === main && s.main)}>{serviceShort(id)}</span>
              </span>
            );
          })}
          {children}
        </div>

        {mode === '112' && (
          <>
            {serviceIds.length > 7 && (
              <button {...stubProps('Свернуть службы')} className={s.square}>
                <svg viewBox="0 0 14 16" width="14" height="16" aria-hidden="true">
                  <path d="M2 6l5-5 5 5M2 10l5 5 5-5" fill="none" stroke="currentColor" strokeWidth="1.6" />
                </svg>
              </button>
            )}
            <button className={s.square} onClick={() => setOpen(true)} aria-label="Добавить службу">
              <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
                <path d="M8 1v14M1 8h14" stroke="currentColor" strokeWidth="1.8" />
              </svg>
            </button>
            <div className={s.right}>
              <button className={s.save} onClick={onSave} data-testid="save-card">сохранить</button>
              {ICONS.map(([name, d]) => (
                <button key={name} {...stubProps(name)} className={clsx(s.square, s.icon)}>
                  <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
                    <path d={d} fill="none" stroke="currentColor" strokeWidth="1.7"
                          strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </button>
              ))}
            </div>
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
