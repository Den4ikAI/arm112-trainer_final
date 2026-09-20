import { useEffect, useState } from 'react';

/**
 * Часы, идущие ровно раз в секунду.
 *
 * Отдельный хук, потому что брать `new Date()` прямо в теле компонента
 * недостаточно: React перерисовывает его только когда что-то меняется,
 * и секунды скачут рывками — по приходу события из канала или по
 * запасному опросу. В боевой ПОВ-112 часы идут непрерывно.
 *
 * Тик выравнивается по границе секунды: иначе за час набегает
 * заметное расхождение с системным временем.
 */
export const useClock = (): Date => {
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;

    const tick = () => {
      const current = new Date();
      setNow(current);
      timer = setTimeout(tick, 1000 - current.getMilliseconds());
    };

    timer = setTimeout(tick, 1000 - now.getMilliseconds());
    return () => clearTimeout(timer);
    // Пересоздавать таймер на каждый тик не нужно — он сам себя продлевает
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return now;
};
