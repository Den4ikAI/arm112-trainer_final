/**
 * Преподавателю: веса оценки, ход занятия по местам, аналитика по
 * группе и по ученику. Пути — как в backend/src/api/grading.rs и
 * backend/src/api/monitor.rs.
 */
import { api } from './client';

export type CheckGroup =
  | 'time' | 'statuses' | 'comments' | 'address' | 'typeServices' | 'completeness' | 'text';

export type Weights = Record<CheckGroup, number>;

export interface WeightsView {
  weights: Weights;
  defaults: Weights;
  criticalCap: number;
}

export interface WeightsPreview {
  total: number;
  changed: number;
  threshold: number;
  before: { avg: number | null; passing: number };
  after: { avg: number | null; passing: number };
  students: Array<{
    userId: string; fio: string; duty: string | null; attempts: number;
    before: number | null; after: number | null;
  }>;
}

export interface MonitorStep { key: string; label: string; done: boolean }

export interface MonitorCard {
  cardId: string;
  number: string;
  theme: string;
  address: string;
  elapsedSec: number;
  normSec: number;
  late: boolean;
  steps: MonitorStep[];
  status: string | null;
}

export interface MonitorSeat {
  seat: number;
  userId: string;
  fio: string;
  duty: 'op112' | 'dds' | string;
  serviceId: string | null;
  workstation: string | null;
  tasksTotal: number;
  tasksDone: number;
  counters: [number, number, number];
  queue: number;
  notNotified: number;
  refused: number;
  current: MonitorCard | null;
  lastScore: number | null;
}

export interface LessonMonitor {
  lessonId: string;
  title: string;
  groupTitle: string;
  mode: 'card112' | 'dds' | string;
  kind: string;
  startedAt: string | null;
  finishedAt: string | null;
  now: string;
  seats: number;
  working: number;
  late: number;
  notNotified: number;
  refused: number;
  notFinished: number;
  graded: number;
  reviewed: number;
  op112: MonitorSeat[];
  dds: MonitorSeat[];
}

export interface GroupRate { group: CheckGroup; errorPct: number | null }

export interface StudentRow {
  userId: string;
  fio: string;
  duty: string;
  attempts: number;
  avgScore: number | null;
  timeRatio: number | null;
  rates: GroupRate[];
  topIssues: string[];
  forecast: number | null;
  forecastError: number | null;
}

export interface GroupAnalytics {
  groupId: string;
  title: string;
  attempts: number;
  avgScore: number | null;
  timeRatio: number | null;
  checks: CheckGroup[];
  typical: Array<{ label: string; count: number }>;
  students: StudentRow[];
}

export interface StudentAnalytics {
  userId: string;
  fio: string;
  duty: string;
  avgScore: number | null;
  attempts: Array<{
    at: string; kind: 'card112' | 'dds' | string; cardId: string; cardNumber: string;
    score: number | null; elapsedSec: number | null; normSec: number | null; issues: string[];
  }>;
  rates: GroupRate[];
  topIssues: Array<{ label: string; count: number }>;
  forecast: Array<{ at: string; predicted: number; actual: number | null }>;
}

/** Подписи групп проверок — как на экране «Веса оценки». */
export const CHECK_GROUP: Record<CheckGroup, { title: string; hint: string }> = {
  time: { title: 'Время: 30 с, 3 мин, набор карточки',
    hint: 'открыть карточку ДДС за 30 с, первая запись за 3 мин, норматив набора карточки 112' },
  statuses: { title: 'Порядок статусов',
    hint: 'статусы по порядку и по докладам бригады, бригада направлена' },
  comments: { title: 'Комментарии',
    hint: 'первая запись со статусом и текстом, мотивировка отказа' },
  address: { title: 'Адрес', hint: 'улица, дом и прочие поля; похожая улица — критичная ошибка' },
  typeServices: { title: 'Тип происшествия и службы', hint: 'тип, требуемые и лишние службы' },
  completeness: { title: 'Полнота карточки и вопросы',
    hint: 'признаки, пострадавшие, «сказал ↔ заполнил»: выяснено или угадано' },
  text: { title: 'Понятность текста',
    hint: 'замечания анализа описания нейросетью, не отклонённые преподавателем' },
};

export const CHECK_ORDER: CheckGroup[] =
  ['time', 'statuses', 'comments', 'address', 'typeServices', 'completeness', 'text'];

export const grading = {
  view: () => api.get<WeightsView>('/api/grading/weights'),
  preview: (w: Weights) => api.post<WeightsPreview>('/api/grading/weights/preview', w),
  save: (w: Weights) => api.put<WeightsPreview>('/api/grading/weights', w),
};

export const monitor = {
  lesson: (id: string) => api.get<LessonMonitor>(`/api/lessons/${id}/monitor`),
};

export const analytics = {
  group: (id: string, duty: 'op112' | 'dds') =>
    api.get<GroupAnalytics>(`/api/analytics/groups/${id}?duty=${duty}`),
  student: (id: string) => api.get<StudentAnalytics>(`/api/analytics/students/${id}`),
};
