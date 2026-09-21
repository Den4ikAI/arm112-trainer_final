import { useNavigate } from 'react-router-dom';
import { useSession, roleTitle } from '@/entities/session/model';
import s from './UserMenu.module.scss';

/**
 * Кто вошёл, куда может перейти и как выйти.
 *
 * Отдельный виджет, а не кусок разметки журнала: выход нужен с любого
 * экрана, где есть шапка, и дублировать его в трёх местах — значит
 * однажды забыть в четвёртом.
 */
export const UserMenu = ({ compact }: { compact?: boolean }) => {
  const user = useSession((st) => st.user);
  const signOut = useSession((st) => st.signOut);
  const nav = useNavigate();

  if (!user) return null;

  // Выход гасит канал класса и забывает токен; дальше Private
  // сам перебросит на форму входа
  const leave = async () => {
    await signOut();
    nav('/login', { replace: true });
  };

  return (
    <span className={s.wrap}>
      <span className={s.who} title={roleTitle(user.role)}>{user.fio}</span>

      {/* Аналитика по группе — учебный процесс, это преподавателю.
          Администрирование — системные журналы и учётные записи,
          по ТЗ это функции администратора, и сервер туда
          преподавателя не пустит */}
      {!compact && user.role !== 'student' && (
        <>
          <button className={s.link} data-testid="lessons-link"
                  onClick={() => nav('/lessons')}>занятия</button>
          <button className={s.link} data-testid="scenarios-link"
                  onClick={() => nav('/scenarios')}>вызовы</button>
          <button className={s.link} data-testid="analytics-link"
                  onClick={() => nav('/analytics')}>аналитика</button>
        </>
      )}
      {!compact && user.role === 'admin' && (
        <button className={s.link} data-testid="admin-link"
                onClick={() => nav('/admin')}>администрирование</button>
      )}

      <button className={s.link} data-testid="logout" onClick={() => void leave()}>
        выйти
      </button>
    </span>
  );
};
