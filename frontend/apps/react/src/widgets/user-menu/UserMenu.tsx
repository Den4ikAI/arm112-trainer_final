import { useLocation, useNavigate } from 'react-router-dom';
import { useSession, roleTitle, dutyTitle } from '@/entities/session/model';
import { useCatalog } from '@/entities/catalog/model';
import { useStand } from '@/entities/stand/model';
import s from './UserMenu.module.scss';

/** Экраны преподавателя: путь, подпись, testid */
const TEACHER_LINKS: [string, string, string][] = [
  ['/lessons', 'занятия', 'lessons-link'],
  ['/scenarios', 'вызовы', 'scenarios-link'],
  ['/analytics', 'аналитика', 'analytics-link'],
  ['/weights', 'веса оценки', 'weights-link'],
  ['/phonebook', 'телефоны ДДС', 'phonebook-link'],
  ['/demo-stats', 'демо-статистика', 'demo-stats-link'],
  ['/journal', 'журнал', 'journal-link'],
];

/**
 * Кто вошёл, куда может перейти и как выйти.
 *
 * Отдельный виджет, а не кусок разметки журнала: выход нужен с любого
 * экрана, где есть шапка, и дублировать его в трёх местах — значит
 * однажды забыть в четвёртом.
 *
 * У преподавателя и администратора — одна навигация на всех экранах:
 * «занятия · вызовы · аналитика · веса оценки · журнал · выйти».
 * Раньше полный набор был только на «Занятиях», а на материале,
 * весах, аналитике и ходе занятия — одно «← В журнал», и из весов в
 * аналитику шли через журнал (UX-проход 1, находка 6).
 */
export const UserMenu = () => {
  const user = useSession((st) => st.user);
  const signOut = useSession((st) => st.signOut);
  const nav = useNavigate();
  const { pathname } = useLocation();
  const serviceShort = useCatalog((st) => st.serviceShort);
  // Приём вызовов 112 свёрнут — в материале только происшествия ДДС
  const op112 = useStand((st) => st.op112);

  if (!user) return null;

  // Рабочее место обучающегося: «один интерфейс, две мини-роли» —
  // человеку надо сразу видеть, за кого он сейчас работает
  const duty = user.role === 'student' && user.duty
    ? dutyTitle(user.duty) + (user.duty === 'dds' && user.serviceId
      ? ` · ${serviceShort(user.serviceId)}` : '')
    : null;

  // Выход гасит канал класса и забывает токен; дальше Private
  // сам перебросит на форму входа
  const leave = async () => {
    await signOut();
    nav('/login', { replace: true });
  };

  return (
    <span className={s.wrap}>
      <span className={s.who} title={roleTitle(user.role)}>{user.fio}</span>
      {duty && <span className={s.duty} data-testid="duty-badge">{duty}</span>}

      {/* Аналитика по группе — учебный процесс, это преподавателю.
          Администрирование — системные журналы и учётные записи,
          по ТЗ это функции администратора, и сервер туда
          преподавателя не пустит */}
      {user.role !== 'student' && (
        <>
          {TEACHER_LINKS.map(([path, label, testid]) => (
            <button key={path} className={path === '/demo-stats' ? `${s.link} ${s.accent}` : s.link}
                    data-testid={testid}
                    aria-current={pathname === path ? 'page' : undefined}
                    onClick={() => nav(path)}>
              {path === '/scenarios' && !op112 ? 'учебный материал' : label}
            </button>
          ))}
        </>
      )}
      {user.role === 'admin' && (
        <button className={s.link} data-testid="admin-link"
                aria-current={pathname === '/admin' ? 'page' : undefined}
                onClick={() => nav('/admin')}>администрирование</button>
      )}

      <button className={s.link} data-testid="logout" onClick={() => void leave()}>
        выйти
      </button>
    </span>
  );
};
