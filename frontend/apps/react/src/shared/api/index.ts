/** Эндпоинты сервера. Пути — как в backend/src/api. */
import { api, setToken } from './client';
import type {
  AdminUser, AuditPage, Brigade, Card, CardBrief, CardDetail, ClassifierVersion,
  ClassifyResult, Grade, GroupBrief, Health, IncidentType, JobView,
  ScenarioFull, ScenarioPublic, Service, SessionState, SurveyCard, TrainingSession,
  TranscriptLine, TurnResult, User,
} from './types';

export * from './types';
export { ApiError, getToken, setToken } from './client';
export { channel, type ServerEvent } from './ws';

export const auth = {
  login: async (login: string, password: string, workstation?: string) => {
    const r = await api.post<{ token: string; user: User }>('/api/auth/login',
      { login, password, workstation });
    setToken(r.token);
    return r.user;
  },
  me: () => api.get<User>('/api/auth/me'),
  logout: async () => {
    await api.post<void>('/api/auth/logout').catch(() => undefined);
    setToken(null);
  },
};

export const catalog = {
  incidentTypes: () => api.get<IncidentType[]>('/api/catalog/incident-types'),
  services: () => api.get<Service[]>('/api/catalog/services'),
  surveyCard: (typeId: string) =>
    api.get<SurveyCard>(`/api/catalog/survey-cards/${encodeURIComponent(typeId)}`),
  applicantStatuses: () => api.get<string[]>('/api/catalog/applicant-statuses'),
  evaluate: (typeIds: string[], answers: Record<string, string[]>) =>
    api.post<ClassifyResult>('/api/classifier/evaluate', { typeIds, answers }),
};

export interface ScenarioBrief {
  id: string;
  ticket: number | null;
  callNo: number | null;
  legend: string;
  normSec: number;
  questionsCount: number;
  status: 'draft' | 'approved' | 'archived';
  createdAt: string;
}

export const scenarios = {
  list: () => api.get<ScenarioBrief[]>('/api/scenarios'),

  /** Полный вызов вместе с эталоном — только преподавателю. */
  one: (id: string) => api.get<ScenarioFull>(`/api/scenarios/${id}`),

  /** Подготовить вызовы из билетов. Долго, поэтому через очередь. */
  generate: (tickets: number[], count: number) =>
    api.post<{ jobId: string }>('/api/scenarios/generate', { tickets, count }),

  chat: (id: string) => api.get<import('./types').ChatMessage[]>(`/api/scenarios/${id}/chat`),
  say: (id: string, comment: string) =>
    api.post<TurnResult>(`/api/scenarios/${id}/chat`, { comment }),

  edit: (id: string, patch: Record<string, unknown>) =>
    api.patch<ScenarioFull>(`/api/scenarios/${id}`, patch),
  approve: (id: string) => api.post<ScenarioFull>(`/api/scenarios/${id}/approve`),
  remove: (id: string) => api.del<void>(`/api/scenarios/${id}`),
};

/**
 * Занятие, которым управляет преподаватель.
 *
 * Осторожно с терминами: в ТЗ «занятие» — это то, что ведёт
 * преподаватель, а один отработанный вызов у нас называется
 * `session`. Здесь `lesson` — занятие, `session` — вызов.
 */
export interface LessonBrief {
  id: string;
  title: string;
  groupId: string;
  groupTitle: string;
  mode: 'card112' | 'dds';
  source: 'generated' | 'by_students' | 'mixed';
  typeIds: string[];
  /** Режим ДДС: через сколько секунд службе приходит следующая карточка. */
  cardIntervalSec: number;
  startedAt: string | null;
  finishedAt: string | null;
  members: number;
  items: number;
}

export interface LessonProgress {
  userId: string;
  fio: string;
  started: number;
  graded: number;
  avgScore: number | null;
}

export type LessonFull = LessonBrief & { progress: LessonProgress[] };

