import { useCallback, useEffect, useState } from 'react';
import {
  review, type Attempt, type CardReview, type ReviewedFinding, type TextFindingKind,
} from '@/shared/api';
import { useCatalog } from '@/entities/catalog/model';
import { ChiefReviewList } from '@/entities/chief/ChiefReviewList';
import s from './AttemptReview.module.scss';

const KIND_LABEL: Record<TextFindingKind, string> = {
  missing: 'не записано',
  contradiction: 'противоречие',
  invented: 'не сообщалось',
  no_reason: 'нет причины',
  inconsistent: 'расходится с докладом',
};

const STATUS_LABEL = {
  queued: 'готовится — модель читает текст, на процессоре это до минуты',
  ready: 'готов',
  unavailable: 'модель недоступна',
  failed: 'модель не дала разбора',
} as const;

/** Пока анализ в очереди, перечитываем разбор: модели нужно время. */
const POLL_MS = 5_000;

/**
 * Разбор попытки: автоматический балл правил, нейросетевой анализ
 * ручного ввода и экспертная оценка преподавателя.
 *
 * ТЗ требует «автоматическую оценку И экспертную оценку» — поэтому
 * экспертный балл стоит рядом с автоматическим, а не вместо него.
 * Замечания модели — подсказка преподавателю, а не оценка: балл они
 * не меняют, преподаватель их подтверждает или отклоняет.
 */
export const AttemptReview = ({ cardId, only }: {
  cardId: string;
  /** Показать только эту попытку — например, вызов 112 в разборе курсанта */
  only?: Attempt['kind'];
}) => {
  const [data, setData] = useState<CardReview | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await review.get(cardId));
    } catch {
      setData({ attempts: [], canGrade: false });
    }
  }, [cardId]);

  useEffect(() => { void load(); }, [load]);

  const waiting = !!data?.attempts.some(
    (a) => a.analysis?.status === 'queued' || !!a.chief?.review?.pending);
  useEffect(() => {
    if (!waiting) return;
    const t = setInterval(() => void load(), POLL_MS);
    return () => clearInterval(t);
  }, [waiting, load]);

  const attempts = (data?.attempts ?? []).filter((a) => !only || a.kind === only);
  if (!data || attempts.length === 0) return null;

  return (
    <section className={s.wrap} aria-label="Разбор попытки" data-testid="attempt-review">
      {attempts.map((a) => (
        <AttemptBlock key={`${a.kind}-${a.serviceId ?? ''}`} cardId={cardId} attempt={a}
                      canGrade={data.canGrade} onChange={load} />
      ))}
    </section>
  );
};

const AttemptBlock = ({ cardId, attempt, canGrade, onChange }: {
  cardId: string; attempt: Attempt; canGrade: boolean; onChange: () => Promise<void>;
}) => {
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const target = { kind: attempt.kind, serviceId: attempt.serviceId };
  const title = attempt.kind === 'card112'
    ? 'Вызов 112'
    : `ДДС: ${serviceTitle(attempt.serviceId ?? '')}`;
  const a = attempt.analysis;

  return (
    <div className={s.attempt} data-testid={`attempt-${attempt.kind}`}>
      <h2 className={s.title}>
        {title}{attempt.student && <span className={s.muted}> — {attempt.student.fio}</span>}
      </h2>

      <div className={s.scores}>
        <div className={s.score}>
          <div className={s.scoreValue} data-testid="auto-score">{attempt.autoScore ?? '—'}</div>
          <div className={s.muted}>автоматическая оценка</div>
        </div>
        <div className={s.score}>
          <div className={s.scoreValue} data-testid="expert-score">{attempt.expert?.score ?? '—'}</div>
          <div className={s.muted}>
            экспертная оценка{attempt.expert && ` · ${attempt.expert.teacher}`}
          </div>
        </div>
      </div>
      {attempt.expert?.comment && (
        <p className={s.comment} data-testid="expert-comment">{attempt.expert.comment}</p>
      )}

      {canGrade && (
        <ExpertForm key={attempt.expert?.updatedAt ?? 'new'} initial={attempt.expert}
                    onSave={async (score, comment) => {
                      await review.setExpert(cardId, target, score, comment);
                      await onChange();
                    }} />
      )}

      {attempt.kind === 'dds' && attempt.chief && (
        <>
          <h3 className={s.subtitle}>Доклад начальнику</h3>
          {attempt.chief.review
            ? <ChiefReviewList review={attempt.chief.review} />
            : <p className={s.muted}>Звонок ещё идёт: разбор появится после «Принял».</p>}
          <details className={s.muted}>
            <summary>Разговор ({attempt.chief.lines.length} реплик)</summary>
            <ol className={s.list} data-testid="chief-transcript">
              {attempt.chief.lines.map((l, i) => (
                <li key={i}>{l.who === 'chief' ? 'Начальник' : 'Диспетчер'}: {l.text}</li>
              ))}
            </ol>
          </details>
        </>
      )}

      <h3 className={s.subtitle}>Анализ текста (нейросеть)</h3>
      {!a ? (
        <p className={s.muted}>Анализа нет: текст ещё не отправлен на разбор.</p>
      ) : (
        <>
          <p className={s.muted} data-testid="analysis-status">
            {STATUS_LABEL[a.status]}
            {a.status === 'ready' && a.modelMs != null && ` · ${Math.round(a.modelMs / 1000)} с`}
            {a.note && ` · ${a.note}`}
          </p>
          {a.status === 'ready' && a.findings.length === 0 && (
            <p className={s.muted}>Расхождений с услышанным модель не нашла.</p>
          )}
          {a.findings.length > 0 && (
            <ul className={s.list}>
              {a.findings.map((f, i) => (
                <FindingRow key={i} f={f} canGrade={canGrade}
                            onVerdict={async (v) => {
                              await review.setVerdict(cardId, a.id, i, v);
                              await onChange();
                            }} />
              ))}
            </ul>
          )}
          {a.dropped > 0 && (
            <p className={s.muted}>
              Отброшено замечаний модели: {a.dropped} — их цитаты не нашлись ни в тексте, ни в разговоре.
            </p>
          )}
          <p className={s.note}>
            Замечания модели балл не меняют. Модель ошибается, поэтому каждое
            замечание подтверждено цитатой и решение по нему за преподавателем.
          </p>
          {canGrade && (a.status === 'unavailable' || a.status === 'failed') && (
            <button className={s.button} onClick={async () => {
              await review.analyse(cardId, target);
              await onChange();
            }}>Повторить анализ</button>
          )}
        </>
      )}
    </div>
  );
};

