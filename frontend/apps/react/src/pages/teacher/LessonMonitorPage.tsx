import { useCallback, useEffect, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { lessons } from '@/shared/api';
import { monitor, type LessonMonitor, type MonitorSeat } from '@/shared/api/teacher';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import clsx from '@/shared/lib/clsx';
import s from './Teacher.module.scss';

/** м:сс */
const mmss = (sec: number) => {
  const v = Math.max(0, Math.floor(sec));
  return `${Math.floor(v / 60)}:${String(v % 60).padStart(2, '0')}`;
};

/**
 * Ход занятия — «весь класс на одном экране».
 *
 * Заказчик на встрече: преподаватель ходит по классу и визуально
 * контролирует каждого; удалённый контроль — то же на отдельном
 * компьютере. Здесь место за местом, операторы 112 и диспетчеры ДДС
 * отдельно: что в работе, сколько прошло против норматива, как далеко
 * заполнена карточка. Опаздывающие — в красной рамке.
 */
export const LessonMonitorPage = () => {
  const { id = '' } = useParams();
  const [params, setParams] = useSearchParams();
  const projector = params.get('projector') === '1';
  const user = useSession((st) => st.user);
  const nav = useNavigate();
  const [m, setM] = useState<LessonMonitor | null>(null);
  const [fetchedAt, setFetchedAt] = useState(() => Date.now());
  const [tick, setTick] = useState(() => Date.now());
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await monitor.lesson(id);
      setM(r); setFetchedAt(Date.now()); setError(null);
    } catch {
      setError('Ход занятия недоступен');
    }
  }, [id]);

  // Опрос раз в 2,5 с и секундный тик для таймеров между опросами
  useEffect(() => {
    void load();
    const poll = window.setInterval(() => void load(), 2500);
    const t = window.setInterval(() => setTick(Date.now()), 1000);
    return () => { window.clearInterval(poll); window.clearInterval(t); };
  }, [load]);

  if (!user || user.role === 'student') {
    return <div className={s.page}><p className={s.empty}>Раздел доступен преподавателю.</p></div>;
  }
  if (!m) return <div className={s.page}><p className={s.empty}>{error ?? 'Загрузка…'}</p></div>;

  const drift = Math.max(0, (tick - fetchedAt) / 1000);
  const running = !!m.startedAt && !m.finishedAt;
  const runningSec = m.startedAt
    ? ((m.finishedAt ? new Date(m.finishedAt) : new Date(m.now)).getTime()
       - new Date(m.startedAt).getTime()) / 1000 + (running ? drift : 0)
    : 0;

  const finish = async () => {
    await lessons.finish(m.lessonId);
    await load();
  };

  return (
    <div className={clsx(s.page, projector && s.projector)} data-testid="lesson-monitor">
      {!projector && (
        <header className={s.head}>
          <div>
            <button className={s.link} onClick={() => nav('/lessons')}>← Занятия</button>
            <h1 className={s.title}>
              {m.title}{' '}
              <span className={running ? s.pillBlue : s.pill}>
                {m.finishedAt ? 'завершено' : m.startedAt ? 'идёт' : 'не начато'}
              </span>
            </h1>
            <p className={s.sub}>
              {m.groupTitle} · мест: {m.seats} ·{' '}
              {m.mode === 'dds' ? 'карточки ДДС' : 'вызовы 112'}
              {m.kind === 'marathon' ? ' · экзамен' : m.kind === 'blitz' ? ' · блиц' : ''}
              {m.startedAt && ` · начало ${new Date(m.startedAt).toLocaleString('ru-RU')}`}
            </p>
          </div>
          <div className={s.tools}>
            <button className={s.btn} onClick={() => nav('/lessons')}>Проверка попыток</button>
            <button className={s.btn} onClick={() => nav('/analytics')}>Отчёт</button>
            <button className={s.btn} onClick={() => setParams({ projector: '1' })}>⛶ Режим проектора</button>
            {running && (
              <button className={s.danger} data-testid="monitor-finish"
                      onClick={() => void finish()}>■ Завершить занятие</button>
            )}
            <UserMenu compact />
          </div>
        </header>
      )}
      {projector && (
        <button className={s.btn} style={{ marginBottom: 8 }}
                onClick={() => setParams({})}>Выйти из режима проектора</button>
      )}

      <div className={clsx(s.card, s.statusLine)} data-testid="monitor-status">
        {running && <><span className={s.dot} /><span className={s.live}>Идёт {mmss(runningSec)}</span></>}
        <span>· {m.seats} мест · работают {m.working}</span>
        <span className={m.late > 0 ? s.pillRed : s.pill}>опаздывают сейчас: {m.late}</span>
        <span className={m.notNotified > 0 ? s.pillRed : s.pill}>Не оповещено: {m.notNotified}</span>
        <span className={s.pill}>Отказ: {m.refused}</span>
        <span className={s.pill}>Не завершено: {m.notFinished}</span>
        <span style={{ marginLeft: 'auto' }} className={s.muted}>
          На проверке: {m.reviewed} из {m.graded}
        </span>
      </div>

      <h2 className={s.blockTitle}>Операторы 112 <span className={s.muted}>({m.op112.length})</span></h2>
      {m.op112.length === 0 && <p className={s.empty}>В группе нет операторов 112.</p>}
      <div className={s.seats} data-testid="monitor-112">
        {m.op112.map((seat) => <Seat key={seat.userId} seat={seat} drift={running ? drift : 0} />)}
      </div>

      <h2 className={s.blockTitle}>Диспетчеры ДДС <span className={s.muted}>({m.dds.length})</span></h2>
      {m.dds.length === 0 && <p className={s.empty}>В группе нет диспетчеров ДДС.</p>}
      <div className={s.seats} data-testid="monitor-dds">
        {m.dds.map((seat) => <Seat key={seat.userId} seat={seat} drift={running ? drift : 0} />)}
      </div>
    </div>
  );
};