export const lessons = {
  list: () => api.get<LessonBrief[]>('/api/lessons'),
  one: (id: string) => api.get<LessonFull>(`/api/lessons/${id}`),
  create: (body: {
    groupId: string; title: string;
    mode: 'card112' | 'dds';
    typeIds?: string[];
    source?: 'generated' | 'by_students' | 'mixed';
    cardIntervalSec?: number;
  }) => api.post<LessonBrief>('/api/lessons', body),
  start: (id: string) => api.post<LessonFull>(`/api/lessons/${id}/start`),
  finish: (id: string) => api.post<LessonFull>(`/api/lessons/${id}/finish`),
  remove: (id: string) => api.del<void>(`/api/lessons/${id}`),
};

/**
 * Прогноз результата и проверка самого прогноза.
 *
 * Критерий ТЗ просит не прогноз, а сравнение прогнозных значений
 * с реальными, поэтому главное здесь — `accuracy`, посчитанная по
 * фактически сделанным прогнозам.
 */
export interface ForecastAccuracy {
  checked: number;
  meanError: number | null;
  hitRate: number | null;
  tolerance: number;
  baselineError: number | null;
}

export interface CheckedForecast {
  sessionId: string;
  fio: string;
  predicted: number;
  actual: number;
  basis: number;
  trained: boolean;
  madeAt: string;
  checkedAt: string;
}

export interface MyForecast {
  sessionId: string;
  predicted: number;
  actual: number | null;
  basis: number;
  trained: boolean;
  features: {
    contributions?: { label: string; points: number }[];
    intercept?: number;
  };
  madeAt: string;
}

export interface SettingRow {
  key: string;
  value: number;
  title: string;
  hint: string;
}

export const forecast = {
  group: (id: string) =>
    api.get<{ accuracy: ForecastAccuracy; checked: CheckedForecast[] }>(
      `/api/forecast/group/${id}`),
  mine: () => api.get<MyForecast[]>('/api/forecast/me'),
  settings: () => api.get<SettingRow[]>('/api/settings'),
  setSetting: (key: string, value: number) =>
    api.patch<SettingRow>(`/api/settings/${key}`, { value }),
};

/**
 * Рекомендации курсанту и проверка того, сработали ли они.
 *
 * Критерий ТЗ — «проверка рекомендаций сервиса и реальных
 * рекомендаций». Рекомендация фиксируется с временем, вердикт ставит
 * первый следующий разбор, где у ошибки была возможность повториться.
 */
export interface Advice {
  id: string;
  text: string;
  field: string;
  kind: 'guessed' | 'wrong' | 'missing' | 'extra';
  label: string;
  occurrences: number;
  windowSize: number;
  madeAt: string;
  /** Разбор, после которого выдана */
  sourceSessionId: string;
  /** Разбор, которым проверена; null — ещё не проверена */
  checkSessionId: string | null;
  worked: boolean | null;
  checkedAt: string | null;
}

export interface AdviceEfficacy {
  checked: number;
  worked: number;
  rate: number | null;
  baselineChecked: number;
  baselineRate: number | null;
  tooFew: boolean;
}

export interface CheckedAdvice {
  fio: string;
  text: string;
  worked: boolean;
  madeAt: string;
  checkedAt: string;
}

export const advice = {
  mine: () => api.get<Advice[]>('/api/advice/me'),
  group: (id: string) =>
    api.get<{ efficacy: AdviceEfficacy; recent: CheckedAdvice[] }>(`/api/advice/group/${id}`),
};

export const sessions = {
  create: (scenarioId: string, mode: 'card112' | 'dds' = 'card112') =>
    api.post<{ session: TrainingSession; card: Card; scenario: ScenarioPublic }>(
      '/api/sessions', { scenarioId, mode }),
  ask: (id: string, factId: string) =>
    api.post<{ answer: string; atSec: number; unlocks: string[] }>(
      `/api/sessions/${id}/ask`, { factId }),
  patchCard: (id: string, patch: Record<string, unknown>) =>
    api.patch<{ card: Card; classify: ClassifyResult }>(`/api/sessions/${id}/card`, patch),
  save: (id: string) =>
    api.post<{ card: Card; grade: Grade }>(`/api/sessions/${id}/save`),
  grade: (id: string) => api.get<Grade>(`/api/sessions/${id}/grade`),
  transcript: (id: string) => api.get<TranscriptLine[]>(`/api/sessions/${id}/transcript`),
  /** Незаконченный вызов курсанта; `undefined` — такого нет. */
  active: () => api.get<SessionState | undefined>('/api/sessions/active'),
  state: (id: string) => api.get<SessionState>(`/api/sessions/${id}`),
};

