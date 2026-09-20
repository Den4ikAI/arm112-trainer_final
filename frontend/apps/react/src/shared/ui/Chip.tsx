import clsx from '../lib/clsx';
import s from './Chip.module.scss';

interface Props {
  children: React.ReactNode;
  selected?: boolean;
  disabled?: boolean;
  tone?: 'default' | 'danger' | 'accent';
  onClick?: () => void;
  title?: string;
  /** Плашка есть в боевом интерфейсе, но в тренажёре не работает. */
  stub?: string;
}

/** Плашка-переключатель признака. Выбранная — синяя, как в боевой ПОВ-112. */
export const Chip = ({
  children, selected, disabled, tone = 'default', onClick, title, stub,
}: Props) => (
  <button
    type="button"
    className={clsx(s.chip, selected && s.selected, s[tone], disabled && s.disabled)}
    aria-pressed={selected}
    disabled={disabled}
    title={stub ? `${stub} — в тренажёре не реализовано` : title}
    aria-disabled={stub ? true : undefined}
    data-stub={stub ? 'true' : undefined}
    onClick={stub ? undefined : onClick}
  >
    {children}
  </button>
);
