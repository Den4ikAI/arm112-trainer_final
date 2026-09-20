import { useState } from 'react';
import { JournalTable, type JournalRow } from '@/widgets/journal-table/JournalTable';
import { SCENARIOS } from '@arm112/domain';
import { useTraining } from '@/entities/incident/model';
import { useNavigate } from 'react-router-dom';
import s from './JournalPage.module.scss';

const ROWS: JournalRow[] = [
  {
    id: '1', number: '36814848', date: '17.09.26', time: '11:16:53', type: 'ДТП',
    victims: 'Нет', address: 'Москва , (ТАО, Вороновское) , Троицкий административный округ',
    serviceStatus: 'Добавлена', operator: '0', workstation: '4',
    description: 'ДТП, столкнулись 2 автомобиля, разлитие топлива, есть пострадавшие',
    to: '/card-dds/36814848',
  },
  {
    id: '2', number: '36814845', date: '17.09.26', time: '11:12:43', type: '101',
    victims: 'Нет', address: 'Москва , (ТАО, Вороновское) , Троицкий административный округ',
    serviceStatus: 'Добавлена', operator: '0', workstation: '4',
    description: 'Пожар в квартире', to: '/card-dds/36814845',
  },
];

/** Главный экран: поиск, список происшествий, часы. Плюс запуск учебного вызова. */
export const JournalPage = () => {
  const [query, setQuery] = useState('');
  const start = useTraining((st) => st.startScenario);
  const nav = useNavigate();
  const now = new Date();

  const launch = (scenarioId: string) => {
    const number = String(36814850 + Math.floor(Math.random() * 40));
    start(scenarioId, number);
    nav(`/card/${number}`);
  };

  const rows = ROWS.filter((r) =>
    !query || `${r.number} ${r.type} ${r.address} ${r.description}`
      .toLowerCase().includes(query.toLowerCase()));

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
            <button className={s.link}>расширенный по параметрам ⌄</button>
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
          <div className={s.clockUser}>, УМЦ O n</div>
        </div>
      </div>

      <section className={s.launcher} aria-label="Учебные вызовы">
        <h2 className={s.launcherTitle}>Учебные вызовы</h2>
        <div className={s.launcherList}>
          {SCENARIOS.map((sc) => (
            <button key={sc.id} className={s.launchBtn}
                    data-testid={`launch-${sc.id}`} onClick={() => launch(sc.id)}>
              <strong>Билет {sc.ticket} · вызов {sc.callNo}</strong>
              <span>{sc.legend.slice(0, 74)}…</span>
            </button>
          ))}
        </div>
      </section>

      <JournalTable rows={rows} />
    </div>
  );
};
