import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ApiError, channel, me, SEGMENT_LABEL, type WorkItem, type WorkSection } from '@/shared/api';
import { useTraining } from '@/entities/incident/model';
import { useSession } from '@/entities/session/model';
import clsx from '@/shared/lib/clsx';
import s from './StudentWork.module.scss';

/**
 * Разделы курсанта — то, что выдал преподаватель (Денис, 29.09):
 * у оператора 112 «Учебные вызовы», у диспетчера ДДС «Учебные
 * происшествия», у обоих «Экзамены» и «Тренировка по частям» (блиц:
 * отработка отдельной части карточки).
 *
 * В строке — дата выдачи, тема, статус цветом, балл. Не начатое в
 * идущем занятии начинается, начатое — продолжается, закончённое
 * открывается только на просмотр разбора: пройти его заново нельзя.
 */
const РАЗДЕЛЫ: Record<'op112' | 'dds', Array<[WorkSection, string]>> = {
  op112: [['training', 'Учебные вызовы'], ['exam', 'Экзамены'], ['blitz', 'Тренировка по частям']],
  dds: [['training', 'Учебные происшествия'], ['exam', 'Экзамены'], ['blitz', 'Тренировка по частям']],
};

const СТАТУС: Record<WorkItem['status'], string> = {
  not_started: 'не начато',
  in_progress: 'выполняется',
  done: 'закончено',
  missed: 'не пройдено',
};

const дата = (iso: string | null) => (iso
  ? new Date(iso).toLocaleString('ru-RU', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })
  : '—');

export const StudentWork = () => {
  const user = useSession((st) => st.user);
  const profile = user?.duty === 'dds' ? 'dds' : 'op112';
  const [section, setSection] = useState<WorkSection>(() => {
    try { return (sessionStorage.getItem('work-section') as WorkSection) || 'training'; } catch { return 'training'; }
  });
  const [items, setItems] = useState<WorkItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const start = useTraining((st) => st.start);
  const resume = useTraining((st) => st.resume);
  const review = useTraining((st) => st.review);
  const nav = useNavigate();

  useEffect(() => {
    try { sessionStorage.setItem('work-section', section); } catch { /* приватный режим */ }
    let alive = true;
    const load = () => me.work(section)
      .then((r) => { if (alive) { setItems(r); setError(null); } })
      .catch((e) => { if (alive) setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Нет связи с сервером'); });
    void load();
    // Занятие началось, происшествие выпущено, попытка закончена —
    // список обновляется сам
    const off = channel.on((e) => {
      if (['lesson.started', 'lesson.finished', 'card.dispatched', 'response.recorded',
           'session.finished', 'lagged'].includes(e.type)) void load();
    });
    const timer = setInterval(() => void load(), 15_000);
    return () => { alive = false; off(); clearInterval(timer); };
  }, [section]);

  const open = async (it: WorkItem) => {
    setError(null);
    try {
      if (profile === 'dds') {
        if (it.cardId) nav(`/card-dds/${it.cardId}`);
        return;
      }
      if (it.status === 'done' && it.sessionId) {
        if (await review(it.sessionId)) nav('/debrief');
        return;
      }
      if (it.status === 'in_progress' && it.cardNumber) {
        if (await resume(it.cardNumber)) nav(`/card/${it.cardNumber}`);
        return;
      }
      if (it.status === 'not_started' && it.scenarioId) {
        await start(it.scenarioId);
        const number = useTraining.getState().card?.number;
        if (number) nav(`/card/${number}`);
      }
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не получилось открыть');
    }
  };

  // Экзамен идёт по порядку: начать можно только очередной вызов
  const очередной = new Set<string>();
  const занятия = new Set<string>();
  [...(items ?? [])]
    .filter((it) => it.kind === 'marathon' && it.status === 'not_started')
    .sort((a, b) => (a.seq ?? 0) - (b.seq ?? 0))
    .forEach((it) => {
      if (!занятия.has(it.lessonId)) { занятия.add(it.lessonId); очередной.add(it.id); }
    });

  const действие = (it: WorkItem) => {
    if (profile === 'dds') return it.status === 'done' ? 'Разбор' : 'Открыть';
    if (it.status === 'done') return 'Разбор';
    if (it.status === 'in_progress') return 'Продолжить';
    if (it.status === 'not_started' && (it.kind !== 'marathon' || очередной.has(it.id))) return 'Начать';
    return null;
  };

  // Совместимость с проверками: кнопка начала вызова несёт номер билета
  const testid = (it: WorkItem) => {
    if (it.status !== 'not_started') return `work-${it.id}`;
    return it.ticket != null ? `launch-t${it.ticket}-c${it.callNo}` : `launch-${it.id}`;
  };

  return (
    <section className={s.wrap} aria-label="Мои задания" data-testid="student-work">
      <div className={s.tabs} role="tablist">
        {РАЗДЕЛЫ[profile].map(([key, title]) => (
          <button key={key} role="tab" aria-selected={section === key}
                  className={clsx(s.tab, section === key && s.tabOn)}
                  data-testid={`section-${key}`} onClick={() => setSection(key)}>
            {title}
          </button>
        ))}
      </div>

      {error && <div className={s.error} role="alert">{error}</div>}

      <table className={s.table}>
        <thead>
          <tr>
            <th className={s.thDate}>Выдано</th>
            <th>Тема</th>
            <th className={s.thLesson}>Занятие</th>
            <th className={s.thStatus}>Статус</th>
            <th className={s.thScore}>Балл</th>
            <th className={s.thAction} />
          </tr>
        </thead>
        <tbody>
          {items?.map((it) => {
            const act = действие(it);
            return (
              <tr key={it.id} className={s.row} data-status={it.status}
                  data-testid={`work-row-${it.id}`}>
                <td className={s.date}>{дата(it.issuedAt)}</td>
                <td className={s.topic}>
                  <b>{it.kind === 'marathon' && it.seq != null ? `${it.seq}. ` : ''}{it.topic}</b>
                  <span className={s.detail}>
                    {it.segment ? `${SEGMENT_LABEL[it.segment]} · ` : ''}{it.detail}
                  </span>
                </td>
                <td className={s.lesson}>{it.lessonTitle}</td>
                <td>
                  <span className={clsx(s.badge, s[it.status])} data-testid="work-status">
                    {СТАТУС[it.status]}
                  </span>
                </td>
                <td className={s.score} title={it.hidden ? 'Балл — после окончания экзамена' : undefined}>
                  {it.hidden ? '…' : it.score ?? '—'}
                </td>
                <td className={s.action}>
                  {act && (
                    <button className={clsx(s.btn, it.status === 'done' && s.btnView)}
                            data-testid={testid(it)} onClick={() => void open(it)}>
                      {act}
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {items && items.length === 0 && !error && (
        <p className={s.empty}>
          {section === 'training'
            ? 'Заданий пока нет — их выдаёт преподаватель на занятии.'
            : section === 'exam' ? 'Экзаменов пока не назначено.' : 'Тренировок по частям пока не назначено.'}
        </p>
      )}
    </section>
  );
};
