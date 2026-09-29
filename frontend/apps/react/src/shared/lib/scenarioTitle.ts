/**
 * Подпись учебного вызова.
 *
 * У вызовов из билетов есть номер билета и вызова. У подготовленных
 * моделью и заведённых вручную их нет, и преподаватель видел ряд
 * одинаковых «Билет · вызов — горит мусоропровод». Такие вызовы
 * подписываем коротким номером и датой: по ним соседние с одной и
 * той же легендой различимы.
 */
export const scenarioTitle = (sc: {
  id: string;
  ticket: number | null;
  callNo: number | null;
  createdAt?: string;
  /** Тема, которую дал преподаватель, — она понятнее номера */
  title?: string | null;
}): string => {
  if (sc.title) return sc.title;
  if (sc.ticket != null) {
    return sc.callNo != null ? `Билет ${sc.ticket} · вызов ${sc.callNo}` : `Билет ${sc.ticket}`;
  }
  const date = sc.createdAt
    ? ` от ${new Date(sc.createdAt).toLocaleDateString('ru-RU', { day: '2-digit', month: '2-digit' })}`
    : '';
  return `Вызов № ${sc.id.slice(0, 6)}${date}`;
};
