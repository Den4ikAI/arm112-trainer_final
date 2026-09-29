import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  ApiError, ddsIncidents, reports, scenarios,
  type Incident, type LibraryItem, type ScenarioFull,
} from '@/shared/api';
import { useSession } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { ScenarioChat } from '@/widgets/scenario-chat/ScenarioChat';
import { FactStatements } from '@/widgets/fact-statements/FactStatements';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { PromptBuilder } from '@/widgets/material-builder/PromptBuilder';
import { ScenarioBuilder } from '@/widgets/material-builder/ScenarioBuilder';
import { IncidentBuilder } from '@/widgets/material-builder/IncidentBuilder';
import { IncidentView } from '@/widgets/material-builder/IncidentView';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import clsx from '@/shared/lib/clsx';
import s from './ScenariosPage.module.scss';

type Tab = '112' | 'dds';
/** Что открыто справа: материал или один из конструкторов */
type Pane = 'view' | 'prompt' | 'manual' | 'edit';

const SOURCE: Record<string, string> = {
  generated: 'подготовлен моделью из билета',
  prompt: 'подготовлен моделью по описанию',
  manual: 'собран вручную',
};

const дата = (iso: string) => new Date(iso).toLocaleDateString('ru-RU',
  { day: '2-digit', month: '2-digit', year: 'numeric' });

/**
 * Учебный материал преподавателя.
 *
 * Две вкладки — по рабочим местам: вызовы для оператора 112 и
 * происшествия для диспетчера ДДС. И то и другое преподаватель создаёт
 * сам — по описанию для модели или вручную — правит и утверждает.
 * Утверждённое доступно при составлении занятия (п. 6 замечаний команды).
 *
 * Вызовы из билетов готовятся как раньше, из того же раздела. Эталон
 * везде отделён и помечен как посчитанный классификатором: эту часть
 * модель не выдумывала.
 */
