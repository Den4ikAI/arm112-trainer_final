/** Эндпоинты сервера. Пути — как в backend/src/api. */
import { api, getToken, setToken, ApiError as ClientError } from './client';
import type {
  AdminUser, AuditPage, Brigade, Card, CardBrief, CardDetail, ClassifierVersion,
  ClassifyResult, Duty, Grade, GroupBrief, GroupMember, Health, IncidentType, JobView,
  ScenarioFull, ScenarioPublic, Service, SessionState, SurveyCard, TrainingSession,
  TranscriptLine, TurnResult, User, VoiceRole, VoiceStatus,
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
  /** Тема от преподавателя: «Пожар в жилом доме — вызов 1» */
  title?: string | null;
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
  /** Правка утверждений фактов. Пустая строка — подготовить заново моделью. */
  statements: (id: string, body: { legendStatements?: string[]; facts?: Record<string, string> }) =>
    api.put<ScenarioFull>(`/api/scenarios/${id}/statements`, body),
  prepareStatements: (id: string) => api.post<void>(`/api/scenarios/${id}/statements/prepare`),
  remove: (id: string) => api.del<void>(`/api/scenarios/${id}`),

  /** Все вызовы с темой и статусом — список преподавателя. */
  library: () => api.get<import('./types').LibraryItem[]>('/api/teacher/scenarios'),
  /** Собрать вызов вручную. Эталон посчитает классификатор. */
  createManual: (body: import('./types').NewScenario) =>
    api.post<ScenarioFull>('/api/scenarios/manual', body),
  /** Вызов по описанию: реплики пишет модель, эталон — классификатор. */
  fromPrompt: (title: string, prompt: string) =>
    api.post<ScenarioFull>('/api/scenarios/from-prompt', { title, prompt }),
};

/** Учебные происшествия ДДС — готовые карточки, собранные преподавателем. */
export const ddsIncidents = {
  list: () => api.get<import('./types').Incident[]>('/api/dds-incidents'),
  one: (id: string) => api.get<import('./types').Incident>(`/api/dds-incidents/${id}`),
  create: (body: import('./types').IncidentInput) =>
    api.post<import('./types').Incident>('/api/dds-incidents', body),
  update: (id: string, body: import('./types').IncidentInput) =>
    api.put<import('./types').Incident>(`/api/dds-incidents/${id}`, body),
  approve: (id: string) => api.post<import('./types').Incident>(`/api/dds-incidents/${id}/approve`),
  remove: (id: string) => api.del<void>(`/api/dds-incidents/${id}`),
  fromPrompt: (title: string, prompt: string) =>
    api.post<import('./types').Incident>('/api/dds-incidents/from-prompt', { title, prompt }),
  /** Собрать системой целиком для службы: признаки — классификатор, текст — модель */
  generate: (serviceId: string) =>
    api.post<import('./types').Incident>('/api/dds-incidents/generate', { serviceId }),
  /** Самостоятельная тренировка диспетчера: происшествие от ИИ сразу в ленту */
  practice: () => api.post<import('./types').Practice>('/api/dds-incidents/practice'),
};

