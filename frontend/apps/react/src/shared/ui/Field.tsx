import s from './Field.module.scss';

interface Props {
  label: string;
  value: string;
  onChange?: (v: string) => void;
  readOnly?: boolean;
  placeholder?: string;
  name?: string;
}

/** Поле карточки: мелкая подпись сверху, ввод с подчёркиванием. */
export const Field = ({ label, value, onChange, readOnly, placeholder, name }: Props) => (
  <label className={s.wrap}>
    <span className={s.label}>{label}</span>
    <input
      className={s.input}
      name={name}
      value={value}
      readOnly={readOnly || !onChange}
      placeholder={placeholder}
      aria-label={label}
      onChange={(e) => onChange?.(e.target.value)}
    />
  </label>
);
