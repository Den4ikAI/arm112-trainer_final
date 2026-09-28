import { useEffect, useState } from 'react';
import { ApiError, scenarios, type ScenarioFull } from '@/shared/api';
import s from './FactStatements.module.scss';

/**
 * Утверждения фактов вызова — по ним описание курсанта сверяется
 * кросс-энкодером на процессоре.
 *
 * Кросс-энкодер плохо читает ответ заявителя как есть: «Нет, дом не
 * газифицирован.» он понимает как отрицание, а легенда несёт несколько
 * сведений в одной фразе. Поэтому модель при подготовке вызова
 * переписывает их утверждениями, а преподаватель проверяет и правит
 * здесь — вместе с вызовом, до утверждения.
 */
export const FactStatements = ({
  scenario, onChanged,
}: { scenario: ScenarioFull; onChanged: (s: ScenarioFull) => void }) => {
  const [legend, setLegend] = useState('');
  const [facts, setFacts] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  // Новый вызов или свежие утверждения с сервера — сбрасываем правку
  useEffect(() => {
    setLegend((scenario.legendStatements ?? []).join('\n'));
    setFacts(Object.fromEntries(scenario.hidden.map((h) => [h.key, h.statement ?? ''])));
  }, [scenario]);

  // Сообщение — только при переходе к другому вызову. Сохранение тоже
  // присылает обновлённый вызов, и сброс по нему стирал «Сохранено»
  // в тот же миг, как оно появлялось
  useEffect(() => {
    setNote(null);
  }, [scenario.id]);

  const pending = scenario.legendStatements == null
    || scenario.hidden.some((h) => !h.statement);

  const save = async () => {
    setBusy(true);
    try {
      const next = await scenarios.statements(scenario.id, {
        legendStatements: legend.split('\n').map((l) => l.trim()).filter(Boolean),
        facts,
      });
      onChanged(next);
      setNote('Сохранено');
    } catch (e) {
      setNote(e instanceof ApiError ? e.message : 'Не сохранилось');
    } finally {
      setBusy(false);
    }
  };

  const prepare = async () => {
    setBusy(true);
    try {
      await scenarios.prepareStatements(scenario.id);
      setNote('Поставлено в очередь модели — обновите вызов через полминуты');
    } catch (e) {
      setNote(e instanceof ApiError ? e.message : 'Не получилось');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={s.box} data-testid="fact-statements">
      <p className={s.hint}>
        По этим утверждениям описание курсанта сверяется с разговором: что
        записано, что упущено, что противоречит сказанному. Одно утверждение —
        одно сведение, без «да» и «нет» в начале.
        {pending && <b className={s.pending}> Часть ещё не подготовлена моделью.</b>}
      </p>

      <label className={s.label} htmlFor="legend-statements">Первая фраза — по сведению на строку</label>
      <textarea
        id="legend-statements"
        className={s.input}
        rows={Math.max(2, legend.split('\n').length)}
        value={legend}
        placeholder="готовится…"
        onChange={(e) => setLegend(e.target.value)}
        data-testid="legend-statements"
      />

      {scenario.hidden.map((h) => (
        <div key={h.key} className={s.row}>
          <label className={s.label} htmlFor={`st-${h.key}`}>{h.answer}</label>
          <input
            id={`st-${h.key}`}
            className={s.input}
            value={facts[h.key] ?? ''}
            placeholder="готовится…"
            onChange={(e) => setFacts({ ...facts, [h.key]: e.target.value })}
            data-testid={`statement-${h.key}`}
          />
        </div>
      ))}

      <div className={s.actions}>
        <button onClick={() => void save()} disabled={busy} data-testid="save-statements">
          сохранить утверждения
        </button>
        <button onClick={() => void prepare()} disabled={busy}>подготовить заново моделью</button>
        {note && <span className={s.note} role="status">{note}</span>}
      </div>
    </div>
  );
};
