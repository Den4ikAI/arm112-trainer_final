/**
 * Формы данных для диаграмм — ровно то, что отдаёт
 * `backend/src/reports/group.rs`.
 *
 * Отдельный файл, потому что диаграммы не должны знать ни про API,
 * ни про экран: им дают числа, они рисуют. Так их можно проверить
 * на любых данных, включая пустые.
 */

export interface HeatCell {
  label: string;
  kind: string;
  count: number;
}

export interface TimelinePoint {
  at: string;
  fio: string;
  elapsedSec: number;
  normSec: number;
  score: number;
  overNorm: boolean;
}

export interface Funnel {
  learned: number;
  guessed: number;
  wrong: number;
  missing: number;
}

export interface KindCount {
  kind: string;
  count: number;
}

/** Подписи видов замечаний — те же, что в разборе занятия. */
export const KIND_LABEL: Record<string, string> = {
  ok: 'верно',
  guessed: 'угадал',
  wrong: 'ошибка',
  missing: 'не заполнено',
  extra: 'лишнее',
};

/**
 * Цвета видов. Смысловые, а не декоративные: «угадал» оранжевый,
 * потому что это не ошибка, но и не работа; ошибка красная,
 * пропуск серый.
 */
export const KIND_COLOR: Record<string, string> = {
  ok: '#2e7d32',
  guessed: '#ed8b00',
  wrong: '#d32f2f',
  missing: '#8a8a8a',
  extra: '#7b5cd6',
};