const Seat = ({ seat, drift }: { seat: MonitorSeat; drift: number }) => {
  const serviceShort = useCatalog((st) => st.serviceShort);
  const dds = seat.duty === 'dds';
  const c = seat.current;
  const elapsed = c ? c.elapsedSec + drift : 0;
  const late = (c && (c.late || elapsed > c.normSec) && (!dds || !c.status)) || seat.notNotified > 0;
  const labels = dds ? ['Открыл', 'Ответил', 'Сдал'] : ['Вызовов', 'Сохранил', 'Сдал'];

  return (
    <article className={clsx(s.seat, late && s.seatLate)} data-testid={`seat-${seat.userId}`}>
      <div className={s.seatHead}>
        <span>Место {seat.workstation ?? seat.seat}</span>
        <span className={dds ? s.pillBlue : s.pillOrange}>{dds ? 'ДДС' : '112'}</span>
        {dds && seat.serviceId && <span className={s.muted}>{serviceShort(seat.serviceId)}</span>}
        {seat.queue > 0 && <span className={s.pillYellow}>в очереди {seat.queue}</span>}
      </div>
      <div className={s.fio}>{seat.fio}</div>
      <div className={s.muted}>задания ≈ {seat.tasksDone} из {seat.tasksTotal}</div>

      {c ? (
        <>
          <div className={s.current}>
            <div>№ {c.number} · {c.theme}</div>
            <div className={s.muted}>{c.address || 'адрес ещё не указан'}</div>
          </div>
          <div className={clsx(s.timer, late && s.timerLate)}>
            <strong>{mmss(elapsed)}</strong>
            {dds
              ? <span>работа по карточке{c.status ? ` · ${c.status}` : ' · первая запись — норматив ' + mmss(c.normSec)}</span>
              : <span>набор карточки · норматив {mmss(c.normSec)}</span>}
          </div>
          {!dds && (
            <div className={s.steps} aria-label="Ход заполнения">
              {c.steps.map((st) => (
                <span key={st.key} className={st.done ? s.stepDone : s.step}>
                  {st.done ? '✓ ' : ''}{st.label}
                </span>
              ))}
            </div>
          )}
        </>
      ) : (
        <div className={s.current}><div className={s.muted}>Нет открытой карточки</div></div>
      )}

      <div className={s.counters}>
        {labels.map((l, i) => <div key={l}>{l}<strong>{seat.counters[i]}</strong></div>)}
      </div>
      {seat.notNotified > 0 && <span className={s.pillRed}>Не оповещено {seat.notNotified}</span>}
      <div className={s.muted}>
        {seat.lastScore == null ? 'Попыток ещё нет' : `Последний балл: ${seat.lastScore}`}
      </div>
    </article>
  );
};
