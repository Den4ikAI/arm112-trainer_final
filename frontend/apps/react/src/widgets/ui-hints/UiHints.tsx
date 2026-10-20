import { useCallback, useEffect, useLayoutEffect, useState } from 'react';
import clsx from '@/shared/lib/clsx';
import { HINTS, type Hint, type HintPage } from './hints';
import s from './UiHints.module.scss';

interface Props {
  page: HintPage;
  /** Только значок «?» — для тесных мест вроде блока пользователя в ленте. */
  compact?: boolean;
}

/** Найденная на странице подсказка и место её окна. */
interface Placed extends Hint {
  left: number;
  top: number;
  width: number;
  height: number;
}

const SEEN_KEY = 'arm112.hints.seen';

const seenBefore = () => {
  try { return localStorage.getItem(SEEN_KEY) === '1'; } catch { return false; }
};

const markSeen = () => {
  try { localStorage.setItem(SEEN_KEY, '1'); } catch { /* приватный режим */ }
};

const reducedMotion = () =>
  typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

/** Окна, которые есть на странице сейчас, с их координатами. */
const place = (hints: Hint[]): Placed[] => hints.flatMap((h) => {
  const el = document.querySelector(h.selector);
  if (!(el instanceof HTMLElement)) return [];
  const r = el.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) return [];
  return [{ ...h, left: r.left, top: r.top, width: r.width, height: r.height }];
});

/**
 * Режим подсказок по интерфейсу рабочего места ДДС.
 *
 * Кнопка «?» включает накладку: каждое функциональное окно обведено и
 * пронумеровано, текущее — с пояснением, что это и когда этим
 * пользоваться. «Далее»/«Назад» ведут по окнам в порядке работы по
 * памятке; номер на рамке открывает подсказку сразу. Накладка не
 * перестраивает вёрстку: окна измеряются по месту, рамки рисуются
 * поверх и не ловят щелчки — интерфейс под ними живой.
 *
 * Первое включение запоминается в localStorage: пока курсант не
 * заглянул в подсказки, кнопка подписана и подсвечена.
 */
export const UiHints = ({ page, compact }: Props) => {
  const [on, setOn] = useState(false);
  const [step, setStep] = useState(0);
  const [placed, setPlaced] = useState<Placed[]>([]);
  const [seen, setSeen] = useState(seenBefore);

  // Ничего не сдвинулось — состояние не трогаем: сама накладка тоже
  // меняет DOM, и без этой проверки промер звал бы сам себя по кругу
  const measure = useCallback(() => setPlaced((prev) => {
    const next = place(HINTS[page]);
    const same = prev.length === next.length && prev.every((p, i) => {
      const n = next[i];
      return p.key === n.key && p.left === n.left && p.top === n.top
        && p.width === n.width && p.height === n.height;
    });
    return same ? prev : next;
  }), [page]);

  // Окна меряются при включении и перемеряются, когда страница
  // прокручивается, меняет размер или перерисовывается: панели
  // карточки появляются по ходу работы
  useLayoutEffect(() => {
    if (!on) return;
    let raf = 0;
    const later = () => {
      if (raf) return;
      raf = window.requestAnimationFrame(() => { raf = 0; measure(); });
    };
    later();
    window.addEventListener('resize', later);
    window.addEventListener('scroll', later, true);
    const mo = new MutationObserver(later);
    mo.observe(document.body, { childList: true, subtree: true, attributes: true });
    return () => {
      window.removeEventListener('resize', later);
      window.removeEventListener('scroll', later, true);
      mo.disconnect();
      if (raf) window.cancelAnimationFrame(raf);
    };
  }, [on, measure]);

  useEffect(() => {
    if (!on) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOn(false);
      else if (e.key === 'ArrowRight') setStep((n) => n + 1);
      else if (e.key === 'ArrowLeft') setStep((n) => Math.max(0, n - 1));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [on]);

  const total = placed.length;
  const index = total ? Math.min(step, total - 1) : 0;
  const current = placed[index];

  // Текущее окно — в поле зрения. Зависимость — селектор, а не объект:
  // объекты пересоздаются каждым промером, а прокручивать надо
  // только при смене шага
  const currentSelector = current?.selector;
  useEffect(() => {
    if (!on || !currentSelector) return;
    document.querySelector(currentSelector)
      ?.scrollIntoView({ block: 'nearest', behavior: reducedMotion() ? 'auto' : 'smooth' });
  }, [on, currentSelector]);

  const toggle = () => {
    if (!on) {
      setStep(0);
      markSeen();
      setSeen(true);
    }
    setOn((v) => !v);
  };

  // Пояснение — под окном, если есть место, иначе над ним; по ширине
  // не вылезает за экран
  const calloutStyle = (p: Placed) => {
    const w = Math.min(360, window.innerWidth - 32);
    const left = Math.max(16, Math.min(p.left, window.innerWidth - w - 16));
    const below = p.top + p.height + 8;
    const fits = below + 190 < window.innerHeight;
    return fits
      ? { left, top: below, width: w }
      : { left, bottom: Math.max(16, window.innerHeight - p.top + 8), width: w };
  };

  return (
    <>
      <button type="button"
              className={clsx(s.toggle, compact && s.toggleCompact, on && s.toggleOn, !seen && s.toggleNew)}
              data-testid="hints-toggle" data-seen={seen ? 'true' : 'false'}
              aria-pressed={on} title={on ? 'Выключить подсказки' : 'Подсказки по интерфейсу'}
              onClick={toggle}>
        <span className={s.toggleMark} aria-hidden="true">?</span>
        {!compact && <span className={s.toggleText}>{on ? 'Выключить подсказки' : 'Подсказки'}</span>}
      </button>

      {on && (
        <>
          <div className={s.layer} data-testid="hints-layer" aria-hidden="true">
            {placed.map((p, i) => (
              <div key={p.key} className={clsx(s.box, i === index && s.boxOn)}
                   style={{ left: p.left, top: p.top, width: p.width, height: p.height }}>
                <button type="button" className={s.num} tabIndex={-1}
                        data-testid={`hint-num-${p.key}`}
                        onClick={() => setStep(i)}>{i + 1}</button>
              </div>
            ))}
          </div>

          {current && (
            <aside className={s.callout} style={calloutStyle(current)} role="note"
                   data-testid="hint-callout" data-hint-key={current.key}>
              <div className={s.calloutHead}>
                <span className={s.calloutNum}>{index + 1}</span>
                <strong data-testid="hint-title">{current.title}</strong>
              </div>
              <p className={s.calloutText} data-testid="hint-text">{current.text}</p>
            </aside>
          )}

          <div className={s.bar} role="toolbar" aria-label="Подсказки по интерфейсу" data-testid="hints-bar">
            <span className={s.counter} data-testid="hints-counter">
              {total ? `${index + 1} из ${total}` : 'окон не найдено'}
            </span>
            <button type="button" className={s.navBtn} data-testid="hints-prev"
                    disabled={index === 0} onClick={() => setStep(Math.max(0, index - 1))}>
              ‹ Назад
            </button>
            <button type="button" className={s.navBtn} data-testid="hints-next"
                    disabled={index >= total - 1} onClick={() => setStep(index + 1)}>
              Далее ›
            </button>
            <button type="button" className={clsx(s.navBtn, s.navClose)} data-testid="hints-close"
                    onClick={() => setOn(false)}>
              Закрыть
            </button>
          </div>
        </>
      )}
    </>
  );
};