/** Сведения о стенде для экрана входа — до входа, без токена. */
export const stand = {
  get: () => api.get<import('./types').Stand>('/api/stand'),
  /**
   * Включить или свернуть систему 112 с экрана входа. Решает
   * администратор: вход его учётной записью, тот же параметр `ui.op112`,
   * что во вкладке «Параметры», и сразу выход. Своим токеном не
   * пользуемся — на экране входа его нет, а чужой сеанс не подменяем.
   */
  setOp112: async (login: string, password: string, on: boolean) => {
    const call = async <T,>(path: string, method: string, body?: unknown, token?: string) => {
      const res = await fetch(path, {
        method,
        headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      if (!res.ok) {
        let data: { error?: string; detail?: string } | null = null;
        try { data = await res.json(); } catch { /* тело не JSON */ }
        throw new ClientError(res.status, data?.error ?? 'unknown', data?.detail);
      }
      return (res.status === 204 ? null : await res.json()) as T;
    };
    const { token } = await call<{ token: string }>('/api/auth/login', 'POST', { login, password });
    try {
      await call('/api/settings/ui.op112', 'PATCH', { value: on ? 1 : 0 }, token);
    } finally {
      await call('/api/auth/logout', 'POST', undefined, token).catch(() => undefined);
    }
  },
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
  /** Обычное занятие, марафон (экзамен на 5–10 вызовов) или блиц (один сегмент). */
  kind: LessonKind;
  /** Сегмент блица. */
  segment: BlitzSegment | null;
  /** Сколько вызовов раздаётся, если они не выбраны поимённо. */
  callCount: number | null;
  /** Вызовы, выбранные преподавателем, по порядку. */
  scenarioIds: string[];
  /** Учебные происшествия ДДС, выбранные преподавателем, по порядку. */
  cardIds: string[];
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

export type LessonKind = 'training' | 'marathon' | 'blitz';
export type BlitzSegment = 'address' | 'type' | 'services' | 'statuses';

/** Подписи сегментов блица — те же, что отдаёт сервер в протоколе. */
export const SEGMENT_LABEL: Record<BlitzSegment, string> = {
  address: 'Адрес',
  type: 'Тип происшествия и опросная карта',
  services: 'Службы',
  statuses: 'Статусы по докладам',
};

/** Вызов в итоговом протоколе занятия. */
export interface ProtocolCall {
  seq: number;
  scenarioId: string;
  ticket: number | null;
  callNo: number | null;
  legend: string;
  sessionId: string | null;
  cardId: string | null;
  finished: boolean;
  /** В блице — по сегменту. Пусто, пока марафон закрывает разбор. */
  score: number | null;
  elapsedSec: number | null;
  overNorm: boolean | null;
  /** Грубые ошибки — подписи полей. */
  gross: string[];
  expertScore: number | null;
}

export interface ProtocolRow {
  userId: string;
  fio: string;
  calls: ProtocolCall[];
  done: number;
  total: number;
  avgScore: number | null;
  totalSec: number;
  grossCount: number;
  /** Баллы закрыты: марафон курсанта ещё идёт. */
  hidden: boolean;
}

export interface Protocol {
  lessonId: string;
  title: string;
  kind: LessonKind;
  mode: 'card112' | 'dds';
  segment: BlitzSegment | null;
  segmentLabel: string | null;
  finished: boolean;
  rows: ProtocolRow[];
}

/** Раздел курсанта: учебные, экзамены, тренировка по частям карточки. */
export type WorkSection = 'training' | 'exam' | 'blitz';
export type WorkStatus = 'not_started' | 'in_progress' | 'done' | 'missed';

/** Выданное курсанту преподавателем: вызов 112 или происшествие ДДС. */
export interface WorkItem {
  id: string;
  lessonId: string;
  lessonTitle: string;
  kind: LessonKind;
  segment: BlitzSegment | null;
  lessonRunning: boolean;
  issuedAt: string | null;
  topic: string;
  detail: string;
  status: WorkStatus;
  score: number | null;
  /** Балл скрыт до конца экзамена. */
  hidden: boolean;
  seq: number | null;
  scenarioId: string | null;
  ticket: number | null;
  callNo: number | null;
  sessionId: string | null;
  cardNumber: string | null;
  cardId: string | null;
}

export const me = {
  work: (section: WorkSection) => api.get<WorkItem[]>(`/api/me/work?section=${section}`),
};

export const lessons = {
  list: () => api.get<LessonBrief[]>('/api/lessons'),
  one: (id: string) => api.get<LessonFull>(`/api/lessons/${id}`),
  create: (body: {
    groupId: string; title: string;
    mode: 'card112' | 'dds';
    typeIds?: string[];
    source?: 'generated' | 'by_students' | 'mixed';
    cardIntervalSec?: number;
    kind?: LessonKind;
    segment?: BlitzSegment;
    callCount?: number;
    scenarioIds?: string[];
    /** Режим ДДС: учебные происшествия, выбранные преподавателем, по порядку */
    cardIds?: string[];
  }) => api.post<LessonBrief>('/api/lessons', body),
  /** Итоговый протокол: преподавателю — все, курсанту — он сам. */
  protocol: (id: string) => api.get<Protocol>(`/api/lessons/${id}/protocol`),
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

/**
 * Рекомендации диспетчеру ДДС: по замечаниям разбора закрытой карточки,
 * проверяются следующей карточкой. По открытой рекомендации курсант
 * берёт следующее задание от ИИ — без преподавателя.
 */
export interface DdsAdvice {
  id: string;
  text: string;
  kind: import('./types').DdsFindingKind;
  stage: string;
  label: string;
  occurrences: number;
  windowSize: number;
  madeAt: string;
  /** Карточка, по разбору которой выдана */
  sourceCardId: string;
  /** Карточка, которой проверена; null — ещё не проверена */
  checkCardId: string | null;
  worked: boolean | null;
  checkedAt: string | null;
  /** Задание от ИИ, выданное для отработки этой рекомендации */
  nextCardId: string | null;
}

export const ddsAdvice = {
  mine: () => api.get<DdsAdvice[]>('/api/advice/dds/me'),
  group: (id: string) =>
    api.get<{ efficacy: AdviceEfficacy; recent: CheckedAdvice[] }>(`/api/advice/dds/group/${id}`),
  next: (id: string) => api.post<import('./types').Practice>(`/api/advice/dds/${id}/next`),
};

/** Попытка внутри карточки: вызов 112 или работа службы ДДС. */
export interface ReviewTarget {
  kind: 'card112' | 'dds';
  serviceId?: string | null;
}

/** Разбор попытки: автоматический балл, анализ текста, экспертная оценка. */
export const review = {
  get: (cardId: string) =>
    api.get<import('./types').CardReview>(`/api/cards/${cardId}/review`),
  setExpert: (cardId: string, target: ReviewTarget, score: number, comment: string) =>
    api.post<import('./types').ExpertGrade>(`/api/cards/${cardId}/review/expert`,
      { ...target, score, comment }),
  setVerdict: (cardId: string, analysisId: string, index: number,
               verdict: 'confirmed' | 'rejected' | null) =>
    api.post<void>(`/api/cards/${cardId}/review/verdict`, { analysisId, index, verdict }),
  analyse: (cardId: string, target: ReviewTarget) =>
    api.post<void>(`/api/cards/${cardId}/review/analyse`, target),
};

/** Доклад диспетчера ДДС вышестоящему начальнику. */
export const chief = {
  get: (cardId: string, serviceId?: string) =>
    api.get<import('./types').ChiefReport | null>(
      `/api/cards/${cardId}/chief${serviceId ? `?serviceId=${encodeURIComponent(serviceId)}` : ''}`),
  start: (cardId: string) =>
    api.post<import('./types').ChiefReport>(`/api/cards/${cardId}/chief/start`, {}),
  say: (cardId: string, text: string) =>
    api.post<import('./types').ChiefReport>(`/api/cards/${cardId}/chief/say`, { text }),
  finish: (cardId: string) =>
    api.post<import('./types').ChiefReport>(`/api/cards/${cardId}/chief/finish`, {}),
};

export const sessions = {
  /** `assignmentId` — какое именно задание начато: один и тот же вызов
   *  бывает выдан сразу в экзамене и в блице. */
  create: (scenarioId: string, mode: 'card112' | 'dds' = 'card112', assignmentId?: string) =>
    api.post<{ session: TrainingSession; card: Card; scenario: ScenarioPublic }>(
      '/api/sessions', { scenarioId, mode, assignmentId }),
  ask: (id: string, factId: string) =>
    api.post<{ answer: string; atSec: number; unlocks: string[] }>(
      `/api/sessions/${id}/ask`, { factId }),
  /** Свободный вопрос своими словами — напечатанный или распознанный. */
  askFree: (id: string, text: string) =>
    api.post<import('./types').AskFreeResult>(`/api/sessions/${id}/ask-free`, { text }),
  patchCard: (id: string, patch: Record<string, unknown>) =>
    api.patch<{ card: Card; classify: ClassifyResult }>(`/api/sessions/${id}/card`, patch),
  /** `grade` пуст, пока разбор закрывает марафон. */
  save: (id: string) =>
    api.post<{ card: Card; grade: Grade | null }>(`/api/sessions/${id}/save`),
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
  /** `speaker` — кто на линии: начальник (старший) бригады или её специалист. */
  callBrigade: (id: string, serviceId: string, said?: string, speaker?: 'head' | 'specialist') =>
    api.post<{ text: string; learned: string | null; brigade: Brigade }>(
      `/api/cards/${id}/brigade/call`, { serviceId, said, speaker }),
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
    role: string; serviceId?: string | null; duty?: Duty | null; workstation?: string | null;
  }) => api.post<AdminUser>('/api/admin/users', body),
  /**
   * Направить курсанта на другое рабочее место. Администратор — любого,
   * преподаватель — своего. Действует сразу, без перевхода.
   */
  setDuty: (id: string, duty: Duty, serviceId: string | null) =>
    api.patch<AdminUser>(`/api/users/${id}/duty`, { duty, serviceId }),
  groupMembers: (groupId: string) => api.get<GroupMember[]>(`/api/groups/${groupId}/members`),
  setActive: (id: string, active: boolean) =>
    api.post<void>(`/api/admin/users/${id}/active`, { active }),
  /**
   * Удалить учётную запись. Без истории — стирается; с историей —
   * резервная копия и обезличивание.
   */
  deleteUser: (id: string) =>
    api.del<{ mode: 'deleted' | 'anonymized'; backupId: string | null }>(`/api/admin/users/${id}`),

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

/**
 * Запрос с телом не в JSON: аудио туда и обратно. Общий клиент умеет
 * только JSON, а звук ему незачем пропускать через JSON.parse.
 */
const rawRequest = async (path: string, init: RequestInit): Promise<Response> => {
  const token = getToken();
  const res = await fetch(path, {
    ...init,
    headers: { ...(init.headers ?? {}), ...(token ? { Authorization: `Bearer ${token}` } : {}) },
  });
  if (!res.ok) {
    if (res.status === 401) setToken(null);
    let data: { error?: string; detail?: string } | null = null;
    try { data = await res.json(); } catch { /* тело не JSON */ }
    throw new ClientError(res.status, data?.error ?? 'unknown', data?.detail);
  }
  return res;
};

/**
 * IP-телефон диспетчера ДДС. Разговор ведут прежние эндпоинты бригады и
 * начальника; здесь — вызовы, журнал и запись разговора.
 */
export const phone = {
  config: () => api.get<import('./types').PhoneConfig>('/api/phone/config'),
  view: (cardId: string, serviceId?: string) =>
    api.get<import('./types').PhoneView>(`/api/cards/${cardId}/phone${serviceId
      ? `?serviceId=${encodeURIComponent(serviceId)}` : ''}`),
  dial: (cardId: string, peer: import('./types').PhonePeer, direction: 'in' | 'out') =>
    api.post<import('./types').PhoneCall>(`/api/cards/${cardId}/phone/calls`, { peer, direction }),
  answer: (callId: string) =>
    api.post<import('./types').PhoneCall>(`/api/phone/calls/${callId}/answer`),
  /** `declined` — сбросить звонящий вызов; без него — пропущен или окончен. */
  end: (callId: string, outcome?: 'declined') =>
    api.post<import('./types').PhoneCall>(`/api/phone/calls/${callId}/end`, outcome ? { outcome } : {}),
  line: (callId: string, who: 'dispatcher' | 'peer', text: string) =>
    api.post<{ id: number }>(`/api/phone/calls/${callId}/lines`, { who, text }),
  /** Голос диспетчера к реплике — для записи разговора. */
  lineAudio: async (callId: string, lineId: number, wav: Blob) => {
    await rawRequest(`/api/phone/calls/${callId}/lines/${lineId}/audio`,
      { method: 'PUT', headers: { 'Content-Type': 'audio/wav' }, body: wav });
  },
  /** Запись разговора, WAV. */
  recording: async (callId: string) =>
    (await rawRequest(`/api/phone/calls/${callId}/recording`, { method: 'GET' })).blob(),
  /** Реплика руководителя организации или заявителя: без текста —
   *  абонент снял трубку. Сервер сам пишет её в журнал разговора. */
  reply: (callId: string, text?: string) =>
    api.post<import('./types').PhoneReply>(`/api/phone/calls/${callId}/reply`, text ? { text } : {}),
};

/**
 * Телефонный справочник ДДС: формирует преподаватель, не более 10
 * номеров на службу (рекомендации финалистам, п. 8).
 */
export const phonebook = {
  view: (serviceId?: string) =>
    api.get<import('./types').PhoneBook>(`/api/phonebook${serviceId
      ? `?serviceId=${encodeURIComponent(serviceId)}` : ''}`),
  save: (serviceId: string, entries: import('./types').PhoneBookEntry[]) =>
    api.put<import('./types').PhoneBook>(`/api/phonebook?serviceId=${encodeURIComponent(serviceId)}`, { entries }),
  reset: (serviceId: string) =>
    api.del<import('./types').PhoneBook>(`/api/phonebook?serviceId=${encodeURIComponent(serviceId)}`),
};

/**
 * Голос. Не подключён — `status` честно говорит «нет», остальные
 * вызовы отвечают 503, и разговор идёт текстом.
 */
export const voice = {
  status: () => api.get<VoiceStatus>('/api/voice/status'),
  /** Распознать реплику, WAV. */
  recognize: async (wav: Blob) => {
    const res = await rawRequest('/api/voice/asr',
      { method: 'POST', headers: { 'Content-Type': 'audio/wav' }, body: wav });
    return (await res.json()) as { text: string; durationMs: number };
  },
  /** Озвучить реплику собеседника. Ответ — WAV. */
  speak: async (text: string, role: VoiceRole) => {
    const res = await rawRequest('/api/voice/tts', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text, voice: role }),
    });
    return res.blob();
  },
};

