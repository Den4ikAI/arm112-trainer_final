/**
 * Машина статусов реагирования ДДС.
 * Памятка для ДДС, раздел «Проставление статусов реагирования»:
 * выбрать статус можно только последовательно. Сначала доступны два первичных —
 * «Принята» и «Не принята». После «Принята» открываются статусы хода работ.
 * После «Не принята» единственный доступный вариант — «Принята».
 */
import type { ResponseStatus, ResponseEntry } from './types';

/** Норматив подтверждения карточки диспетчером ДДС, секунды. */
export const ACK_NORM_SEC = 30;
/** Порог перехода карточки в «Не завершено», часы. */
export const COMPLETION_NORM_HOURS = 48;

const AFTER_ACCEPTED: ResponseStatus[] = [
  'Начало реагирования', 'Прибытие', 'Проведение работ',
  'Работы завершены', 'Отказ от выполнения работ',
];

/** Какие статусы можно выбрать следующими. */
export const nextStatuses = (entries: ResponseEntry[]): ResponseStatus[] => {
  const chain = entries.map((e) => e.status);
  const last = [...chain].reverse()
    .find((s) => s !== 'Добавлена' && s !== 'Получена службой');

  if (!last) return ['Принята', 'Не принята'];
  if (last === 'Не принята') return ['Принята'];
  // Терминальные статусы закрывают карточку для редактирования.
  if (last === 'Работы завершены' || last === 'Отказ от выполнения работ') return [];

  const idx = AFTER_ACCEPTED.indexOf(last as ResponseStatus);
  if (last === 'Принята') return AFTER_ACCEPTED;
  // Дальше по цепочке можно идти только вперёд.
  return AFTER_ACCEPTED.slice(idx + 1);
};

/** Статусы, для которых комментарий обязателен (памятка, «Полнота информации»). */
export const requiresComment = (status: ResponseStatus): boolean =>
  status === 'Не принята' || status === 'Отказ от выполнения работ';

/** Терминальные статусы: после них карточка закрыта для редактирования. */
export const isTerminal = (status: ResponseStatus): boolean =>
  status === 'Работы завершены' || status === 'Отказ от выполнения работ';
