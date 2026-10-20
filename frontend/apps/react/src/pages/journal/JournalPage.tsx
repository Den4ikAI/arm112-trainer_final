import { useEffect, useState } from 'react';
import { JournalTable, type JournalRow } from '@/widgets/journal-table/JournalTable';
import { useTraining } from '@/entities/incident/model';
import { useSession } from '@/entities/session/model';
import { useStand } from '@/entities/stand/model';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { StudentWork } from '@/widgets/student-work/StudentWork';
import { UiHints } from '@/widgets/ui-hints/UiHints';
import { ApiError, cards, channel, lessons, scenarios as scenariosApi, sessions, SEGMENT_LABEL,
         type CardBrief, type LessonBrief, type Protocol, type ScenarioBrief,
         type SessionState } from '@/shared/api';
import { fmt } from '@/shared/lib/useTimer';
import { useNavigate } from 'react-router-dom';
import { useClock } from '@/shared/lib/useClock';
import { scenarioTitle } from '@/shared/lib/scenarioTitle';
import s from './JournalPage.module.scss';
import { stubProps } from '@/shared/ui/Stub';

/**
 * Строка журнала из карточки, пришедшей с сервера.
 * Адрес собираем из заполненных полей — пустые не показываем,
 * иначе получается «Москва , , , ».
 */
const toRow = (c: CardBrief): JournalRow => {
  const saved = new Date(c.savedAt ?? c.createdAt);
  const address = ['region', 'settlement', 'district', 'street', 'house', 'building']
    .map((f) => c.address[f])
    .filter(Boolean)
    .join(', ');

  return {
    id: c.id,
    number: c.number,
    date: saved.toLocaleDateString('ru-RU', { day: '2-digit', month: '2-digit', year: '2-digit' }),
    time: saved.toLocaleTimeString('ru-RU'),
    type: c.outcomeType ?? '—',
    // Сохранённое в карточке, а не заглушка: на стенде журнал писал
    // «Нет» при отмеченных пострадавших
    victims: c.victims == null ? 'Нет' : String(c.victims),
    address,
    serviceStatus: c.status,
    waitingSec: c.waitingSec,
    opened: c.opened ?? null,
    recorded: c.recorded ?? null,
    operator: c.operator,
    workstation: c.workstation ?? '',
    description: c.description,
    to: `/card-dds/${c.id}`,
  };
};

/**
 * Записей на странице — как на кадре журнала боевой системы.
 * Раньше подпись говорила «10», а приходило 50 без перехода дальше,
 * и остальная история была недоступна.
 */
const PAGE_SIZE = 10;

/**
 * Плашка марафона или блица: что за упражнение, сколько пройдено,
 * а когда марафон закончен — его итоги.
 */
const DrillBanner = ({ lesson, row }: { lesson: LessonBrief; row: Protocol['rows'][number] | null }) => {
  const segment = lesson.segment ? SEGMENT_LABEL[lesson.segment] : null;
  const пройдено = row ? `${row.done} из ${row.total}` : null;

  if (lesson.kind === 'blitz') {
    return (
      <div className={s.lesson} role="status" data-testid="lesson-banner">
        Идёт блиц «{lesson.title}»: оценивается только «{segment}».
        Разбор — сразу после каждого задания.
        {пройдено && ` Пройдено ${пройдено}.`}
      </div>
    );
  }

  // Марафон
  if (row && row.total > 0 && row.done === row.total && !row.hidden) {
    return (
      <div className={s.lesson} role="status" data-testid="marathon-result">
        <div>
          Марафон «{lesson.title}» пройден: средний балл{' '}
          <b>{row.avgScore == null ? '—' : Math.round(row.avgScore)}</b>,
          время {fmt(row.totalSec)}, грубых ошибок {row.grossCount}.
        </div>
        <ol className={s.drillList}>
          {row.calls.map((c) => (
            <li key={c.seq}>
              Вызов {c.seq}: <b>{c.score ?? '—'}</b> баллов
              {c.elapsedSec != null && `, ${fmt(c.elapsedSec)}`}
              {c.gross.length > 0 && (
                // Полные названия служб длинные: первые три, остальное числом
                <span title={c.gross.join('\n')}>
                  {' — грубые ошибки: '}
                  {c.gross.slice(0, 3).map((g) => g.replace(/\s*\(.*\)$/, '')).join(', ')}
                  {c.gross.length > 3 && ` и ещё ${c.gross.length - 3}`}
                </span>
              )}
            </li>
          ))}
        </ol>
      </div>
    );
  }
  return (
    <div className={s.lesson} role="status" data-testid="lesson-banner">
      Идёт марафон «{lesson.title}»
      {row && row.total > 0 && `: вызов ${Math.min(row.done + 1, row.total)} из ${row.total}`}.
      Вызовы идут по порядку, у каждого одна попытка. Разбор и рекомендации —
      после последнего вызова.
    </div>
  );
};

