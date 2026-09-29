import { useEffect, useRef, useState } from 'react';
import {
  analytics, CHECK_GROUP, type CheckGroup, type GroupAnalytics, type StudentAnalytics,
} from '@/shared/api/teacher';
import clsx from '@/shared/lib/clsx';
import s from './Teacher.module.scss';

const fmt = (v: number | null | undefined, digits = 0) => (v == null ? '—' : v.toFixed(digits));

/** Цвет ячейки тепловой карты: чем больше ошибок, тем краснее. */
const heat = (pct: number | null) => {
  if (pct == null) return { background: '#f4f5f6', color: '#999' };
  if (pct >= 50) return { background: '#e53935', color: '#fff' };
  if (pct >= 25) return { background: '#ffb74d', color: '#3a2a00' };
  if (pct > 0) return { background: '#fff3c4', color: '#5a4a00' };
  return { background: '#d7ecd9', color: '#1b5e20' };
};

const short = (g: CheckGroup) => CHECK_GROUP[g].title.split(':')[0];

/**
 * Аналитика по людям: группа — операторы 112 и диспетчеры ДДС
 * отдельно, и один обучающийся.
 *
 * Числа — из сохранённых разборов при текущих весах оценки. Когда
 * попыток нет, так и написано: пустая диаграмма хуже честного «нет
 * данных».
 */
