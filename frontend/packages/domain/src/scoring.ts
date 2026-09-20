/**
 * Оценка заполнения карточки.
 *
 * Три слоя сравнения (см. IDEAS.md, раздел 6):
 *   1. эталон сценария        — что должно было оказаться в карточке;
 *   2. что курсант ввёл       — фактическое содержимое карточки;
 *   3. что курсант ВЫЯСНИЛ    — какие скрытые факты он вытащил из заявителя.
 *
 * Поле, совпавшее с эталоном, но не подтверждённое разговором, помечается
 * как «угадал». В боевых условиях это ошибка, даже если значение верное.
 */
import type { IncidentCard } from './types';
import { SURVEY_CARDS } from './catalog';
import type { Scenario } from './scenarios';
import { classify } from './classifier';

export type FindingKind = 'ok' | 'guessed' | 'wrong' | 'missing' | 'extra';

export interface Finding {
  kind: FindingKind;
  /** Путь поля: 'address.settlement', 'answers.gasified', 'victims'. */
  field: string;
  label: string;
  expected?: string;
  actual?: string;
  /** Последствие ошибки, а не абстрактный балл. */
  consequence?: string;
}

export interface Grade {
  findings: Finding[];
  /** Секунды, затраченные курсантом. */
  elapsedSec: number;
  normSec: number;
  overNorm: boolean;
  /** Службы: эталон, факт, пропущенные, лишние. */
  services: { expected: string[]; actual: string[]; missed: string[]; extra: string[] };
  score: number;
}

/**
 * Человеческие названия полей для разбора.
 * Признаки берутся прямо из опросных карт, чтобы курсант видел
 * ту же формулировку, что и на экране карточки, а не идентификатор.
 */
const QUESTION_LABELS: Record<string, string> = Object.fromEntries(
  SURVEY_CARDS.flatMap((c) => c.questions.map((q) => [`answers.${q.id}`, q.label])),
);

const FIELD_LABELS: Record<string, string> = {
  'address.region': 'Субъект',
  'address.settlement': 'Населённый пункт',
  'address.street': 'Улица',
  'address.house': 'Дом/Вл.',
  'address.building': 'Корпус',
  'address.entrance': 'Подъезд',
  'address.floor': 'Этаж',
  'address.code': 'Код',
  'address.descriptive': 'Описательный адрес',
  'victims': 'Пострадавшие',
};

const readPath = (card: IncidentCard, path: string): string => {
  const [head, tail] = path.split('.');
  if (head === 'address') {
    const addr = card.address as unknown as Record<string, string>;
    return String(addr[tail] ?? '');
  }
  if (head === 'answers') {
    const v = card.answers[tail];
    return Array.isArray(v) ? v.join(', ') : String(v ?? '');
  }
  if (head === 'victims') return card.victims == null ? '' : String(card.victims);
  return '';
};

const norm = (s: string) => s.trim().toLowerCase().replace(/ё/g, 'е');

export const grade = (
  card: IncidentCard,
  scenario: Scenario,
  elapsedSec: number,
  /** id скрытых фактов, которые курсант успел выяснить. */
  revealedFactIds: string[],
): Grade => {
  const findings: Finding[] = [];

  /** Поля, подтверждённые разговором. */
  const provenFields = new Set<string>();
  scenario.hidden
    .filter((h) => revealedFactIds.includes(h.id))
    .forEach((h) => h.unlocks.forEach((f) => provenFields.add(f)));

  /** Поля, которые вообще можно было выяснить только вопросом. */
  const askableFields = new Set<string>();
  scenario.hidden.forEach((h) => h.unlocks.forEach((f) => askableFields.add(f)));

  const check = (field: string, expected: string) => {
    const actual = readPath(card, field);
    const label = FIELD_LABELS[field] ?? QUESTION_LABELS[field] ?? field.split('.')[1] ?? field;
    if (!actual) {
      findings.push({ kind: 'missing', field, label, expected,
        consequence: 'Поле не заполнено — служба получит неполную карточку.' });
      return;
    }
    if (norm(actual) !== norm(expected)) {
      findings.push({ kind: 'wrong', field, label, expected, actual,
        consequence: field.startsWith('address')
          ? 'Силы и средства уедут не на тот адрес.'
          : 'Признак искажён — список оповещения может быть неверным.' });
      return;
    }
    if (askableFields.has(field) && !provenFields.has(field)) {
      findings.push({ kind: 'guessed', field, label, expected, actual,
        consequence: 'Значение верное, но заявитель его не сообщал — ' +
                     'курсант не выяснил, а предположил.' });
      return;
    }
    findings.push({ kind: 'ok', field, label, expected, actual });
  };

  Object.entries(scenario.expected.address).forEach(([k, v]) => {
    if (v) check(`address.${k}`, v);
  });
  Object.entries(scenario.expected.answers).forEach(([k, v]) => {
    check(`answers.${k}`, Array.isArray(v) ? v.join(', ') : String(v));
  });

  // Тип происшествия.
  scenario.expected.typeIds.forEach((t) => {
    if (!card.typeIds.includes(t)) {
      findings.push({ kind: 'missing', field: 'typeIds', label: 'Тип происшествия',
        expected: t, consequence: 'Без типа список оповещения не сформируется.' });
    }
  });

  // Службы: сравниваем фактический список с тем, что даёт движок классификатора
  // на ЭТАЛОННОЙ карточке.
  const referenceCard: IncidentCard = {
    ...card,
    typeIds: scenario.expected.typeIds,
    answers: scenario.expected.answers,
  };
  const expectedServices = classify(referenceCard).services;
  const actualServices = card.serviceIds;
  const missed = expectedServices.filter((s) => !actualServices.includes(s));
  const extra = actualServices.filter((s) => !expectedServices.includes(s));

  missed.forEach((s) =>
    findings.push({ kind: 'missing', field: `service.${s}`, label: 'Служба', expected: s,
      consequence: 'Служба не оповещена — фактически не выехала.' }));
  extra.forEach((s) =>
    findings.push({ kind: 'extra', field: `service.${s}`, label: 'Служба', actual: s,
      consequence: 'Избыточное оповещение — служба отвлечена на непрофильный вызов.' }));

  const overNorm = elapsedSec > scenario.normSec;

  const weight: Record<FindingKind, number> = {
    ok: 0, guessed: 1, extra: 1, missing: 2, wrong: 3,
  };
  const penalty = findings.reduce((acc, f) => acc + weight[f.kind], 0)
    + (overNorm ? 2 : 0);
  const total = findings.length + 2;
  const score = Math.max(0, Math.round(100 * (1 - penalty / (total * 3))));

  return {
    findings, elapsedSec, normSec: scenario.normSec, overNorm,
    services: { expected: expectedServices, actual: actualServices, missed, extra },
    score,
  };
};
