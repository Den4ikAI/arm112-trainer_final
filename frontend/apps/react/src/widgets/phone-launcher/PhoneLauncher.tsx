import { useEffect, useState, type ReactNode } from 'react';
import clsx from '@/shared/lib/clsx';
import s from './PhoneLauncher.module.scss';

interface Props {
  /** Звонит входящий: кружок пульсирует, окно телефона открывается само. */
  ringing: boolean;
  /** Сам телефон. Остаётся смонтированным и в закрытом окне — иначе
   *  закрытие окна оборвало бы разговор и подписку на события сервера. */
  children: ReactNode;
}

/**
 * Зелёный кружок с трубкой на рабочей панели диспетчера ДДС.
 *
 * Рекомендация заказчика по итогам отбора (п. 8): «виртуальный IP —
 * зелёный кружок с телефоном, при активации которого отображается
 * виртуальный телефон». Кружок закреплён в правом нижнем углу над
 * полосой служб: поля карточки он не закрывает, а виден с любого места
 * страницы. Телефон открывается окном поверх карточки, закрывается
 * крестиком, клавишей Esc или повторным нажатием на кружок.
 */
export const PhoneLauncher = ({ ringing, children }: Props) => {
  const [open, setOpen] = useState(false);

  // Входящий вызов диспетчер не должен искать: окно открывается само.
  // Состояние выводится прямо в отрисовке — без эффекта и лишнего кадра
  const [wasRinging, setWasRinging] = useState(ringing);
  if (ringing !== wasRinging) {
    setWasRinging(ringing);
    if (ringing) setOpen(true);
  }

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  return (
    <>
      <button type="button"
              className={clsx(s.launcher, ringing && s.ringing, open && s.open)}
              data-testid="phone-launcher" data-ringing={ringing ? 'true' : 'false'}
              data-hint="phone"
              aria-expanded={open} aria-controls="ip-phone-window"
              aria-label={ringing ? 'Входящий вызов — открыть IP-телефон' : open ? 'Скрыть IP-телефон' : 'Открыть IP-телефон'}
              title={ringing ? 'Входящий вызов' : 'IP-телефон'}
              onClick={() => setOpen((v) => !v)}>
        <svg viewBox="0 0 24 24" width="28" height="28" aria-hidden="true">
          <path fill="currentColor"
                d="M6.6 10.8c1.4 2.8 3.8 5.1 6.6 6.6l2.2-2.2c.3-.3.7-.4 1-.2 1.1.4 2.3.6 3.6.6.6 0 1 .4 1 1V20c0 .6-.4 1-1 1C10.6 21 3 13.4 3 4c0-.6.4-1 1-1h3.5c.6 0 1 .4 1 1 0 1.3.2 2.5.6 3.6.1.3 0 .7-.2 1l-2.3 2.2z" />
        </svg>
        {ringing && <span className={s.badge} data-testid="phone-launcher-badge">входящий</span>}
      </button>

      {/* `hidden`, а не размонтирование: разговор и секундомер живут
          в телефоне и не должны обрываться, когда окно убрали с глаз */}
      <div id="ip-phone-window" className={s.window} role="dialog" aria-label="Виртуальный IP-телефон"
           hidden={!open} data-testid="phone-window">
        <div className={s.windowHead}>
          <span>Виртуальный IP-телефон</span>
          <button type="button" className={s.close} data-testid="phone-close"
                  aria-label="Скрыть телефон" title="Скрыть (разговор не прервётся)"
                  onClick={() => setOpen(false)}>×</button>
        </div>
        {children}
      </div>
    </>
  );
};
