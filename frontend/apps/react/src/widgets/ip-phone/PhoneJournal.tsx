import { useEffect, useState } from 'react';
import { channel, phone, type PhoneCall } from '@/shared/api';
import { PhoneLog } from './PhoneLog';
import s from './IpPhone.module.scss';

/**
 * Журнал IP-телефона по карточке для преподавателя: все вызовы всех
 * диспетчеров, реплики и записи разговоров. Перечитывается по событиям
 * карточки — разговор идёт, пока преподаватель смотрит.
 */
export const PhoneJournal = ({ cardId }: { cardId: string }) => {
  const [calls, setCalls] = useState<PhoneCall[] | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () => void phone.view(cardId)
      .then((v) => { if (alive) setCalls(v.calls); })
      .catch(() => undefined);
    load();
    const off = channel.on((e) => { if (e.cardId === cardId) load(); });
    // Вызовы сами событий не шлют: раз в полминуты подтягиваем новые
    const t = window.setInterval(load, 30_000);
    return () => { alive = false; off(); window.clearInterval(t); };
  }, [cardId]);

  if (!calls) return null;
  return (
    <section className={s.phone} aria-label="Журнал IP-телефона" data-testid="phone-journal">
      <header className={s.head}>
        <span className={s.name}>Журнал IP-телефона</span>
        <span className={s.ext}>вызовов: {calls.length}</span>
      </header>
      <PhoneLog calls={calls} showDispatcher />
    </section>
  );
};
