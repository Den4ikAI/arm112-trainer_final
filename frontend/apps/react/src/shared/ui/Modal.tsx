import { useEffect } from 'react';
import s from './Modal.module.scss';

interface Props {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
}

export const Modal = ({ title, onClose, children, footer }: Props) => {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className={s.backdrop} role="presentation" onClick={onClose}>
      <div className={s.dialog} role="dialog" aria-modal="true" aria-label={title}
           onClick={(e) => e.stopPropagation()}>
        <div className={s.head}>
          <h2 className={s.title}>{title}</h2>
          <button className={s.close} onClick={onClose} aria-label="Закрыть">×</button>
        </div>
        <div className={s.body}>{children}</div>
        {footer && <div className={s.footer}>{footer}</div>}
      </div>
    </div>
  );
};
