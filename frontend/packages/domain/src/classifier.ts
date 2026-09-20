/**
 * Движок эталона: признаки → итоговый тип → список оповещения.
 *
 * ВНИМАНИЕ: это ВРЕМЕННАЯ ЗАГЛУШКА, а не Единый классификатор.
 *
 * Настоящий ЕКП — 1283 позиции, 58 служб, 104 колонки — импортирован
 * на бэкенде (backend/src/ingest/classifier.rs). Правила ниже я вывел
 * вручную, чтобы фронтенд заработал до появления сервера. Они ПОХОЖИ
 * на логику ЕКП, но против настоящих данных не сверялись и могут
 * расходиться с ним.
 *
 * Использовать их для оценки курсантов нельзя. Файл удаляется в задаче
 * #18, когда фронтенд перейдёт на POST /api/classifier/evaluate.
 *
 * Устройство настоящего классификатора отличается: там не сложение
 * правил, а точный поиск строки по комбинации признаков.
 */
import type { IncidentCard } from './types';

export interface ClassifierRule {
  /** Тип происшествия, к которому относится правило. */
  typeId: string;
  /** Условия по ответам опросной карты: questionId → допустимые значения. */
  when?: Record<string, string[]>;
  /** Итоговый тип происшествия — то, что попадает в строку «Класс.:». */
  outcome?: string;
  /** Службы, добавляемые в список оповещения. */
  services: string[];
}

export const RULES: ClassifierRule[] = [
  // --- 101, пожар ---
  { typeId: '101', outcome: 'пожар', services: ['s101'] },
  { typeId: '101', when: { 'where': ['Дом'], 'house-kind': ['Дом многоквартирный'] },
    outcome: 'пожар: квартира', services: ['s101', 'dep-jkh', 'mosbez'] },
  { typeId: '101', when: { 'inhouse': ['Квартира'] },
    outcome: 'пожар: квартира', services: ['s101', 's102', 'dep-jkh'] },
  { typeId: '101', when: { 'inhouse': ['Лифт'] }, services: ['moslift'] },
  { typeId: '101', when: { 'inhouse': ['Газовая колонка', 'Газовая плита'] },
    services: ['s104', 'mosgaz'] },
  { typeId: '101', when: { 'gasified': ['Да'] }, services: ['s104'] },
  { typeId: '101', when: { 'people-threat': ['Да'] }, services: ['s103', 'cemp'] },
  { typeId: '101', when: { 'road-block': ['Да'] }, services: ['codd'] },
  { typeId: '101', when: { 'where': ['Улица'] }, outcome: 'пожар: мусор', services: ['s101', 'oati'] },

  // --- 104, запах газа ---
  { typeId: '104', outcome: 'запах газа', services: ['s104', 'mosgaz'] },
  { typeId: '104', when: { 'gas-sign': ['Запах газа в помещении (в квартире, в доме)'] },
    outcome: 'запах бытового газа в помещении', services: ['s104', 's101', 'dep-jkh'] },
  { typeId: '104', when: { 'people-threat': ['Да'] }, services: ['s103', 'cemp', 's101'] },

  // --- Взрыв ---
  { typeId: 'explosion', outcome: 'взрыв', services: ['s101', 's102', 'fsb', 'mosbez'] },
  { typeId: 'explosion', when: { 'has-fire': ['Да'] }, services: ['s101'] },
  { typeId: 'explosion', when: { 'collapse-threat': ['Да'] }, services: ['s103', 'cemp', 'dep-jkh'] },

  // Правил для ДТП и БПЛА здесь нет намеренно: они опирались на опросные
  // карты, которых на скриншотах не было. Настоящие правила для этих
  // групп придут из импортированного классификатора.
];

const answerValues = (card: IncidentCard, questionId: string): string[] => {
  const v = card.answers[questionId];
  if (v == null) return [];
  return Array.isArray(v) ? v : [v];
};

const ruleMatches = (rule: ClassifierRule, card: IncidentCard): boolean => {
  if (!card.typeIds.includes(rule.typeId)) return false;
  if (!rule.when) return true;
  return Object.entries(rule.when).every(([questionId, allowed]) => {
    const got = answerValues(card, questionId);
    return got.some((g) => allowed.includes(g));
  });
};

export interface ClassifyResult {
  /** Итоговый тип происшествия — самое специфичное сработавшее правило. */
  outcome: string;
  /** Список оповещения, порядок — как в SERVICES. */
  services: string[];
  /** Какие правила сработали — нужно для объяснения оценки курсанту. */
  firedRules: ClassifierRule[];
}

/** Считает эталонный список оповещения по заполненной карточке. */
export const classify = (card: IncidentCard): ClassifyResult => {
  const fired = RULES.filter((r) => ruleMatches(r, card));
  const services = new Set<string>();
  let outcome = '';
  let bestSpecificity = -1;
  for (const rule of fired) {
    rule.services.forEach((s) => services.add(s));
    const specificity = rule.when ? Object.keys(rule.when).length : 0;
    if (rule.outcome && specificity >= bestSpecificity) {
      bestSpecificity = specificity;
      outcome = rule.outcome;
    }
  }
  return { outcome, services: [...services], firedRules: fired };
};
