import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useSession } from '@/entities/session/model';
import { ApiError } from '@/shared/api';
import {
  CHECK_GROUP, CHECK_ORDER, grading, type Weights, type WeightsPreview,
} from '@/shared/api/teacher';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import clsx from '@/shared/lib/clsx';
import s from './Teacher.module.scss';

const fmt = (v: number | null | undefined) => (v == null ? '—' : v.toFixed(0));

/**
 * Веса оценки.
 *
 * Преподаватель — конечный валидатор (ответ заказчика 27.09), поэтому
 * что в карточке важнее, решает он. Ползунок двигается — баллы всех
 * попыток пересчитываются на сервере сразу, но только для показа;
 * сохраняются и попадают в журнал действий только по кнопке.
 */
export const WeightsPage = () => {
  const user = useSession((st) => st.user);
  const nav = useNavigate();
  const [saved, setSaved] = useState<Weights | null>(null);
  const [defaults, setDefaults] = useState<Weights | null>(null);
  const [w, setW] = useState<Weights | null>(null);
  const [cap, setCap] = useState(40);
  const [preview, setPreview] = useState<WeightsPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    void grading.view().then((v) => {
      setSaved(v.weights); setDefaults(v.defaults); setW(v.weights); setCap(v.criticalCap);
    }).catch(() => setNote('Веса недоступны'));
  }, []);

  // Пересчёт «стало» с небольшой задержкой: ползунок тянут, а не щёлкают
  useEffect(() => {
    if (!w) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      void grading.preview(w).then(setPreview).catch(() => setPreview(null));
    }, 250);
    return () => window.clearTimeout(timer.current);
  }, [w]);

  if (!user || user.role === 'student') {
    return <div className={s.page}><p className={s.empty}>Раздел доступен преподавателю.</p></div>;
  }

  const save = async () => {
    if (!w) return;
    setBusy(true); setNote(null);
    try {
      const r = await grading.save(w);
      setSaved(w);
      setNote(`Сохранено. Пересчитано попыток: ${r.changed} из ${r.total}.`);
      setPreview(await grading.preview(w));
    } catch (e) {
      setNote(e instanceof ApiError ? e.message : 'Не сохранилось');
    } finally {
      setBusy(false);
    }
  };

  const same = (a: Weights | null, b: Weights | null) =>
    !!a && !!b && CHECK_ORDER.every((k) => a[k] === b[k]);
  const beforeAvg = preview?.before.avg ?? null;
  const afterAvg = preview?.after.avg ?? null;
  const diff = beforeAvg != null && afterAvg != null ? Math.round(afterAvg) - Math.round(beforeAvg) : 0;

  return (
    <div className={s.page}>
      <header className={s.head}>
        <div>
          <h1 className={s.title}>Веса оценки</h1>
          <p className={s.sub}>
            Двигайте ползунки — баллы всех уже сданных попыток пересчитываются сразу,
            до сохранения. Веса общие для всех занятий.
          </p>
        </div>
        <div className={s.tools}>
          <UserMenu compact />
          <button className={s.btn} onClick={() => nav('/journal')}>← В журнал</button>
        </div>
      </header>

      {!w && <p className={s.empty}>{note ?? 'Загрузка…'}</p>}

      {w && (
        <div className={s.weightsGrid}>
          <section className={s.card} aria-label="Группы проверок">
            <div className={s.head} style={{ marginBottom: 8 }}>
              <h2 className={s.cardTitle}>
                Семь групп проверок <span className={s.muted}>0 — не учитывать, 5 — важнее всего</span>
              </h2>
              <div className={s.tools}>
                <button className={s.btn} disabled={same(w, defaults)}
                        onClick={() => defaults && setW(defaults)}>По умолчанию</button>
                <button className={s.btn} disabled={same(w, saved)}
                        onClick={() => saved && setW(saved)}>Как сохранено</button>
              </div>
            </div>
            {CHECK_ORDER.map((k) => (
              <div key={k} className={s.weightRow}>
                <div>
                  <div className={s.weightTitle}>{CHECK_GROUP[k].title}</div>
                  <div className={s.muted}>{CHECK_GROUP[k].hint}</div>
                </div>
                <input type="range" min={0} max={5} step={1} value={w[k]}
                       aria-label={`Вес: ${CHECK_GROUP[k].title}`} data-testid={`weight-${k}`}
                       onChange={(e) => setW({ ...w, [k]: Number(e.target.value) })} />
                <div className={s.weightValue}>{w[k]}</div>
              </div>
            ))}
            <p className={s.muted}>
              Балл попытки — доля пройденных проверок, взвешенная по группам. «Не применимо»
              не считается. Критичная ошибка — похожая, но другая улица — ограничивает балл
              {' '}{cap} из 100: бригада уедет по реальному, но чужому адресу.
            </p>
          </section>

          <aside className={s.card} aria-label="Было и стало">
            <div className={s.muted}>Средний балл попыток</div>
            <div data-testid="weights-after">
              <span className={s.muted} style={{ fontSize: 22 }}>{fmt(beforeAvg)} → </span>
              <span className={s.big}>{fmt(afterAvg)}</span>
              <span className={clsx(s.delta, diff > 0 && s.up, diff < 0 && s.down)}>
                {diff > 0 ? `+${diff}` : diff}
              </span>
            </div>
            <div className={s.kv}>
              <span>Изменят балл — из всех попыток</span>
              <strong data-testid="weights-changed">{preview ? `${preview.changed} из ${preview.total}` : '—'}</strong>
              <span>С баллом {preview?.threshold ?? 70} и больше</span>
              <strong>{preview ? `${preview.before.passing} → ${preview.after.passing} из ${preview.total}` : '—'}</strong>
            </div>
            <p className={s.muted}>
              Сохранено: {same(saved, defaults) ? 'по умолчанию' : 'свои веса'}.
              Пересчёт — по сохранённым разборам; эталон не меняется. Изменение
              оценок записывается в журнал действий.
            </p>
            <button className={s.primary} style={{ width: '100%', padding: '10px' }}
                    disabled={busy || same(w, saved)} data-testid="weights-save"
                    onClick={() => void save()}>
              Сохранить и пересчитать
            </button>
            {note && <p className={s.muted} role="status">{note}</p>}
          </aside>

          <section className={s.card} style={{ gridColumn: '1 / -1' }} aria-label="По ученикам">
            <h2 className={s.cardTitle}>Средний балл по ученикам: было → стало</h2>
            <p className={s.muted}>Шкала 0–100. Серая полоса — сохранённые веса, синяя — новые.</p>
            {!preview?.students.length && <p className={s.empty}>Сданных попыток пока нет.</p>}
            <div className={s.bars}>
              {preview?.students.map((st) => (
                <div key={st.userId} className={s.barRow}>
                  <span>
                    {st.fio} <span className={s.muted}>
                      · {st.duty === 'dds' ? 'ДДС' : '112'} · {st.attempts}
                    </span>
                  </span>
                  <div className={s.barPair}>
                    <div className={s.barOld} style={{ width: `${st.before ?? 0}%` }} />
                    <div className={s.barNew} style={{ width: `${st.after ?? 0}%` }} />
                  </div>
                  <span className={s.barNum}>{fmt(st.before)} → <strong>{fmt(st.after)}</strong></span>
                </div>
              ))}
            </div>
          </section>
        </div>
      )}
    </div>
  );
};