export const cards = {
  list: (params: { scope?: string; status?: string; q?: string; page?: number; size?: number } = {}) => {
    const qs = new URLSearchParams();
    Object.entries(params).forEach(([k, v]) => {
      if (v !== undefined && v !== '') qs.set(k, String(v));
    });
    return api.get<{ items: CardBrief[]; total: number; page: number; size: number }>(
      `/api/cards${qs.size ? `?${qs}` : ''}`);
  },
  one: (id: string) => api.get<CardDetail>(`/api/cards/${id}`),
  /**
   * Диспетчер открыл карточку своей службы: сервер отмечает «Получена
   * службой» один раз. От этого считается норматив 30 секунд.
   */
  opened: (id: string) => api.post<void>(`/api/cards/${id}/opened`),
  addResponse: (id: string, body: {
    serviceId: string; status: string; orderNo?: string; comment?: string;
  }) => api.post<import('./types').ResponseEntry>(`/api/cards/${id}/responses`, body),

  brigade: (id: string, serviceId?: string) =>
    api.get<Brigade>(`/api/cards/${id}/brigade${serviceId
      ? `?serviceId=${encodeURIComponent(serviceId)}` : ''}`),
  assignBrigade: (id: string, serviceId: string, brigadeNo: number) =>
    api.post<Brigade>(`/api/cards/${id}/brigade`, { serviceId, brigadeNo }),
  /** Диспетчер сам звонит старшему и узнаёт текущий этап. */
  callBrigade: (id: string, serviceId: string) =>
    api.post<{ text: string; learned: string | null; brigade: Brigade }>(
      `/api/cards/${id}/brigade/call`, { serviceId }),
};

export const admin = {
  health: () => api.get<Health>('/api/admin/health'),

  audit: (params: {
    from?: string; to?: string; userId?: string;
    action?: string; entity?: string; entityId?: string;
    page?: number; size?: number;
  } = {}) => {
    const qs = new URLSearchParams();
    Object.entries(params).forEach(([k, v]) => {
      if (v !== undefined && v !== '') qs.set(k, String(v));
    });
    return api.get<AuditPage>(`/api/admin/audit${qs.size ? `?${qs}` : ''}`);
  },

  users: () => api.get<AdminUser[]>('/api/admin/users'),
  createUser: (body: {
    login: string; password: string; fio: string;
    role: string; serviceId?: string | null; workstation?: string | null;
  }) => api.post<AdminUser>('/api/admin/users', body),
  setActive: (id: string, active: boolean) =>
    api.post<void>(`/api/admin/users/${id}/active`, { active }),

  versions: () => api.get<ClassifierVersion[]>('/api/admin/classifier-versions'),
  activateVersion: (id: string) =>
    api.post<void>(`/api/admin/classifier-versions/${id}/activate`),
};

export const reports = {
  groups: () => api.get<GroupBrief[]>('/api/groups'),

  session: (id: string) => api.get<unknown>(`/api/reports/session/${id}`),
  /** Ссылка на выгрузку: скачивание делает браузер, не мы. */
  sessionCsvUrl: (id: string) => `/api/reports/session/${id}?format=csv`,

  group: (id: string) => api.post<{ jobId: string }>(`/api/reports/group/${id}`),
  insights: (id: string) => api.post<{ jobId: string }>(`/api/insights/group/${id}`),
  job: (id: string) => api.get<JobView>(`/api/jobs/${id}`),

  /**
   * Дождаться результата задачи.
   *
   * Отчёт по группе считается в очереди — норматив ТЗ на подготовку
   * 30 секунд, поэтому и предел ожидания вдвое больше: лучше честно
   * сказать «не дождались», чем висеть бесконечно.
   */
  awaitJob: async (id: string, timeoutMs = 60_000): Promise<unknown> => {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const job = await reports.job(id);
      if (job.status === 'done') return job.output;
      if (job.status === 'failed') throw new Error(job.error ?? 'задача не выполнена');
      if (Date.now() > deadline) throw new Error('отчёт не готов, попробуйте ещё раз');
      await new Promise((r) => setTimeout(r, 400));
    }
  },
};
