/**
 * Формы ответов сервера. Списаны с обработчиков в backend/src/api,
 * сериализация camelCase.
 */

export type Role = 'student' | 'teacher' | 'admin';

export interface User {
  id: string;
  login: string;
  fio: string;
  role: Role;
  serviceId: string | null;
  workstation: string | null;
}

export interface IncidentType {
  id: string;
  title: string;
  frequent: boolean;
  group: number | null;
}

export interface Service {
  id: string;
  title: string;
}

export interface SurveyQuestion {
  id: string;
  label: string;
  kind: 'single' | 'multi' | 'text' | 'number';
  options: string[];
}

export interface SurveyCard {
  typeId: string;
  title: string;
  questions: SurveyQuestion[];
}

export interface NotifiedService {
  serviceId: string;
  title: string;
  /** Значение из матрицы: «пожар: квартира», «карточка-112». */
  value: string;
  responding: boolean;
  condition: string;
}

export interface ClassifyResult {
  outcome: string | null;
  positions: string[];
  services: NotifiedService[];
  /** Службы, получающие копию. Оператору не показываются. */
  informational: NotifiedService[];
  /** Чего не хватило, если позиция не определилась. */
  reason: string | null;
}

export interface Card {
  id: string;
  number: string;
  sessionId: string | null;
  createdAt: string;
  savedAt: string | null;
  operator: string;
  workstation: string | null;
  phoneAon: string;
  phoneProvided: string;
  phoneOnSite: string;
  applicantName: string;
  applicantStatus: string;
  address: Record<string, string>;
  description: string;
  victims: number | null;
  noContact: boolean;
  callDropped: boolean;
  typeIds: string[];
  answers: Record<string, string[] | string>;
  outcomeType: string | null;
  status: string;
  serviceIds: string[];
}

export interface ScenarioQuestion {
  factId: string;
  key: string;
  question: string;
  revealed: boolean;
}

/** Сценарий без эталона: ответы приходят по одному, через /ask. */
export interface ScenarioPublic {
  id: string;
  ticket: number | null;
  callNo: number | null;
  legend: string;
  applicantName: string;
  applicantPhone: string;
  applicantStatus: string;
  normSec: number;
  questions: ScenarioQuestion[];
}

export interface TrainingSession {
  id: string;
  mode: 'card112' | 'dds';
  startedAt: string;
  finishedAt: string | null;
  normSec: number;
}

export type FindingKind = 'ok' | 'guessed' | 'wrong' | 'missing' | 'extra';

export interface Finding {
  kind: FindingKind;
  field: string;
  label: string;
  expected?: string;
  actual?: string;
  consequence?: string;
}

export interface Grade {
  findings: Finding[];
  elapsedSec: number;
  normSec: number;
  overNorm: boolean;
  services: { expected: string[]; actual: string[]; missed: string[]; extra: string[] };
  score: number;
}

export interface TranscriptLine {
  who: string;
  text: string;
  atSec: number;
}

export interface CardBrief {
  id: string;
  number: string;
  createdAt: string;
  savedAt: string | null;
  address: Record<string, string>;
  description: string;
  outcomeType: string | null;
  status: string;
  operator: string;
  /** Секунды ожидания подтверждения ДДС. Норматив — 30. */
  waitingSec: number | null;
}

export interface ResponseEntry {
  id: string;
  serviceId: string;
  status: string;
  at: string;
  operator: string;
  orderNo: string | null;
  comment: string | null;
}

export interface ServiceResponses {
  serviceId: string;
  title: string;
  entries: ResponseEntry[];
  /** Что можно выбрать следующим. Считает сервер. */
  allowedNext: string[];
}

export type CardDetail = Card & { services: ServiceResponses[] };