const FindingRow = ({ f, canGrade, onVerdict }: {
  f: ReviewedFinding; canGrade: boolean;
  onVerdict: (v: 'confirmed' | 'rejected' | null) => Promise<void>;
}) => (
  <li className={s.item} data-testid="analysis-finding">
    <span className={s.badge}>{KIND_LABEL[f.kind] ?? f.kind}</span>
    <div>
      <div>{f.text}</div>
      {f.quoteInput && <div className={s.quote}>в тексте: «{f.quoteInput}»</div>}
      {f.quoteSource && <div className={s.quote}>услышано: «{f.quoteSource}»</div>}
      <div className={s.verdictRow}>
        {f.verdict === 'confirmed' && <span className={s.ok}>подтверждено преподавателем</span>}
        {f.verdict === 'rejected' && <span className={s.muted}>отклонено преподавателем</span>}
        {canGrade && (
          <>
            <button className={s.linkBtn} disabled={f.verdict === 'confirmed'}
                    onClick={() => void onVerdict('confirmed')}>подтвердить</button>
            <button className={s.linkBtn} disabled={f.verdict === 'rejected'}
                    onClick={() => void onVerdict('rejected')}>отклонить</button>
          </>
        )}
      </div>
    </div>
  </li>
);

const ExpertForm = ({ initial, onSave }: {
  initial: Attempt['expert'];
  onSave: (score: number, comment: string) => Promise<void>;
}) => {
  const [score, setScore] = useState(initial ? String(initial.score) : '');
  const [comment, setComment] = useState(initial?.comment ?? '');
  const [error, setError] = useState<string | null>(null);
  const n = Number(score);
  const valid = score.trim() !== '' && Number.isInteger(n) && n >= 0 && n <= 100;

  return (
    <form className={s.form} data-testid="expert-form" onSubmit={async (e) => {
      e.preventDefault();
      if (!valid) return;
      try {
        setError(null);
        await onSave(n, comment);
      } catch {
        setError('Не удалось сохранить оценку');
      }
    }}>
      <label className={s.field}>
        <span className={s.muted}>Экспертный балл, 0–100</span>
        <input type="number" min={0} max={100} value={score} name="expert-score"
               onChange={(e) => setScore(e.target.value)} className={s.input} />
      </label>
      <label className={s.field}>
        <span className={s.muted}>Комментарий курсанту</span>
        <textarea value={comment} name="expert-comment" rows={2}
                  onChange={(e) => setComment(e.target.value)} className={s.input} />
      </label>
      <button type="submit" className={s.button} disabled={!valid}>
        {initial ? 'Изменить оценку' : 'Поставить оценку'}
      </button>
      {error && <span className={s.bad}>{error}</span>}
    </form>
  );
};
