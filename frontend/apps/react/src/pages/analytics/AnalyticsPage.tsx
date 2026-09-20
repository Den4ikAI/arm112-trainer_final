import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { reports, type GroupBrief, type GroupStats } from '@/shared/api';
import { useSession } from '@/entities/session/model';
import { FunnelChart, Heatmap, KindsChart, TimeChart } from '@/widgets/charts';
import clsx from '@/shared/lib/clsx';
import s from './AnalyticsPage.module.scss';

/**
 * Аналитика по группе.
 *
 * Все числа здесь посчитаны базой по сохранённым разборам. Ни одной
 * декоративной диаграммы: когда данных нет, вместо графика стоит
 * прямая надпись, что показывать нечего. График на выдуманных числах
 * в тренажёре для диспетчеров — не украшение, а способ принять
 * неверное решение о подготовке людей.
 *
 * Отчёт считается в очереди: норматив ТЗ на подготовку — 30 секунд,
 * держать соединение всё это время незачем.
 */
export const AnalyticsPage = () => {
  const [groups, setGroups] = useState<GroupBrief[]>([]);
  const [groupId, setGroupId] = useState('');
  const [stats, setStats] = useState<GroupStats | null>(null);
  const [state, setState] = useState<'idle' | 'working' | 'error'>('idle');
  const [insights, setInsights] = useState<string | null>(null);
  const user = useSession((st) => st.user);
  const nav = useNavigate();

  useEffect(() => {
    void reports.groups()
      .then((g) => {
        setGroups(g);
        if (g.length) setGroupId(g[0].id);
      })
      .catch(() => setState('error'));
  }, []);

  const load = useCallback(async (id: string) => {
    if (!id) return;
    setState('working');
    setStats(null);
    setInsights(null);
    try {
      const { jobId } = await reports.group(id);
      setStats(await reports.awaitJob(jobId) as GroupStats);
      setState('idle');
    } catch {
      setState('error');
    }
  }, []);

  useEffect(() => { void load(groupId); }, [groupId, load]);

  const askInsights = async () => {
    setInsights('считаю…');
    try {
      const { jobId } = await reports.insights(groupId);
      const r = await reports.awaitJob(jobId) as { text?: string };
      setInsights(r.text ?? 'нет данных');
    } catch {
      setInsights('не удалось получить сводку');
    }
  };

  if (!user || user.role === 'student') {
    return (
      <div className={s.page}>
        <p className={s.empty}>Раздел доступен преподавателю и администратору.</p>
        <button className={s.plain} onClick={() => nav('/journal')}>В журнал</button>
      </div>
    );
  }

  return (
    <div className={s.page}>
      <header className={s.head}>
        <h1 className={s.title}>Аналитика группы</h1>
        <div className={s.tools}>
          <select value={groupId} aria-label="Группа"
                  onChange={(e) => setGroupId(e.target.value)}>
            {groups.map((g) => <option key={g.id} value={g.id}>{g.title}</option>)}
          </select>
          <button className={s.plain} onClick={() => void load(groupId)}>обновить</button>
          <button className={s.plain} onClick={() => nav('/journal')}>← В журнал</button>
        </div>
      </header>

      {state === 'working' && <p className={s.empty}>Отчёт считается…</p>}
      {state === 'error' && <p className={s.empty}>Не удалось получить отчёт.</p>}

      {stats && (
        <>
          <section className={s.metrics} data-testid="analytics-metrics">
            <Metric label="Занятий" value={String(stats.sessions)} />
            <Metric label="Средний балл"
                    value={stats.avgScore == null ? '—' : stats.avgScore.toFixed(0)} />
            <Metric label="Среднее время"
                    value={stats.avgElapsedSec == null ? '—' : `${stats.avgElapsedSec.toFixed(0)} с`}
                    note={stats.avgNormSec == null ? undefined
                      : `норматив ${stats.avgNormSec.toFixed(0)} с`} />
            {/* Четверть угаданного — уже повод разбираться с группой */}
            <Metric label="Угадано, а не выяснено"
                    value={`${Math.round(stats.guessedShare * 100)}%`}
                    danger={stats.guessedShare > 0.25}
                    note={`${stats.guessed} из ${stats.guessed + stats.ok}`} />
          </section>

          <Block title="Выяснил или угадал"
                 hint="Обе доли совпали с эталоном. Работа оператора — только первая.">
            <FunnelChart funnel={stats.funnel} />
          </Block>

          <Block title="Тепловая карта ошибок"
                 hint="Поле карточки против вида замечания. Насыщенность — доля от самой частой ошибки.">
            <Heatmap cells={stats.heatmap} />
          </Block>

          <Block title="Время заполнения против норматива"
                 hint="Каждая точка — занятие. Красные выше черты норматива.">
            <TimeChart points={stats.timeline} />
          </Block>

          <Block title="Виды замечаний">
            <KindsChart kinds={stats.byKind} />
          </Block>

          <Block title="Сводка по группе">
            {insights
              ? <p className={s.insights} data-testid="insights">{insights}</p>
              : (
                <button className={s.primary} data-testid="ask-insights"
                        onClick={() => void askInsights()}>
                  собрать сводку
                </button>
              )}
            <p className={s.note}>
              Числа считает база, модель только формулирует текст —
              выдумать процент она не может.
            </p>
          </Block>
        </>
      )}
    </div>
  );
};

const Metric = ({ label, value, note, danger }: {
  label: string; value: string; note?: string; danger?: boolean;
}) => (
  <div className={clsx(s.metric, danger && s.metricDanger)}>
    <div className={s.metricLabel}>{label}</div>
    <div className={s.metricValue}>{value}</div>
    {note && <div className={s.metricNote}>{note}</div>}
  </div>
);

const Block = ({ title, hint, children }: {
  title: string; hint?: string; children: React.ReactNode;
}) => (
  <section className={s.block}>
    <h2 className={s.blockTitle}>{title}</h2>
    {hint && <p className={s.blockHint}>{hint}</p>}
    {children}
  </section>
);
