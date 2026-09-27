import { Fragment } from 'react';
import { useNavigate } from 'react-router-dom';
import clsx from '@/shared/lib/clsx';
import s from './JournalTable.module.scss';

/**
 * Нормативы ДДС, секунды. Ответ заказчика в чате задачи 27.09: 30 секунд
 * от поступления карточки в список до её открытия, 3 минуты — до первой
 * записи. Сервер по ним же переводит карточку в «Не оповещено».
 */
const OPEN_NORM_SEC = 30;
const FIRST_RECORD_NORM_SEC = 180;

export interface JournalRow {
  id: string;
  number: string;
  date: string;
  time: string;
  type: string;
  victims: string;
  address: string;
  serviceStatus: string;
  /** Сколько секунд прошло с поступления карточки в ленту. */
  waitingSec: number | null;
  /** Своя служба открыла карточку; `null` — своей службы в карточке нет. */
  opened: boolean | null;
  /** Своя служба сделала первую запись. */
  recorded: boolean | null;
  description: string;
  operator: string;
  workstation: string;
  /** Куда ведёт строка: карточка 112 или карточка ДДС. */
  to: string;
}

/**
 * Карточка просрочена.
 *
 * Диспетчеру, чья служба в карточке, — по его собственной работе: не
 * открыл за 30 секунд или не сделал первую запись за 3 минуты с
 * поступления. Общий статус карточки тут не годится: в занятии ДДС
 * приходят и старые карточки, давно ставшие «Не оповещено».
 *
 * Остальным — по статусу, который ставит сервер, а секунды считаем
 * сами, чтобы между тактами сторожа (5 с) счётчик не замирал.
 */
const overdue = (r: JournalRow): boolean => {
  const w = r.waitingSec ?? 0;
  if (r.opened != null) {
    return (!r.opened && w > OPEN_NORM_SEC) || (!r.recorded && w > FIRST_RECORD_NORM_SEC);
  }
  return r.serviceStatus === 'Не оповещено'
    || (r.waitingSec != null && w > OPEN_NORM_SEC && r.serviceStatus === 'Зарегистрирована');
};

const fmtWait = (sec: number | null): string => {
  if (sec == null) return '';
  if (sec < 60) return `${sec} с`;
  if (sec < 3600) return `${Math.floor(sec / 60)} мин`;
  return `${Math.floor(sec / 3600)} ч`;
};

/** Постраничный вывод: какая страница, сколько на ней и сколько всего. */
export interface JournalPager {
  /** С нуля, как у сервера */
  page: number;
  size: number;
  total: number;
  onPage: (page: number) => void;
}

/** Список происшествий — главный экран, снятый со скриншота журнала. */
export const JournalTable = ({ rows, pager }: { rows: JournalRow[]; pager?: JournalPager }) => {
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
              <tr
                className={clsx(s.row, overdue(r) && s.overdue)}
                data-testid={`journal-row-${r.number}`}
                data-overdue={overdue(r) ? 'true' : 'false'}
                onClick={() => nav(r.to)}
              >
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
                <td className={s.status}>
                  {r.serviceStatus}
                  {overdue(r) && (
                    <span className={s.overdueBadge}
                          title="Нормативы: открыть — 30 с, первая запись — 3 мин">
                      {fmtWait(r.waitingSec)}
                    </span>
                  )}
                </td>
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

      <Pager rows={rows.length} pager={pager} />
    </div>
  );
};

/**
 * Подвал журнала. Числа — настоящие: раньше подпись была вписана
 * («Записей на странице: 10» при 50 строках), а пустой журнал
 * показывал «1-0 из 0».
 */
const Pager = ({ rows, pager }: { rows: number; pager?: JournalPager }) => {
  const page = pager?.page ?? 0;
  const size = pager?.size ?? rows;
  const total = pager?.total ?? rows;
  const from = rows === 0 ? 0 : page * size + 1;
  const to = rows === 0 ? 0 : page * size + rows;
  const pages = Math.max(1, Math.ceil(total / Math.max(size, 1)));

  return (
    <div className={s.pager} data-testid="journal-pager">
      <span>Страница: {page + 1} из {pages}</span>
      <span>Записей на странице: {size}</span>
      <strong>{from}-{to} из {total}</strong>
      {pager && pages > 1 && (
        <>
          <button className={s.pageBtn} disabled={page === 0}
                  aria-label="Предыдущая страница" onClick={() => pager.onPage(page - 1)}>‹</button>
          <button className={s.pageBtn} disabled={page + 1 >= pages}
                  aria-label="Следующая страница" onClick={() => pager.onPage(page + 1)}>›</button>
        </>
      )}
    </div>
  );
};