/** Главный экран: поиск, список происшествий, часы. Плюс запуск учебного вызова. */
export const JournalPage = () => {
  const [query, setQuery] = useState('');
  const [rows, setRows] = useState<JournalRow[]>([]);
  const [page, setPage] = useState(0);
  const [total, setTotal] = useState(0);
  const [scenarios, setScenarios] = useState<ScenarioBrief[]>([]);
  // Идущее занятие по группе курсанта. Пока оно идёт, и список вызовов,
  // и лента ограничены его материалом — курсант должен понимать почему
  const [lesson, setLesson] = useState<LessonBrief | null>(null);
  // Марафон и блиц: сколько пройдено, а после марафона — его итоги
  const [protocol, setProtocol] = useState<Protocol | null>(null);
  const [error, setError] = useState<string | null>(null);
  const start = useTraining((st) => st.start);
  const resume = useTraining((st) => st.resume);
  // Незаконченный вызов: после обновления страницы или повторного
  // входа его можно продолжить, а не начинать билет заново
  const [unfinished, setUnfinished] = useState<SessionState | null>(null);
  const user = useSession((st) => st.user);
  // Приём вызовов 112 свёрнут (эксперты, 18.10): преподавателю вместо
  // билетов 112 — вход в работу с происшествиями ДДС
  const showOp112 = useStand((st) => st.op112);
  const nav = useNavigate();
  const now = useClock();
  // Рабочее место обучающегося: у оператора 112 — свои вызовы и билеты
  // для приёма, у диспетчера ДДС — лента своей службы. Преподаватель и
  // администратор видят всё
  const dds = user?.role === 'student' && user.duty === 'dds';
  const op112 = user?.role === 'student' && user.duty === 'op112';

  // Лента фильтруется сервером по службе пользователя: «в их ленту
  // попадают только профильные события»
  useEffect(() => {
    let alive = true;
    // Две половины экрана грузятся врозь. Раньше это был один
    // Promise.all с одним catch: у курсанта без службы лента
    // отвечает отказом — и вместе с ней пропадали учебные вызовы,
    // а на экране было «Нет связи с сервером», хотя связь есть
    const load = async () => {
      const scope = dds ? 'service' : 'all';

      // Курсанту — только выданное преподавателем, в разделах (Денис,
      // 29.09). Оператору 112 список происшествий не нужен вовсе, ему —
      // карточки для заполнения; диспетчеру ДДС лента остаётся, но в
      // ней только выданные ему происшествия. Билеты для свободного
      // приёма — только преподавателю и администратору
      const [feed, list] = await Promise.allSettled([
        op112
          ? Promise.resolve({ items: [] as CardBrief[], total: 0, page: 0, size: PAGE_SIZE })
          : cards.list({ scope, q: query || undefined, page, size: PAGE_SIZE }),
        user?.role === 'student' || !showOp112
          ? Promise.resolve([] as ScenarioBrief[]) : scenariosApi.list(),
      ]);
      if (!alive) return;

      if (feed.status === 'fulfilled') {
        setRows(feed.value.items.map(toRow));
        setTotal(feed.value.total);
      }
      if (list.status === 'fulfilled') setScenarios(list.value);

      // Занятие — надстройка: не узнали о нём, и экран работает как обычно
      if (user?.role === 'student') {
        const идущие = await lessons.list().catch(() => [] as LessonBrief[]);
        const незакончен = op112 ? await sessions.active().catch(() => undefined) : undefined;
        if (alive) setUnfinished(незакончен ?? null);
        if (alive) setLesson(идущие[0] ?? null);
        const идёт = идущие[0];
        const итоги = идёт && идёт.kind !== 'training' && идёт.mode === 'card112'
          ? await lessons.protocol(идёт.id).catch(() => null)
          : null;
        if (alive) setProtocol(итоги);
      }

      // Говорим, что именно не открылось, а не «что-то сломалось»
      const беда = (r: PromiseSettledResult<unknown>) =>
        r.status === 'rejected'
          ? (r.reason instanceof ApiError ? (r.reason.detail ?? r.reason.message) : 'нет связи с сервером')
          : null;
      const лента = беда(feed);
      const вызовы = беда(list);

      if (лента && вызовы) setError('Нет связи с сервером');
      else if (лента) setError(`Лента происшествий недоступна: ${лента}`);
      else if (вызовы) setError(`Учебные вызовы недоступны: ${вызовы}`);
      else setError(null);
    };
    void load();

    // Канал класса: карточка от оператора и статусы от других
    // диспетчеров приходят сразу, без обновления страницы
    // Диспетчеру ДДС — лента своей службы; оператору 112 — свой топик:
    // туда приходят начало и сохранение его вызовов, и журнал своих
    // карточек обновляется сам. Преподаватель обновляется опросом
    const topics = dds && user?.serviceId ? [`service:${user.serviceId}`]
      : op112 && user?.id ? [`user:${user?.id}`] : [];
    if (topics.length) channel.subscribe(topics);

    const off = channel.on((e) => {
      if (['card.dispatched', 'response.recorded', 'ack.timeout',
           'card.status', 'session.finished', 'lagged'].includes(e.type)) {
        void load();
      }
    });

    // Запасной опрос: канал мог не подняться, а счётчик ожидания
    // всё равно должен идти. Реже, чем раньше — основную работу
    // делает канал
    const timer = setInterval(() => void load(), 15_000);

    return () => {
      alive = false;
      off();
      clearInterval(timer);
      if (topics.length) channel.unsubscribe(topics);
    };
  }, [query, page, user?.role, user?.serviceId, user?.id, dds, op112, showOp112]);

  const proceed = async (st: SessionState) => {
    if (await resume(st.card.number)) nav(`/card/${st.card.number}`);
  };

  const launch = async (scenarioId: string) => {
    await start(scenarioId);
    const number = useTraining.getState().card?.number;
    if (number) nav(`/card/${number}`);
  };

  return (
    <div className={s.page}>
      <div className={s.top}>
        {op112 ? (
          // Оператору 112 искать нечего: списка происшествий у него нет,
          // только выданные карточки для заполнения
          <div className={s.searchArea}>
            <h1 className={s.opTitle}>Карточки для заполнения</h1>
            <div className={s.searchSub}>выдаёт преподаватель на занятии</div>
          </div>
        ) : (
        <div className={s.searchArea}>
          <div className={s.searchRow}>
            <input className={s.search} value={query} placeholder="Поиск происшествий"
                   aria-label="Поиск происшествий"
                   // Новый поиск — с первой страницы: на пятой его
                   // результатов может не быть
                   onChange={(e) => { setQuery(e.target.value); setPage(0); }} />
            <span className={s.loupe} aria-hidden>⌕</span>
          </div>
          <div className={s.searchSub}>
            <button {...stubProps('Расширенный поиск')} className={s.link}>расширенный по параметрам ⌄</button>
            <button className={s.reset} onClick={() => { setQuery(''); setPage(0); }}>сбросить</button>
          </div>
        </div>
        )}

        {/* data-hint — якорь режима подсказок (widgets/ui-hints) */}
        <div className={s.clock} data-hint="clock">
          <div className={s.clockDate}>
            {now.toLocaleDateString('ru-RU', { weekday: 'long', day: 'numeric',
              month: 'long', year: 'numeric' })}
          </div>
          <div className={s.clockTime}>
            {now.toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' })}
            <span className={s.clockSec}>
              :{String(now.getSeconds()).padStart(2, '0')}
            </span>
          </div>
          {/* Курсанту разделы не нужны, и сервер его туда не пустит */}
          <div className={s.clockUser}>
            <UserMenu />
            {/* Подсказки по окнам и кнопкам — самостоятельное знакомство
                диспетчера с интерфейсом (рекомендация заказчика, п. 7) */}
            {dds && <UiHints page="journal" compact />}
          </div>
        </div>
      </div>

      {user?.role === 'student' && <StudentWork />}

      {user?.role !== 'student' && !showOp112 && (
        <section className={s.launcher} aria-label="Учебные происшествия ДДС" data-testid="dds-start">
          <h2 className={s.launcherTitle}>Учебные происшествия ДДС</h2>
          <div className={s.launcherList}>
            <button className={s.launchBtn} data-testid="dds-start-ai"
                    onClick={() => nav('/scenarios?pane=ai')}>
              <strong>Происшествие от ИИ</strong>
              <span>Выберите службу — система соберёт карточку: признаки и службы по
                классификатору, текст пишет модель. Останется проверить и утвердить.</span>
            </button>
            <button className={s.launchBtn} data-testid="dds-start-material"
                    onClick={() => nav('/scenarios')}>
              <strong>Учебный материал</strong>
              <span>Все происшествия: по описанию для модели, вручную, правка и утверждение.</span>
            </button>
            <button className={s.launchBtn} data-testid="dds-start-lessons"
                    onClick={() => nav('/lessons')}>
              <strong>Занятие или экзамен</strong>
              <span>Выдать происшествия группе диспетчеров потоком и следить за ходом.</span>
            </button>
          </div>
        </section>
      )}

      {user?.role !== 'student' && showOp112 && (
      <section className={s.launcher} aria-label="Учебные вызовы">
        <h2 className={s.launcherTitle}>Учебные вызовы</h2>
        <div className={s.launcherList}>
          {scenarios.map((sc) => (
            <button key={sc.id} className={s.launchBtn}
                    data-testid={`launch-t${sc.ticket}-c${sc.callNo}`}
                    onClick={() => void launch(sc.id)}>
              <strong>{scenarioTitle(sc)}</strong>
              <span>{sc.legend.slice(0, 74)}…</span>
            </button>
          ))}
          {scenarios.length === 0 && !error && (
            <p className={s.launcherEmpty}>
              {lesson?.kind === 'marathon'
                ? 'Все вызовы марафона пройдены.'
                : 'Вызовов пока нет.'}
            </p>
          )}
        </div>
      </section>
      )}

      {unfinished && (
        <div className={s.lesson} role="status" data-testid="unfinished-call">
          Есть незаконченный вызов: карточка {unfinished.card.number}
          {unfinished.scenario.ticket != null
            && `, билет ${unfinished.scenario.ticket} · вызов ${unfinished.scenario.callNo}`}.{' '}
          <button className={s.continue} data-testid="continue-call"
                  onClick={() => void proceed(unfinished)}>Продолжить</button>
        </div>
      )}

      {error && <div className={s.error} role="status">{error}</div>}
      {lesson && lesson.kind === 'training' && (
        <div className={s.lesson} role="status" data-testid="lesson-banner">
          Идёт занятие «{lesson.title}».{' '}
          {lesson.mode === 'dds'
            ? 'В журнале только карточки занятия, касающиеся вашей службы.'
            : 'В списке учебных вызовов только вызовы занятия.'}
        </div>
      )}
      {lesson && lesson.kind !== 'training' && (
        <DrillBanner lesson={lesson} row={protocol?.rows[0] ?? null} />
      )}

      {!op112 && (
        <div data-hint="journal">
          <JournalTable rows={rows}
                        pager={{ page, size: PAGE_SIZE, total, onPage: setPage }} />
        </div>
      )}
    </div>
  );
};
