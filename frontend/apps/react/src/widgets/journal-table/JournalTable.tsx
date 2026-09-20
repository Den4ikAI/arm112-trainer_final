import { Fragment } from 'react';
import { useNavigate } from 'react-router-dom';
import s from './JournalTable.module.scss';

export interface JournalRow {
  id: string;
  number: string;
  date: string;
  time: string;
  type: string;
  victims: string;
  address: string;
  serviceStatus: string;
  description: string;
  operator: string;
  workstation: string;
  /** Куда ведёт строка: карточка 112 или карточка ДДС. */
  to: string;
}

/** Список происшествий — главный экран, снятый со скриншота журнала. */
export const JournalTable = ({ rows }: { rows: JournalRow[] }) => {
  const nav = useNavigate();

  return (
    <div className={s.wrap}>
      <div className={s.caption}>Список происшествий <span className={s.chev}>^</span></div>

      <div className={s.scroll}>
        <table className={s.table}>
        <thead>
          <tr>
            <th className={s.thNarrow}>Связи</th>
            <th className={s.thNarrow}>ЧС</th>
            <th className={s.thNarrow}>Опер.</th>
            <th className={s.thNarrow}>АРМ</th>
            <th>Номер</th>
            <th>Дата</th>
            <th>Время</th>
            <th className={s.thWide}>Тип происшествия</th>
            <th className={s.thNarrow}>Постр.</th>
            <th className={s.thWide}>Адрес</th>
            <th>Статус службы</th>
          </tr>
        </thead>

        <tbody>
          {rows.length === 0 && (
            <tr><td className={s.empty} colSpan={11}>Нет происшествий</td></tr>
          )}

          {rows.map((r) => (
            <Fragment key={r.id}>
              <tr className={s.row} data-testid={`journal-row-${r.number}`}
                  onClick={() => nav(r.to)}>
                <td><span className={s.chev}>˅</span></td>
                <td />
                <td className={s.oper}>{r.operator}</td>
                <td>{r.workstation}</td>
                <td>{r.number}</td>
                <td>{r.date}</td>
                <td className={s.time}>{r.time}</td>
                <td className={s.type}>{r.type}</td>
                <td className={s.victims}>{r.victims}</td>
                <td className={s.addr}>{r.address}</td>
                <td>{r.serviceStatus}</td>
              </tr>
              <tr className={s.descRow}>
                <td colSpan={11}>
                  <span className={s.descLabel}>Описание:</span>
                  <span className={s.descMeta}>{r.date} {r.time} {r.operator} - </span>
                  <span className={s.descText}>{r.description}</span>
                </td>
              </tr>
            </Fragment>
          ))}
        </tbody>
        </table>
      </div>

      <div className={s.pager}>
        <span>Страница: 1</span>
        <span>Записей на странице: 10</span>
        <strong>1-{rows.length} из {rows.length}</strong>
      </div>
    </div>
  );
};
