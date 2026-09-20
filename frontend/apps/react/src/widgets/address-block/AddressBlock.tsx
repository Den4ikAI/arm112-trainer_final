import { Field } from '@/shared/ui/Field';
import type { AddressFields } from '@arm112/domain';
import s from './AddressBlock.module.scss';

interface Props {
  value: AddressFields;
  readOnly?: boolean;
  onChange?: (field: keyof AddressFields, v: string) => void;
  onOpenMap?: () => void;
}

/**
 * Блок «Адрес» (Alt+A). Единая адресная строка сверху — в боевой системе
 * из неё подтягиваются Яндекс.Карты / Яндекс.Организации / ФИАС, а остальные
 * поля заполняются автоматически. Номер дома вводится в той же строке.
 */
export const AddressBlock = ({ value, readOnly, onChange, onOpenMap }: Props) => {
  const set = (f: keyof AddressFields) => (v: string) => onChange?.(f, v);

  return (
    <section className={s.wrap} aria-label="Адрес происшествия">
      <div className={s.head}>
        <span className={s.headLabel}>Адрес:</span>
        <button className={s.mapBtn} onClick={onOpenMap} aria-label="Показать на карте">🗺</button>
      </div>

      <div className={s.rawRow}>
        <input
          className={s.raw}
          value={value.raw}
          readOnly={readOnly}
          aria-label="Единая адресная строка"
          placeholder="Москва"
          onChange={(e) => set('raw')(e.target.value)}
        />
        <button className={s.clearX} aria-label="Очистить поиск"
                onClick={() => set('raw')('')}>×</button>
      </div>

      <div className={s.grid}>
        <div className={s.c4}><Field label="Страна:" value={value.country} onChange={set('country')} /></div>
        <div className={s.c4}><Field label="Субъект:" value={value.region} onChange={set('region')} /></div>
        <div className={s.c4}><Field label="Населенный пункт:" value={value.settlement} onChange={set('settlement')} /></div>

        <div className={s.c6}><Field label="Объект:" value={value.object} onChange={set('object')} /></div>
        <div className={s.c3}><Field label="Округ:" value={value.district} onChange={set('district')} /></div>
        <div className={s.c3}><Field label="Район:" value={value.area} onChange={set('area')} /></div>

        <div className={s.c6}><Field label="Улица:" value={value.street} onChange={set('street')} /></div>
        <div className={s.c3}><Field label="Дом/Вл:" value={value.house} onChange={set('house')} /></div>
        <div className={s.c3}><Field label="Корпус:" value={value.building} onChange={set('building')} /></div>

        <div className={s.c2}><Field label="Стр/соор:" value={value.structure} onChange={set('structure')} /></div>
        <div className={s.c2}><Field label="Квартира/офис:" value={value.flat} onChange={set('flat')} /></div>
        <div className={s.c2}><Field label="Подъезд:" value={value.entrance} onChange={set('entrance')} /></div>
        <div className={s.c2}><Field label="Этаж:" value={value.floor} onChange={set('floor')} /></div>
        <div className={s.c2}><Field label="Код:" value={value.code} onChange={set('code')} /></div>
      </div>

      <div className={s.descr}>
        <Field label="Описательный адрес:" value={value.descriptive} onChange={set('descriptive')} />
      </div>

      <div className={s.actions}>
        <button className={s.clearBtn}>очистить адрес</button>
      </div>
    </section>
  );
};
