/** Эндпоинты сервера. Пути — как в backend/src/api. */
import { api, setToken } from './client';
import type {
  Card, CardBrief, CardDetail, ClassifyResult, Grade, IncidentType,
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
