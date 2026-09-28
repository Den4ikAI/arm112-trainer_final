import { useNavigate } from 'react-router-dom';
import s from './NotForDuty.module.scss';

/**
 * Экран другого рабочего места.
 *
 * Оператор 112 и диспетчер ДДС работают в одном интерфейсе, но у каждого
 * свой функционал. Прямая ссылка на чужой экран должна объяснять, почему
 * здесь пусто, а не показывать пустоту: иначе человек решит, что сломалось.
 * Сервер эти действия тоже не пропустит — экран лишь говорит об этом первым.
 */
export const NotForDuty = ({ what, yours }: { what: string; yours: string }) => {
  const nav = useNavigate();
  return (
    <div className={s.page} data-testid="not-for-duty">
      <div className={s.box}>
        <h1 className={s.title}>Недоступно для вашего профиля</h1>
        <p>{what}</p>
        <p className={s.muted}>
          Ваш профиль — {yours}. Направить на другое рабочее место может
          преподаватель или администратор.
        </p>
        <button className={s.back} onClick={() => nav('/journal')}>В журнал</button>
      </div>
    </div>
  );
};
