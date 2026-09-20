import clsx from '../lib/clsx';
import s from './Chip.module.scss';

interface Props {
  children: React.ReactNode;
  selected?: boolean;
  disabled?: boolean;
  tone?: 'default' | 'danger' | 'accent';
  onClick?: () => void;
  title?: string;
}

/** Плашка-переключатель признака. Выбранная — синяя, как в боевой ПОВ-112. */
export const Chip = ({ children, selected, disabled, tone = 'default', onClick, title }: Props) => (
  <button
    type="button"
    className={clsx(s.chip, selected && s.selected, s[tone], disabled && s.disabled)}
    aria-pressed={selected}
    disabled={disabled}
    title={title}
    onClick={onClick}
  >
    {children}
  </button>
);
