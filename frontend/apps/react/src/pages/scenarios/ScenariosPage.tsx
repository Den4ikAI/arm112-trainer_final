import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ApiError, reports, scenarios,
  type ScenarioBrief, type ScenarioFull,
} from '@/shared/api';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { ScenarioChat } from '@/widgets/scenario-chat/ScenarioChat';
import { FactStatements } from '@/widgets/fact-statements/FactStatements';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import clsx from '@/shared/lib/clsx';
import s from './ScenariosPage.module.scss';

/**
 * Подготовка учебных вызовов.
 *
 * Порядок из ТЗ, раздел «Настройка учебной среды»: система формирует
 * вызовы из билетов, преподаватель их просматривает, правит
 * комментарием и утверждает.
 *
 * Слева список, справа сам вызов и окно правки. Эталон показан
 * отдельным блоком и помечен как посчитанный классификатором —
 * преподаватель должен понимать, что эту часть модель не выдумывала.
 */
export const ScenariosPage = () => {
  const [list, setList] = useState<ScenarioBrief[]>([]);
  const [current, setCurrent] = useState<ScenarioFull | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const user = useSession((st) => st.user);
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const nav = useNavigate();

  const reload = useCallback(async (keep?: string) => {
    const items = await scenarios.list();
    setList(items);
    const pick = keep ?? items[0]?.id;
    if (pick) setCurrent(await scenarios.one(pick));
  }, []);

  useEffect(() => { void reload().catch(() => setError('Список недоступен')); }, [reload]);

  if (!user || user.role === 'student') {
    return (
      <div className={s.page}>
        <p className={s.empty}>Раздел доступен преподавателю и администратору.</p>
        <button onClick={() => nav('/journal')}>В журнал</button>
      </div>
    );
  }

  const open = async (id: string) => {
    try {
      setCurrent(await scenarios.one(id));
      setError(null);
    } catch {
      setError('Вызов не открылся');
    }
  };

  /** Подготовить несколько вызовов из билетов. Идёт в очередь. */
  const generate = async () => {
    setWorking(true);
    setError(null);
    setNote(null);
    try {
      const { jobId } = await scenarios.generate([], 3);
      const out = await reports.awaitJob(jobId, 600_000) as {
        created?: unknown[]; failed?: unknown[]; ужеЗаведены?: number;
      };
      await reload();

      // Об итоге говорим всегда. Молчаливое завершение при нуле
      // созданных неотличимо от зависшей кнопки
      const создано = out.created?.length ?? 0;
      const неудач = out.failed?.length ?? 0;
      const заведены = out.ужеЗаведены ?? 0;
      const части = [`подготовлено: ${создано}`];
      if (неудач) части.push(`не получилось: ${неудач}`);
      if (заведены) части.push(`уже были заведены: ${заведены}`);
      setNote(части.join(', '));
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Подготовка не удалась');
    } finally {
      setWorking(false);
    }
  };

  const approve = async () => {
    if (!current) return;
    try {
      setCurrent(await scenarios.approve(current.id));
      await reload(current.id);
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не удалось утвердить');
    }
  };

  return (
    <div className={s.page}>
      <header className={s.head}>
        <h1 className={s.title}>Учебные вызовы</h1>
        <div className={s.tools}>
          <UserMenu compact />
          <button className={s.primary} onClick={() => void generate()}
                  disabled={working} data-testid="generate">
            {working ? 'готовлю…' : 'подготовить из билетов'}
          </button>
          <button className={s.plain} onClick={() => nav('/journal')}>← В журнал</button>
        </div>
      </header>

      {error && <div className={s.error} role="alert">{error}</div>}
      {note && <div className={s.note} data-testid="generate-note">{note}</div>}

      <div className={s.body}>
        <aside className={s.list} aria-label="Список вызовов">
          {list.map((item) => (
            <button
              key={item.id}
              className={clsx(s.item, current?.id === item.id && s.itemOn)}
              data-testid={`scenario-${item.ticket}-${item.callNo}`}
              onClick={() => void open(item.id)}
            >
              <span className={s.itemHead}>
                {scenarioTitle(item)}
                {item.status === 'draft' && <span className={s.itemDraft}> · черновик</span>}
              </span>
              <span className={s.itemText}>{item.legend.slice(0, 90)}</span>
            </button>
          ))}
          {list.length === 0 && (
            <p className={s.empty}>Вызовов нет. Подготовьте их из билетов.</p>
          )}
        </aside>

        {current && (
          <div className={s.detail}>
            <section className={s.card}>
              <div className={s.cardHead}>
                <span className={clsx(s.badge, current.status === 'approved' && s.badgeOk)}>
                  {current.status === 'approved' ? 'утверждён' : 'черновик'}
                </span>
                <span className={s.source}>
                  {current.source === 'generated' ? 'подготовлен моделью' : 'заведён вручную'}
                </span>
                {current.status !== 'approved' && (
                  <button className={s.primary} onClick={() => void approve()}
                          data-testid="approve">утвердить</button>
                )}
              </div>

              <h3 className={s.blockTitle}>Первая фраза заявителя</h3>
              <p className={s.legend} data-testid="legend">{current.legend}</p>

              <h3 className={s.blockTitle}>
                Что заявитель скажет только в ответ на вопрос
              </h3>
              <ol className={s.facts} data-testid="facts">
                {current.hidden.map((h) => (
                  <li key={h.key}>
                    <span className={s.q}>{h.question}</span>
                    <span className={s.a}>{h.answer}</span>
                  </li>
                ))}
                {current.hidden.length === 0 && (
                  <li className={s.empty}>Скрытых фактов нет — курсанту нечего выяснять.</li>
                )}
              </ol>

              <h3 className={s.blockTitle}>Утверждения для сверки описания</h3>
              <FactStatements scenario={current} onChanged={setCurrent} />

              {/* Эталон отделён намеренно: эту часть считает
                  классификатор, а не модель */}
              <h3 className={s.blockTitle}>Эталон — посчитан классификатором</h3>
              {current.classifyReason ? (
                <p className={s.warn} data-testid="classify-reason">
                  {current.classifyReason}
                </p>
              ) : (
                <dl className={s.expected}>
                  <div>
                    <dt>Класс происшествия</dt>
                    <dd data-testid="outcome">{current.expected.outcome ?? '—'}</dd>
                  </div>
                  <div>
                    <dt>Службы</dt>
                    <dd>
                      {(current.expected.services ?? []).map(serviceTitle).join(', ') || '—'}
                    </dd>
                  </div>
                </dl>
              )}
            </section>

            <ScenarioChat scenario={current} onChanged={setCurrent} />
          </div>
        )}
      </div>
    </div>
  );
};
