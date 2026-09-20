import { useEffect, useState } from 'react';
import { JournalTable, type JournalRow } from '@/widgets/journal-table/JournalTable';
import { useTraining } from '@/entities/incident/model';
import { useSession } from '@/entities/session/model';
import { UserMenu } from '@/widgets/user-menu/UserMenu';
import { cards, channel, scenarios as scenariosApi,
         type CardBrief, type ScenarioBrief } from '@/shared/api';
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
  const [error, setError] = useState<string | null>(null);
  const start = useTraining((st) => st.start);
  const user = useSession((st) => st.user);
  const nav = useNavigate();
  const now = useClock();

  // Лента фильтруется сервером по службе пользователя: «в их ленту
  // попадают только профильные события»
  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const scope = user?.role === 'student' ? 'service' : 'all';
        const [feed, list] = await Promise.all([
          cards.list({ scope, q: query || undefined }),
          scenariosApi.list(),
        ]);
        if (!alive) return;
        setRows(feed.items.map(toRow));
        setScenarios(list);
        setError(null);
      } catch {
        if (alive) setError('Нет связи с сервером');
      }
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

      <JournalTable rows={rows} />
    </div>
  );
};
