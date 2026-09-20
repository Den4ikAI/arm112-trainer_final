import { useEffect, useRef, useState } from 'react';

/** Секундомер карточки. Стартует при открытии, останавливается при сохранении. */
export const useTimer = (running: boolean) => {
  const [sec, setSec] = useState(0);
  const start = useRef<number | null>(null);

  useEffect(() => {
    if (!running) return;
    if (start.current == null) start.current = Date.now();
    const id = setInterval(() => {
      setSec(Math.floor((Date.now() - (start.current ?? Date.now())) / 1000));
    }, 250);
    return () => clearInterval(id);
  }, [running]);

  return sec;
};

export const fmt = (sec: number) =>
  `${String(Math.floor(sec / 60)).padStart(2, '0')}:${String(sec % 60).padStart(2, '0')}`;
