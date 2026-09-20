import { Field } from '@/shared/ui/Field';
import s from './CallBar.module.scss';

interface Props {
  aon: string; provided: string; onSite: string;
  readOnly?: boolean;
  onChange?: (field: 'phoneAon' | 'phoneProvided' | 'phoneOnSite', v: string) => void;
}

/** Верхняя полоса: сброс вызова, записи, три блока телефонов заявителя. */
export const CallBar = ({ aon, provided, onSite, readOnly, onChange }: Props) => (
  <section className={s.bar} aria-label="Телефоны заявителя">
    <button className={s.hangup} aria-label="Отключить вызов">
      <span className={s.hangupIcon} aria-hidden>&#9742;</span>
    </button>

    <div className={s.calls}>
      <div className={s.callsTitle}>Отключение</div>
      <div className={s.callsBtns}>
        <button className={s.smallBtn}>записи звонков</button>
        <button className={s.smallBtn}>список SMS</button>
      </div>
    </div>

    <PhoneBlock label="АОН" value={aon} readOnly={readOnly}
      onChange={(v) => onChange?.('phoneAon', v)} />
    <PhoneBlock label="предоставленный" value={provided} readOnly={readOnly} copyAon
      onChange={(v) => onChange?.('phoneProvided', v)} />
    <PhoneBlock label="телефон на место" value={onSite} readOnly={readOnly} copyAon
      onChange={(v) => onChange?.('phoneOnSite', v)} />
  </section>
);

const PhoneBlock = ({ label, value, readOnly, copyAon, onChange }: {
  label: string; value: string; readOnly?: boolean; copyAon?: boolean;
  onChange: (v: string) => void;
}) => (
  <div className={s.phone}>
    <span className={s.phoneIcon} aria-hidden>&#9742;</span>
    <div className={s.phoneBody}>
      <div className={s.phoneLabel}>{label}</div>
      <Field label={label} value={value} onChange={readOnly ? undefined : onChange}
        placeholder="+7 (   )   -  -" />
    </div>
    {copyAon && <button className={s.aonBtn}>АОН</button>}
  </div>
);
