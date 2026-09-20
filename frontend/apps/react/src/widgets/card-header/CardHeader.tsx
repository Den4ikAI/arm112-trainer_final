import { useNavigate } from 'react-router-dom';
import clsx from '@/shared/lib/clsx';
import { fmt } from '@/shared/lib/useTimer';
import s from './CardHeader.module.scss';
import { stubProps } from '@/shared/ui/Stub';

interface Props {
  number: string;
  savedAt: string;
  operator: string;
  workstation: string;
  /** В режиме 112 — секундомер, в режиме ДДС — кнопки «просмотр»/«дополнение». */
  mode: '112' | 'dds';
  elapsedSec?: number;
  normSec?: number;
}

/**
 * Тёмный блок в правом верхнем углу карточки.
 * Таймер краснеет при превышении норматива — это поведение боевой системы
 * (скриншоты: 00:44 тёмный, 01:03 красный).
 */
export const CardHeader = ({ number, savedAt, operator, workstation, mode, elapsedSec = 0, normSec = 60 }: Props) => {
  const over = elapsedSec > normSec;
  const nav = useNavigate();
  return (
    <section className={s.wrap} aria-label="Сведения о происшествии">
      <div className={s.meta}>
        <div className={s.number}>
          Происшествие <span data-testid="card-number">{number}</span>
          {/* Выход в журнал: без него с открытой карточки не уйти,
              а бросить недозаполненную иногда надо */}
          <button
            className={s.leave}
            data-testid="leave-card"
            title={mode === '112'
              ? 'Вернуться в журнал. Незаполненная карточка не сохранится'
              : 'Вернуться в журнал'}
            onClick={() => nav('/journal')}
          >
            ↩ в журнал
          </button>
        </div>
        <div className={s.line}>Сохр. {savedAt}</div>
        <div className={s.line}>Опер. , АРМ {workstation}, {operator}</div>
      </div>

      {mode === '112' ? (
        <div className={clsx(s.timer, over && s.over)}
             role="timer" aria-label="Время заполнения карточки"
             data-over={over ? 'true' : 'false'} data-testid="card-timer">
          <div className={s.timerValue}>{fmt(elapsedSec)}</div>
          <div className={s.timerUnits}><span>минут</span><span>секунд</span></div>
        </div>
      ) : (
        <div className={s.ddsActions}>
          <button {...stubProps('Просмотр')} className={s.ddsBtn}>просмотр</button>
          <button {...stubProps('Дополнение карточки')} className={s.ddsBtn}>дополнение</button>
        </div>
      )}
    </section>
  );
};