export const PeopleAnalytics = ({ groupId }: { groupId: string }) => {
  const [duty, setDuty] = useState<'op112' | 'dds'>('op112');
  const [data, setData] = useState<GroupAnalytics | null>(null);
  const [student, setStudent] = useState<string | null>(null);
  const [error, setError] = useState(false);
  // Вкладку выбирает сама страница, пока преподаватель её не нажал:
  // у группы диспетчеров вкладка 112 пуста, и аналитика открывалась
  // пустым экраном (UX-проход 1)
  const сам = useRef(true);

  useEffect(() => {
    if (!groupId) return;
    let alive = true;
    setData(null); setError(false);
    void analytics.group(groupId, duty)
      .then((d) => {
        if (!alive) return;
        if (сам.current && duty === 'op112' && d.students.length === 0) {
          сам.current = false;
          setDuty('dds');
          return;
        }
        setData(d);
      })
      .catch(() => { if (alive) setError(true); });
    return () => { alive = false; };
  }, [groupId, duty]);

  const вкладка = (d: 'op112' | 'dds') => { сам.current = false; setDuty(d); };

  if (student) return <StudentView id={student} onBack={() => setStudent(null)} />;

  const ranked = [...(data?.students ?? [])]
    .filter((st) => st.avgScore != null)
    .sort((a, b) => (b.avgScore ?? 0) - (a.avgScore ?? 0));
  const leaders = ranked.slice(0, 3);
  const laggards = ranked.slice(-3).reverse().filter((x) => !leaders.includes(x));
  const maxIssue = Math.max(1, ...(data?.typical.map((t) => t.count) ?? [1]));

  return (
    <section data-testid="people-analytics">
      <div className={s.tabs}>
        <button className={clsx(s.btn, duty === 'op112' && s.on)} aria-pressed={duty === 'op112'}
                onClick={() => вкладка('op112')}>
          Операторы 112
        </button>
        <button className={clsx(s.btn, duty === 'dds' && s.on)} aria-pressed={duty === 'dds'}
                onClick={() => вкладка('dds')}>
          Диспетчеры ДДС
        </button>
      </div>

      {error && <p className={s.empty}>Аналитика недоступна.</p>}
      {!data && !error && <p className={s.empty}>Считаю…</p>}

      {data && (
        <>
          <div className={s.metrics}>
            <div className={clsx(s.card, s.metric)}>
              <span className={s.muted}>Попыток</span><strong>{data.attempts}</strong>
            </div>
            <div className={clsx(s.card, s.metric)}>
              <span className={s.muted}>Средний балл</span><strong>{fmt(data.avgScore)}</strong>
            </div>
            {duty === 'op112' && (
              <div className={clsx(s.card, s.metric)}>
                <span className={s.muted}>Время против норматива</span>
                <strong>{data.timeRatio == null ? '—' : `${Math.round(data.timeRatio * 100)}%`}</strong>
              </div>
            )}
            <div className={clsx(s.card, s.metric)}>
              <span className={s.muted}>Обучающихся</span><strong>{data.students.length}</strong>
            </div>
          </div>

          <div className={s.twoCols}>
            <div className={s.card}>
              <h3 className={s.cardTitle}>Лидеры — и почему</h3>
              {leaders.length === 0 && <p className={s.empty}>Сданных попыток нет.</p>}
              <ol>
                {leaders.map((st) => (
                  <li key={st.userId}>
                    <button className={s.link} onClick={() => setStudent(st.userId)}>{st.fio}</button>
                    {' '}· балл {fmt(st.avgScore)}
                    <div className={s.muted}>{st.topIssues.length ? `ошибается: ${st.topIssues.join('; ')}` : 'без ошибок'}</div>
                  </li>
                ))}
              </ol>
            </div>
            <div className={s.card}>
              <h3 className={s.cardTitle}>Отстающие — и на чём</h3>
              {laggards.length === 0 && <p className={s.empty}>Нет.</p>}
              <ol>
                {laggards.map((st) => (
                  <li key={st.userId}>
                    <button className={s.link} onClick={() => setStudent(st.userId)}>{st.fio}</button>
                    {' '}· балл {fmt(st.avgScore)}
                    <div className={s.muted}>{st.topIssues.join('; ') || '—'}</div>
                  </li>
                ))}
              </ol>
            </div>
          </div>

          <div className={s.twoCols}>
            <div className={s.card}>
              <h3 className={s.cardTitle}>Типичные ошибки группы</h3>
              {data.typical.length === 0 && <p className={s.empty}>Ошибок нет или попыток нет.</p>}
              {data.typical.map((t) => (
                <div key={t.label} className={s.issueBar}>
                  <span title={t.label}>{t.label}</span>
                  <div className={s.issueFill} style={{ width: `${(100 * t.count) / maxIssue}%` }} />
                  <span>{t.count}</span>
                </div>
              ))}
            </div>
            <div className={clsx(s.card, s.scroll)}>
              <h3 className={s.cardTitle}>Тепловая карта: ученик × группа проверок</h3>
              <table className={s.table} data-testid="people-heatmap">
                <thead>
                  <tr>
                    <th>Ученик</th>
                    {data.checks.map((g) => <th key={g} className={s.heat}>{short(g)}</th>)}
                  </tr>
                </thead>
                <tbody>
                  {data.students.map((st) => (
                    <tr key={st.userId}>
                      <td><button className={s.link} onClick={() => setStudent(st.userId)}>{st.fio}</button></td>
                      {data.checks.map((g) => {
                        const r = st.rates.find((x) => x.group === g)?.errorPct ?? null;
                        return <td key={g} className={s.heat} style={heat(r)}>{r == null ? '—' : `${Math.round(r)}%`}</td>;
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className={s.muted}>Доля непройденных проверок группы; «—» — проверок не было.</p>
            </div>
          </div>

          <div className={clsx(s.card, s.scroll)}>
            <h3 className={s.cardTitle}>Обучающиеся</h3>
            <table className={s.table} data-testid="people-table">
              <thead>
                <tr>
                  <th>ФИО</th><th>Попыток</th><th>Средний балл</th>
                  {duty === 'op112' && <th>Время / норматив</th>}
                  <th>Прогноз</th><th>Ошибка прогноза</th><th>На чём ошибается</th>
                </tr>
              </thead>
              <tbody>
                {data.students.map((st) => (
                  <tr key={st.userId}>
                    <td><button className={s.link} data-testid={`student-${st.userId}`}
                                onClick={() => setStudent(st.userId)}>{st.fio}</button></td>
                    <td>{st.attempts}</td>
                    <td>{fmt(st.avgScore)}</td>
                    {duty === 'op112' && <td>{st.timeRatio == null ? '—' : `${Math.round(st.timeRatio * 100)}%`}</td>}
                    <td>{st.forecast ?? '—'}</td>
                    <td>{fmt(st.forecastError, 1)}</td>
                    <td className={s.muted}>{st.topIssues.join('; ') || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </section>
  );
};

const StudentView = ({ id, onBack }: { id: string; onBack: () => void }) => {
  const [d, setD] = useState<StudentAnalytics | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => { void analytics.student(id).then(setD).catch(() => setError(true)); }, [id]);

  if (error) return <p className={s.empty}>Нет данных по обучающемуся.</p>;
  if (!d) return <p className={s.empty}>Загрузка…</p>;
  const scored = d.attempts.filter((a) => a.score != null);
  const maxIssue = Math.max(1, ...d.topIssues.map((t) => t.count));

  return (
    <section data-testid="student-analytics">
      <button className={s.link} onClick={onBack}>← К группе</button>
      <h2 className={s.title} style={{ margin: '8px 0' }}>
        {d.fio} <span className={d.duty === 'dds' ? s.pillBlue : s.pillOrange}>{d.duty === 'dds' ? 'ДДС' : '112'}</span>
      </h2>
      <div className={s.metrics}>
        <div className={clsx(s.card, s.metric)}><span className={s.muted}>Попыток</span><strong>{d.attempts.length}</strong></div>
        <div className={clsx(s.card, s.metric)}><span className={s.muted}>Средний балл</span><strong>{fmt(d.avgScore)}</strong></div>
        {d.rates.map((r) => (
          <div key={r.group} className={clsx(s.card, s.metric)}>
            <span className={s.muted}>{short(r.group)}: ошибок</span>
            <strong style={{ color: (r.errorPct ?? 0) >= 50 ? 'var(--c-danger)' : undefined }}>
              {r.errorPct == null ? '—' : `${Math.round(r.errorPct)}%`}
            </strong>
          </div>
        ))}
      </div>

      <div className={s.twoCols}>
        <div className={s.card}>
          <h3 className={s.cardTitle}>Динамика балла</h3>
          {scored.length === 0 && <p className={s.empty}>Сданных попыток нет.</p>}
          <svg viewBox={`0 0 ${Math.max(2, scored.length) * 40} 110`} width="100%" height="120"
               role="img" aria-label="Балл по попыткам">
            <line x1="0" y1="100" x2={Math.max(2, scored.length) * 40} y2="100" stroke="#ccc" />
            <polyline fill="none" stroke="var(--c-blue)" strokeWidth="2"
                      points={scored.map((a, i) => `${i * 40 + 20},${100 - (a.score ?? 0)}`).join(' ')} />
            {scored.map((a, i) => (
              <g key={i}>
                <circle cx={i * 40 + 20} cy={100 - (a.score ?? 0)} r="3.5" fill="var(--c-blue)" />
                <text x={i * 40 + 20} y={100 - (a.score ?? 0) - 6} fontSize="10" textAnchor="middle">{a.score}</text>
              </g>
            ))}
          </svg>
        </div>
        <div className={s.card}>
          <h3 className={s.cardTitle}>На чём ошибается</h3>
          {d.topIssues.length === 0 && <p className={s.empty}>Ошибок нет.</p>}
          {d.topIssues.map((t) => (
            <div key={t.label} className={s.issueBar}>
              <span title={t.label}>{t.label}</span>
              <div className={s.issueFill} style={{ width: `${(100 * t.count) / maxIssue}%` }} />
              <span>{t.count}</span>
            </div>
          ))}
        </div>
      </div>

      <div className={clsx(s.card, s.scroll)} style={{ marginBottom: 12 }}>
        <h3 className={s.cardTitle}>История попыток</h3>
        <table className={s.table}>
          <thead><tr><th>Дата</th><th>Вид</th><th>Карточка</th><th>Балл</th><th>Время / норматив</th><th>Ошибки</th></tr></thead>
          <tbody>
            {d.attempts.slice().reverse().map((a) => (
              <tr key={`${a.kind}-${a.cardId}`}>
                <td>{new Date(a.at).toLocaleString('ru-RU')}</td>
                <td>{a.kind === 'dds' ? 'ДДС' : '112'}</td>
                <td>{a.cardNumber}</td>
                <td><strong>{a.score ?? '—'}</strong></td>
                <td>{a.elapsedSec != null && a.normSec ? `${a.elapsedSec} с / ${a.normSec} с` : '—'}</td>
                <td className={s.muted}>{a.issues.join('; ') || '—'}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className={clsx(s.card, s.scroll)}>
        <h3 className={s.cardTitle}>Прогноз против факта</h3>
        {d.forecast.length === 0 && <p className={s.empty}>Прогнозов ещё не было.</p>}
        {d.forecast.length > 0 && (
          <table className={s.table}>
            <thead><tr><th>Вызов</th><th>Прогноз</th><th>Факт</th><th>Разница</th></tr></thead>
            <tbody>
              {d.forecast.map((f, i) => (
                <tr key={i}>
                  <td>{new Date(f.at).toLocaleString('ru-RU')}</td>
                  <td>{f.predicted}</td>
                  <td>{f.actual ?? '—'}</td>
                  <td>{f.actual == null ? '—' : f.actual - f.predicted}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
};