export const ScenariosPage = () => {
  const [tab, setTab] = useState<Tab>('112');
  const [pane, setPane] = useState<Pane>('view');
  const [list, setList] = useState<LibraryItem[]>([]);
  const [current, setCurrent] = useState<ScenarioFull | null>(null);
  const [incidents, setIncidents] = useState<Incident[]>([]);
  const [incident, setIncident] = useState<Incident | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  // Модель правит вызов: утверждать нельзя, иначе правка ляжет на уже
  // утверждённый вызов (UX-проход 1, находка 11)
  const [chatBusy, setChatBusy] = useState(false);
  const user = useSession((st) => st.user);
  const serviceTitle = useCatalog((st) => st.serviceTitle);
  const loadCatalog = useCatalog((st) => st.load);
  const nav = useNavigate();

  const reload = useCallback(async (keep?: string) => {
    const items = await scenarios.library();
    setList(items);
    const pick = keep ?? items[0]?.id;
    if (pick) setCurrent(await scenarios.one(pick));
  }, []);

  const reloadIncidents = useCallback(async (keep?: string | null) => {
    const items = await ddsIncidents.list();
    setIncidents(items);
    const pick = keep === null ? items[0] : items.find((i) => i.id === keep) ?? items[0];
    setIncident(pick ?? null);
  }, []);

  useEffect(() => {
    void loadCatalog().catch(() => {});
    void reload().catch(() => setError('Список недоступен'));
    void reloadIncidents(null).catch(() => {});
  }, [reload, reloadIncidents, loadCatalog]);

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
      setPane('view');
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
    if (!current || chatBusy) return;
    try {
      setCurrent(await scenarios.approve(current.id));
      await reload(current.id);
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не удалось утвердить');
    }
  };

  const switchTab = (t: Tab) => {
    setTab(t);
    setPane('view');
    setError(null);
    setNote(null);
  };

  return (
    <div className={s.page}>
      <header className={s.head}>
        <h1 className={s.title}>Учебный материал</h1>
        <div className={s.tools}>
          <UserMenu />
        </div>
      </header>

      <div className={s.tabs} role="tablist" aria-label="Вид материала">
        <button role="tab" aria-selected={tab === '112'} data-testid="tab-112"
                className={clsx(s.tab, tab === '112' && s.tabOn)} onClick={() => switchTab('112')}>
          Вызовы 112 <span className={s.count}>{list.length}</span>
        </button>
        <button role="tab" aria-selected={tab === 'dds'} data-testid="tab-dds"
                className={clsx(s.tab, tab === 'dds' && s.tabOn)} onClick={() => switchTab('dds')}>
          Происшествия ДДС <span className={s.count}>{incidents.length}</span>
        </button>
        <span className={s.tabTools}>
          <button className={s.primary} data-testid="create-prompt"
                  onClick={() => setPane('prompt')}>Создать по описанию (ИИ)</button>
          <button className={s.primary} data-testid="create-manual"
                  onClick={() => setPane('manual')}>Собрать вручную</button>
          {tab === '112' && (
            <button className={s.plain} onClick={() => void generate()}
                    disabled={working} data-testid="generate">
              {working ? 'готовлю…' : 'подготовить из билетов'}
            </button>
          )}
        </span>
      </div>

      {error && <div className={s.error} role="alert">{error}</div>}
      {note && <div className={s.note} data-testid="generate-note">{note}</div>}

      {tab === '112' ? (
        <div className={s.body}>
          <aside className={s.list} aria-label="Список вызовов">
            {list.map((item) => (
              <button
                key={item.id}
                className={clsx(s.item, pane === 'view' && current?.id === item.id && s.itemOn)}
                data-testid={`scenario-${item.ticket}-${item.callNo}`}
                data-scenario={item.id}
                onClick={() => void open(item.id)}
              >
                <span className={s.itemHead}>
                  {scenarioTitle(item)}
                  <span className={item.status === 'approved' ? s.itemOk : s.itemDraft}>
                    {' · '}{item.status === 'approved' ? 'утверждён' : 'черновик'}
                  </span>
                </span>
                <span className={s.itemMeta}>
                  {дата(item.createdAt)}{item.outcome ? ` · ${item.outcome}` : ''}
                </span>
                <span className={s.itemText}>{item.legend.slice(0, 90)}</span>
              </button>
            ))}
            {list.length === 0 && (
              <p className={s.empty}>Вызовов нет. Создайте по описанию, вручную или из билетов.</p>
            )}
          </aside>

          {pane === 'prompt' && (
            <div className={s.detailWide}>
              <PromptBuilder
                what="вызов 112"
                placeholder="Пожилая женщина сообщает, что у соседа за стеной сильно пахнет газом и никто не открывает. Москва, ул. Академика Королёва, д. 12, кв. 45."
                onCancel={() => setPane('view')}
                onSubmit={async (title, prompt) => {
                  const created = await scenarios.fromPrompt(title, prompt);
                  await reload(created.id);
                  setPane('view');
                }}
              />
            </div>
          )}

          {pane === 'manual' && (
            <div className={s.detailWide}>
              <ScenarioBuilder
                onCancel={() => setPane('view')}
                onCreated={async (created) => { await reload(created.id); setPane('view'); }}
              />
            </div>
          )}

          {pane === 'view' && current && (
            <div className={s.detail}>
              <section className={s.card} data-testid="scenario-view">
                <div className={s.cardHead}>
                  <span className={clsx(s.badge, current.status === 'approved' && s.badgeOk)}>
                    {current.status === 'approved' ? 'утверждён' : 'черновик'}
                  </span>
                  <strong className={s.cardTitle} data-testid="scenario-title">
                    {scenarioTitle(current)}
                  </strong>
                  <span className={s.source}>{SOURCE[current.source] ?? 'заведён вручную'}</span>
                  {current.status !== 'approved' && (
                    <button className={s.primary} onClick={() => void approve()}
                            disabled={chatBusy}
                            title={chatBusy ? 'Модель правит вызов — дождитесь ответа' : undefined}
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

              <ScenarioChat scenario={current} onChanged={setCurrent} onBusy={setChatBusy} />
            </div>
          )}
        </div>
      ) : (
        <div className={s.body}>
          <aside className={s.list} aria-label="Список происшествий ДДС">
            {incidents.map((inc) => (
              <button
                key={inc.id}
                className={clsx(s.item, pane === 'view' && incident?.id === inc.id && s.itemOn)}
                data-testid={`incident-${inc.number}`}
                onClick={() => { setIncident(inc); setPane('view'); }}
              >
                <span className={s.itemHead}>
                  {inc.title ?? `Происшествие № ${inc.number}`}
                  <span className={inc.status === 'approved' ? s.itemOk : s.itemDraft}>
                    {' · '}{inc.status === 'approved' ? 'утверждено' : 'черновик'}
                  </span>
                </span>
                <span className={s.itemMeta}>
                  № {inc.number} · {дата(inc.createdAt)}{inc.outcomeType ? ` · ${inc.outcomeType}` : ''}
                </span>
                <span className={s.itemText}>{inc.description.slice(0, 90)}</span>
              </button>
            ))}
            {incidents.length === 0 && (
              <p className={s.empty}>
                Происшествий нет. Карточки операторов 112 к диспетчерам не попадают —
                учебные происшествия ДДС собирает преподаватель.
              </p>
            )}
          </aside>

          <div className={s.detailWide}>
            {pane === 'prompt' && (
              <PromptBuilder
                what="происшествие ДДС"
                placeholder="Задымление мусоропровода в жилом доме: дым в подъезде, открытого огня нет, пострадавших нет. Москва, ул. Берзарина, д. 21, корп. 1, подъезд 3."
                onCancel={() => setPane('view')}
                onSubmit={async (title, prompt) => {
                  const created = await ddsIncidents.fromPrompt(title, prompt);
                  await reloadIncidents(created.id);
                  setPane('view');
                }}
              />
            )}
            {(pane === 'manual' || pane === 'edit') && (
              <IncidentBuilder
                initial={pane === 'edit' ? incident : null}
                onCancel={() => setPane('view')}
                onSaved={async (saved) => { await reloadIncidents(saved.id); setPane('view'); }}
              />
            )}
            {pane === 'view' && incident && (
              <IncidentView
                incident={incident}
                onEdit={() => setPane('edit')}
                onChanged={async (changed) => { await reloadIncidents(changed ? changed.id : null); }}
              />
            )}
          </div>
        </div>
      )}
    </div>
  );
};
