import { useEffect, useRef, useState } from 'react';

/**
 * Секундомер карточки. Стартует при открытии, останавливается при сохранении.
 *
 * `since` — когда вызов начат на сервере. С ним секундомер после
 * обновления страницы продолжает счёт, а не начинает с нуля: сервер
 * считает время заполнения от начала вызова, и экран должен
 * показывать то же, что потом попадёт в разбор.
 */
export const useTimer = (running: boolean, since?: string | null) => {
  const [sec, setSec] = useState(0);
  const start = useRef<number | null>(null);

  useEffect(() => {
    if (since) start.current = Date.parse(since);
  }, [since]);

  useEffect(() => {
    if (!running) return;
    if (start.current == null) start.current = Date.now();
    // Часы клиента могут отставать от серверных — отрицательное
    // время показывать нельзя
    const tick = () =>
      setSec(Math.max(0, Math.floor((Date.now() - (start.current ?? Date.now())) / 1000)));
    tick();
    const id = setInterval(tick, 250);
    return () => clearInterval(id);
  }, [running, since]);

  return sec;
};

export const fmt = (sec: number) =>
  `${String(Math.floor(sec / 60)).padStart(2, '0')}:${String(sec % 60).padStart(2, '0')}`;
