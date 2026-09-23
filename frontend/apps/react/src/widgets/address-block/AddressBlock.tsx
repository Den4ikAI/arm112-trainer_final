import { Field } from '@/shared/ui/Field';
import s from './AddressBlock.module.scss';
import { stubProps } from '@/shared/ui/Stub';

interface Props {
  /** Адрес приходит с сервера разреженным: заполненные поля и только. */
  value: Record<string, string>;
  readOnly?: boolean;
  onChange?: (field: string, v: string) => void;
  onOpenMap?: () => void;
}

/**
 * Блок «Адрес» (Alt+A). Единая адресная строка сверху — в боевой системе
 * из неё подтягиваются Яндекс.Карты / Яндекс.Организации / ФИАС, а остальные
 * поля заполняются автоматически. Номер дома вводится в той же строке.
 */
export const AddressBlock = ({ value, readOnly, onChange, onOpenMap }: Props) => {
  const set = (f: string) => (v: string) => onChange?.(f, v);
  // Незаполненное поле — пустая строка, иначе React ругается на
  // переход с неуправляемого поля на управляемое
  const get = (f: string) => value[f] ?? '';

  // Раньше кнопка была заглушкой «в тренажёре не реализовано», хотя
  // в боевой системе она есть и нужна: заявитель назвал не тот адрес,
  // а переписывать поля по одному долго
  const очистить = () => {
    for (const [поле, значение] of Object.entries(value)) {
      if (значение) onChange?.(поле, '');
    }
  };

  return (
    <section className={s.wrap} aria-label="Адрес происшествия">
      <div className={s.head}>
        <span className={s.headLabel}>Адрес:</span>
        <button className={s.mapBtn} aria-label="Показать на карте"
                {...(onOpenMap ? { onClick: onOpenMap } : stubProps('Карта'))}>🗺</button>
      </div>

      <div className={s.rawRow}>
        <input
          className={s.raw}
          value={get('raw')}
          readOnly={readOnly}
          aria-label="Единая адресная строка"
          placeholder="Москва"
          onChange={(e) => set('raw')(e.target.value)}
        />
        <button className={s.clearX} aria-label="Очистить поиск"
                onClick={() => set('raw')('')}>×</button>
      </div>

      <div className={s.grid}>
        <div className={s.c4}><Field label="Страна:" value={get('country')} onChange={set('country')} /></div>
        <div className={s.c4}><Field label="Субъект:" value={get('region')} onChange={set('region')} /></div>
        <div className={s.c4}><Field label="Населенный пункт:" value={get('settlement')} onChange={set('settlement')} /></div>

        <div className={s.c6}><Field label="Объект:" value={get('object')} onChange={set('object')} /></div>
        <div className={s.c3}><Field label="Округ:" value={get('district')} onChange={set('district')} /></div>
        <div className={s.c3}><Field label="Район:" value={get('area')} onChange={set('area')} /></div>

        <div className={s.c6}><Field label="Улица:" value={get('street')} onChange={set('street')} /></div>
        <div className={s.c3}><Field label="Дом/Вл:" value={get('house')} onChange={set('house')} /></div>
        <div className={s.c3}><Field label="Корпус:" value={get('building')} onChange={set('building')} /></div>

        <div className={s.c2}><Field label="Стр/соор:" value={get('structure')} onChange={set('structure')} /></div>
        <div className={s.c2}><Field label="Квартира/офис:" value={get('flat')} onChange={set('flat')} /></div>
        <div className={s.c2}><Field label="Подъезд:" value={get('entrance')} onChange={set('entrance')} /></div>
        <div className={s.c2}><Field label="Этаж:" value={get('floor')} onChange={set('floor')} /></div>
        <div className={s.c2}><Field label="Код:" value={get('code')} onChange={set('code')} /></div>
      </div>

      <div className={s.descr}>
        <Field label="Описательный адрес:" value={get('descriptive')} onChange={set('descriptive')} />
      </div>

      {!readOnly && (
        <div className={s.actions}>
          <button className={s.clearBtn} onClick={очистить} data-testid="clear-address">
            очистить адрес
          </button>
        </div>
      )}
    </section>
  );
};
