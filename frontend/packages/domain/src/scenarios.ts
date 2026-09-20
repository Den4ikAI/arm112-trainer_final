/**
 * Учебные сценарии.
 *
 * ПРОИСХОЖДЕНИЕ. Источник — «Билеты-задачи по С112» (32 билета × 3 вызова),
 * скан распознан, полный текст в extracted/bilety_96_scenarios.md.
 *
 *   Из билетов взято дословно: ситуация, ФИО заявителя, телефон, адрес
 *   и скрытая часть адреса в скобках.
 *
 *   Написано нами: реплики. Билет даёт сухое описание — «Горит крыша
 *   частного дома, пострадавших нет, дом не газифицирован». Прямая речь
 *   заявителя, вопросы оператора и ответы на них — наша реконструкция
 *   того же содержания. Фактов сверх билета в них нет, но это не цитаты.
 *
 * Механика: скрытые факты заявитель сообщает только в ответ на вопрос.
 * Это заложено авторами билетов — настоящий адрес напечатан в скобках,
 * а заявитель называет ориентир. Курсант, заполнивший поле без вопроса,
 * ГАДАЛ, а не ВЫЯСНИЛ.
 *
 * СОСТОЯНИЕ НА 21.09.2026. Сценарии переехали на сервер: миграция
 * backend/migrations/20260921130000_seed_scenarios.sql, отдаются через
 * POST /api/sessions. Там expected.services прогнан через настоящий
 * классификатор, здесь — через фронтовую заглушку classifier.ts,
 * и идентификаторы служб у них РАЗНЫЕ: на сервере они из самого ЕКП
 * (mchs, codd, gup-msr), здесь — из подписей на кадре (s101, dep-jkh).
 *
 * Этот файл — временный, удаляется вместе с classifier.ts в задаче #18.
 */
import type { ApplicantStatus } from './types';

export interface HiddenFact {
  id: string;
  /** Что спросить, чтобы заявитель это сообщил. */
  question: string;
  /** Что отвечает заявитель. */
  answer: string;
  /** Поля карточки, которые становятся «выясненными» после ответа. */
  unlocks: string[];
}

export interface Scenario {
  id: string;
  ticket: number;
  callNo: number;
  /** Первая фраза заявителя — то, что слышно сразу. */
  legend: string;
  applicantName: string;
  applicantPhone: string;
  applicantStatus: ApplicantStatus;
  /** Норматив заполнения карточки, секунды. Настраивается преподавателем. */
  normSec: number;
  hidden: HiddenFact[];
  /** Эталон: что должно оказаться в карточке. */
  expected: {
    typeIds: string[];
    answers: Record<string, string[] | string>;
    address: Partial<Record<string, string>>;
    /** Службы считаются движком классификатора, здесь — контрольная сумма. */
    services: string[];
  };
}

export const SCENARIOS: Scenario[] = [
  {
    id: 't3-c1', ticket: 3, callNo: 1,
    legend: 'Алло! Горит крыша частного дома! Пожалуйста, приезжайте скорее, ' +
            'это на Станционной улице, дом 28!',
    applicantName: 'Иванова Инна Степановна',
    applicantPhone: '916-126-34-71',
    applicantStatus: 'очевидец',
    normSec: 60,
    hidden: [
      { id: 'city', question: 'Уточните, пожалуйста, город — вы в Москве?',
        answer: 'Нет, что вы, это Королёв, Московская область! Улица Станционная, 28.',
        unlocks: ['address.settlement', 'address.region'] },
      { id: 'victims', question: 'Пострадавшие есть?',
        answer: 'Нет, пострадавших нет, все вышли.', unlocks: ['victims'] },
      { id: 'gas', question: 'Дом газифицирован?',
        answer: 'Нет, дом не газифицирован.', unlocks: ['answers.gasified'] },
      { id: 'house', question: 'Что за дом — частный, многоквартирный?',
        answer: 'Частный дом, одноэтажный. Горит крыша.', unlocks: ['answers.house-kind'] },
      { id: 'flame', question: 'Открытое пламя видно или только дым?',
        answer: 'Пламя видно, крыша горит открыто.', unlocks: ['answers.fire-sign'] },
    ],
    expected: {
      typeIds: ['101'],
      answers: {
        'where': ['Дом'], 'fire-sign': ['Открытое пламя / Дым'],
        'house-kind': ['Дом частный'], 'people-threat': ['Нет'],
        'inhouse': ['Крыша'], 'gasified': ['Нет'],
      },
      address: { region: 'Московская область', settlement: 'Королёв',
                 street: 'Станционная', house: '28' },
      services: ['s101'],
    },
  },
  {
    id: 't2-c1', ticket: 2, callNo: 1,
    legend: 'Здравствуйте, у нас в доме мусоропровод задымил, дым в подъезде идёт. ' +
            'Берзарина, 21, корпус 1.',
    applicantName: 'Ким Олег Юрьевич',
    applicantPhone: '916-126-34-71',
    applicantStatus: 'очевидец',
    normSec: 60,
    hidden: [
      { id: 'floor', question: 'На каком вы этаже, сколько этажей в доме?',
        answer: 'Я на седьмом, в доме семнадцать этажей.',
        unlocks: ['answers.storeys', 'address.floor'] },
      { id: 'flame', question: 'Открытое пламя видите?',
        answer: 'Нет, открытого пламени не вижу, только дым.', unlocks: ['answers.fire-sign'] },
      { id: 'victims', question: 'Пострадавшие есть?',
        answer: 'Нет, пострадавших нет.', unlocks: ['victims'] },
      { id: 'entrance', question: 'Номер подъезда и домофон подскажете?',
        answer: 'Подъезд третий, домофон 68.', unlocks: ['address.entrance', 'address.code'] },
    ],
    expected: {
      typeIds: ['101'],
      answers: {
        'where': ['Дом'], 'fire-sign': ['Открытое пламя / Дым'],
        'house-kind': ['Дом многоквартирный'], 'inhouse': ['Мусоропровод'],
        'people-threat': ['Нет'], 'storeys': '17',
      },
      address: { region: 'Москва', street: 'Берзарина', house: '21',
                 building: '1', entrance: '3', code: '68' },
      services: ['s101', 'dep-jkh', 'mosbez'],
    },
  },
  {
    id: 't1-c1', ticket: 1, callNo: 1,
    legend: 'Тут мусорный контейнер загорелся, рядом со станцией. Депо какое-то, ' +
            'около Москвы-Пассажирской Киевской.',
    applicantName: 'Сидоров Иван Сергеевич',
    applicantPhone: '916-126-34-71',
    applicantStatus: 'очевидец',
    normSec: 60,
    hidden: [
      { id: 'landmark', question: 'Есть рядом какой-то заметный ориентир?',
        answer: 'Да, длинное помещение недалеко от участкового пункта полиции.',
        unlocks: ['address.descriptive'] },
      { id: 'victims', question: 'Пострадавшие есть?',
        answer: 'Нет, пострадавших нет.', unlocks: ['victims'] },
      { id: 'where', question: 'Это на улице или внутри помещения?',
        answer: 'На улице, контейнер во дворе стоит.', unlocks: ['answers.where'] },
    ],
    expected: {
      typeIds: ['101'],
      answers: { 'where': ['Улица'], 'fire-sign': ['Открытое пламя / Дым'], 'people-threat': ['Нет'] },
      address: { region: 'Москва', street: 'МЖД Киевская 1 км', house: '2' },
      services: ['s101', 'oati'],
    },
  },
];

export const getScenario = (id: string): Scenario | undefined =>
  SCENARIOS.find((s) => s.id === id);
