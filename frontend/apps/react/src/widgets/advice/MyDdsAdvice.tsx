import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { ApiError, ddsAdvice, type DdsAdvice } from '@/shared/api';
import { useSession } from '@/entities/session/model';
import s from './Advice.module.scss';

interface Props {
  /** Карточка, в разборе которой показываем: вердикты, поставленные ею,
   *  и отметка, что она выдана ИИ под рекомендацию. Без карточки —
   *  раздел работ курсанта. */
  cardId?: string;
  /** Служба закончила работу по карточке — разбор окончательный,
   *  можно показывать рекомендации и брать следующее задание. */
  closed?: boolean;
}

/**
 * Рекомендации диспетчеру ДДС и следующее задание от ИИ под них.
 *
 * Звено «результат → рекомендации → новое задание» без преподавателя
 * (рекомендации финалистам, п. 5 и 10). Сверху — вердикт по прошлым
 * рекомендациям, которые проверила эта карточка. Ниже — открытые:
 * что тренировать дальше, и у каждой кнопка «Следующее задание от
 * ИИ»: система соберёт новое происшествие службы курсанта и отметит,
 * какую рекомендацию оно отрабатывает.
 *
 * Пока рекомендаций нет — блок не рисуется.
 */
export const MyDdsAdvice = ({ cardId, closed = true }: Props) => {
  const user = useSession((st) => st.user);
  const [все, setВсе] = useState<DdsAdvice[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const nav = useNavigate();

  const student = user?.role === 'student' && user.duty === 'dds';

  useEffect(() => {
    if (!student) return;
    let alive = true;
    void ddsAdvice.mine()
      .then((r) => { if (alive) setВсе(r); })
      .catch(() => { if (alive) setВсе([]); });
    return () => { alive = false; };
  }, [student, cardId, closed]);

  if (!student || !все) return null;

  const отрабатываем = cardId ? все.filter((a) => a.nextCardId === cardId) : [];
  const проверены = cardId ? все.filter((a) => a.checkCardId === cardId) : [];
  // В разборе ещё идущей карточки открытые рекомендации не показываем:
  // они от прошлых карточек, а эта ещё не разобрана до конца
  const открыты = closed ? все.filter((a) => a.checkedAt === null) : [];
  if (отрабатываем.length === 0 && проверены.length === 0 && открыты.length === 0) return null;

  const next = async (a: DdsAdvice) => {
    setBusy(a.id);
    setError(null);
    try {
      const got = await ddsAdvice.next(a.id);
      nav(`/card-dds/${got.cardId}`);
    } catch (e) {
      setError(e instanceof ApiError ? (e.detail ?? e.message) : 'Не получилось собрать происшествие');
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className={s.wrap} aria-label="Рекомендации диспетчеру" data-testid="my-dds-advice">
      {отрабатываем.map((a) => (
        <p key={a.id} className={s.focus} data-testid="dds-advice-focus">
          Задание от ИИ под рекомендацию: {a.text}
        </p>
      ))}

      {проверены.length > 0 && (
        <>
          <h2 className={s.title}>Прошлые рекомендации</h2>
          <ul className={s.list}>
            {проверены.map((a) => (
              <li key={a.id} className={s.item} data-testid="dds-advice-verdict">
                <span className={a.worked ? s.ok : s.bad}>
                  {a.worked ? 'выполнена' : 'не выполнена'}
                </span>
                <span>{a.text}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      {открыты.length > 0 && (
        <>
          <h2 className={s.title}>Что тренировать дальше</h2>
          <ul className={s.list}>
            {открыты.map((a) => (
              <li key={a.id} className={s.item} data-testid="dds-advice-open">
                <span className={s.muted}>{a.label}</span>
                <span>
                  {a.text}
                  <div className={s.actions}>
                    <button className={s.btn} data-testid="dds-advice-next"
                            disabled={busy !== null}
                            title="Система соберёт новое происшествие вашей службы и отметит, какую рекомендацию вы отрабатываете"
                            onClick={() => void next(a)}>
                      {busy === a.id ? 'ИИ готовит задание…' : 'Следующее задание от ИИ'}
                    </button>
                    {a.nextCardId && (
                      <button className={s.btn} data-testid="dds-advice-next-open"
                              onClick={() => nav(`/card-dds/${a.nextCardId}`)}>
                        Открыть выданное задание
                      </button>
                    )}
                  </div>
                </span>
              </li>
            ))}
          </ul>
          {busy && (
            <p className={s.note} role="status" data-testid="dds-advice-wait">
              Собираю происшествие для вашей службы: на видеокарте — секунды, на процессоре —
              до двух минут. Карточка откроется сама.
            </p>
          )}
          {error && <p className={s.bad} role="alert">{error}</p>}
          <p className={s.note}>
            Рекомендация выдаётся по замечанию разбора закрытой карточки и проверяется
            следующей карточкой, где ошибку можно повторить.
          </p>
        </>
      )}
    </section>
  );
};
