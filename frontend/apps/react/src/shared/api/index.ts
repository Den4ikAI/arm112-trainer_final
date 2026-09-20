/** Эндпоинты сервера. Пути — как в backend/src/api. */
import { api, setToken } from './client';
import type {
  AdminUser, AuditPage, Card, CardBrief, CardDetail, ClassifierVersion,
  ClassifyResult, Grade, GroupBrief, Health, IncidentType, JobView,
  ScenarioPublic, Service, SurveyCard, TrainingSession, TranscriptLine, User,
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
}

export const scenarios = {
  list: () => api.get<ScenarioBrief[]>('/api/scenarios'),
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
};

export const cards = {
  list: (params: { scope?: string; status?: string; q?: string; page?: number } = {}) => {
    const qs = new URLSearchParams();
    Object.entries(params).forEach(([k, v]) => {
      if (v !== undefined && v !== '') qs.set(k, String(v));
    });
    return api.get<{ items: CardBrief[]; total: number; page: number; size: number }>(
      `/api/cards${qs.size ? `?${qs}` : ''}`);
  },
  one: (id: string) => api.get<CardDetail>(`/api/cards/${id}`),
  addResponse: (id: string, body: {
    serviceId: string; status: string; orderNo?: string; comment?: string;
  }) => api.post<import('./types').ResponseEntry>(`/api/cards/${id}/responses`, body),
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
