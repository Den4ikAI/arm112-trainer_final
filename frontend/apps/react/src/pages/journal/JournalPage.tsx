import { useEffect, useState } from 'react';
import { JournalTable, type JournalRow } from '@/widgets/journal-table/JournalTable';
import { useTraining } from '@/entities/incident/model';
import { useSession } from '@/entities/session/model';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { ApiError, cards, channel, lessons, scenarios as scenariosApi,
         type CardBrief, type LessonBrief, type ScenarioBrief } from '@/shared/api';
import { useNavigate } from 'react-router-dom';
import { useClock } from '@/shared/lib/useClock';
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
    victims: 'Нет',
    address,
    serviceStatus: c.status,
    waitingSec: c.waitingSec,
    opened: c.opened ?? null,
    recorded: c.recorded ?? null,
    operator: c.operator,
    workstation: '',
    description: c.description,
    to: `/card-dds/${c.id}`,
  };
};

/** Главный экран: поиск, список происшествий, часы. Плюс запуск учебного вызова. */
export const JournalPage = () => {
  const [query, setQuery] = useState('');
  const [rows, setRows] = useState<JournalRow[]>([]);
  const [scenarios, setScenarios] = useState<ScenarioBrief[]>([]);
  // Идущее занятие по группе курсанта. Пока оно идёт, и список вызовов,
  // и лента ограничены его материалом — курсант должен понимать почему
  const [lesson, setLesson] = useState<LessonBrief | null>(null);
  const [error, setError] = useState<string | null>(null);
  const start = useTraining((st) => st.start);
  const user = useSession((st) => st.user);
  const nav = useNavigate();
  const now = useClock();

  // Лента фильтруется сервером по службе пользователя: «в их ленту
  // попадают только профильные события»
  useEffect(() => {
    let alive = true;
    // Две половины экрана грузятся врозь. Раньше это был один
    // Promise.all с одним catch: у курсанта без службы лента
    // отвечает отказом — и вместе с ней пропадали учебные вызовы,
    // а на экране было «Нет связи с сервером», хотя связь есть
    const load = async () => {
      const scope = user?.role === 'student' ? 'service' : 'all';

      const [feed, list] = await Promise.allSettled([
        cards.list({ scope, q: query || undefined }),
        scenariosApi.list(),
      ]);
      if (!alive) return;

      if (feed.status === 'fulfilled') setRows(feed.value.items.map(toRow));
      if (list.status === 'fulfilled') setScenarios(list.value);

      // Занятие — надстройка: не узнали о нём, и экран работает как обычно
      if (user?.role === 'student') {
        const идущие = await lessons.list().catch(() => [] as LessonBrief[]);
        if (alive) setLesson(идущие[0] ?? null);
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
    const topics = user?.serviceId ? [`service:${user.serviceId}`] : [];
    if (topics.length) channel.subscribe(topics);

    const off = channel.on((e) => {
      if (['card.dispatched', 'response.recorded', 'ack.timeout',
           'card.status', 'lagged'].includes(e.type)) {
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
  }, [query, user?.role, user?.serviceId]);

  const launch = async (scenarioId: string) => {
    await start(scenarioId);
    const number = useTraining.getState().card?.number;
    if (number) nav(`/card/${number}`);
  };

  return (
    <div className={s.page}>
      <div className={s.top}>
        <div className={s.searchArea}>
          <div className={s.searchRow}>
            <input className={s.search} value={query} placeholder="Поиск происшествий"
                   aria-label="Поиск происшествий"
                   onChange={(e) => setQuery(e.target.value)} />
            <span className={s.loupe} aria-hidden>⌕</span>
          </div>
          <div className={s.searchSub}>
            <button {...stubProps('Расширенный поиск')} className={s.link}>расширенный по параметрам ⌄</button>
            <button className={s.reset} onClick={() => setQuery('')}>сбросить</button>
          </div>
        </div>

        <div className={s.clock}>
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
          <div className={s.clockUser}><UserMenu /></div>
        </div>
      </div>

      <section className={s.launcher} aria-label="Учебные вызовы">
        <h2 className={s.launcherTitle}>Учебные вызовы</h2>
        <div className={s.launcherList}>
          {scenarios.map((sc) => (
            <button key={sc.id} className={s.launchBtn}
                    data-testid={`launch-t${sc.ticket}-c${sc.callNo}`}
                    onClick={() => void launch(sc.id)}>
              <strong>Билет {sc.ticket} · вызов {sc.callNo}</strong>
              <span>{sc.legend.slice(0, 74)}…</span>
            </button>
          ))}
          {scenarios.length === 0 && !error && (
            <p className={s.launcherEmpty}>Вызовов пока нет.</p>
          )}
        </div>
      </section>

      {error && <div className={s.error} role="status">{error}</div>}
      {lesson && (
        <div className={s.lesson} role="status" data-testid="lesson-banner">
          Идёт занятие «{lesson.title}».{' '}
          {lesson.mode === 'dds'
            ? 'В журнале только карточки занятия, касающиеся вашей службы.'
            : 'В списке учебных вызовов только вызовы занятия.'}
        </div>
      )}

      <JournalTable rows={rows} />
    </div>
  );
};
