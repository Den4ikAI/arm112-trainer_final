/**
 * Формы ответов сервера. Списаны с обработчиков в backend/src/api,
 * сериализация camelCase.
 */

export type Role = 'student' | 'teacher' | 'admin';

/**
 * Профиль рабочего места обучающегося: оператор 112 принимает вызовы и
 * заполняет карточки, диспетчер ДДС работает с карточками своей службы.
 * У преподавателя и администратора профиля нет.
 */
export type Duty = 'op112' | 'dds';

export interface User {
  id: string;
  login: string;
  fio: string;
  role: Role;
  serviceId: string | null;
  duty: Duty | null;
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
  /** Как в окне «Добавьте службы» боевой системы, если снято с кадров. */
  title: string;
  /** Как на плашке в полосе оповещения: «Служба 101». */
  shortTitle: string;
  /** Заголовок столбца классификатора: «Классификатор МЧС». */
  sourceTitle: string | null;
  /** Дополнительные слова для поиска: «скорая», «02». */
  aliases: string[];
}

export interface SurveyQuestion {
  id: string;
  label: string;
  kind: 'single' | 'multi' | 'text' | 'number';
  options: string[];
  /**
   * Вопрос, от ответа на который зависят варианты этого. У карт,
   * выведенных из классификатора, второй признак зависит от первого.
   */
  dependsOn: string | null;
  /** Вариант → при каких ответах на `dependsOn` он показывается. */
  optionParents: Record<string, string[]>;
  /**
   * Когда показывать сам вопрос: хотя бы одно условие выполнено —
   * вопрос виден. Пустой массив — виден всегда. Карта растёт по мере
   * ответов, как на кадрах: «Правонарушение» появляется после объекта.
   */
  showWhen: ShowWhen[];
}

export interface ShowWhen {
  question: string;
  values: string[];
}

export interface SurveyCard {
  typeId: string;
  title: string;
  questions: SurveyQuestion[];
  /** Со скриншота боевой системы или выведена из классификатора. */
  source: 'screenshot' | 'classifier';
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

/** Вызов целиком — чтобы продолжить его после обновления страницы. */
export interface SessionState {
  session: TrainingSession;
  card: Card;
  scenario: ScenarioPublic;
  transcript: TranscriptLine[];
  classify: ClassifyResult | null;
  grade: Grade | null;
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
  /** Сегмент блица: разбор ограничен им, и балл — по нему. */
  segment?: 'address' | 'type' | 'services' | 'statuses';
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
  workstation: string | null;
  /** Пострадавшие, как их отметил оператор; null — отметки нет. */
  victims: number | null;
  /**
   * Когда карточка поступила в ленту: в занятии ДДС — выпуск потоком
   * занятия, иначе — сохранение оператором 112.
   */
  arrivedAt: string | null;
  /** Секунды с поступления. Нормативы: 30 с на открытие, 3 мин на первую запись. */
  waitingSec: number | null;
  /** Своя служба открыла карточку. `null` — службы нет в списке оповещения. */
  opened: boolean | null;
  /** Своя служба сделала первую запись — статус. */
  recorded: boolean | null;
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

// ─── Администрирование ──────────────────────────────────────────────

export interface AuditEntry {
  id: number;
  at: string;
  userId: string | null;
  fio: string | null;
  action: string;
  entity: string | null;
  entityId: string | null;
  payload: Record<string, unknown> | null;
  ip: string | null;
}

export interface AuditPage {
  items: AuditEntry[];
  total: number;
  page: number;
  size: number;
  /** Какие виды действий встречаются — для фильтра. */
  actions: string[];
}

export interface AdminUser {
  id: string;
  login: string;
  fio: string;
  role: Role;
  serviceId: string | null;
  serviceTitle: string | null;
  duty: Duty | null;
  workstation: string | null;
  isActive: boolean;
  createdAt: string;
  sessions: number;
}

/** Курсант группы с его профилем — для преподавателя. */
export interface GroupMember {
  id: string;
  login: string;
  fio: string;
  duty: Duty | null;
  serviceId: string | null;
  serviceTitle: string | null;
}

export interface ClassifierVersion {
  id: string;
  name: string;
  sourceFile: string;
  positionsCount: number;
  isActive: boolean;
  importedAt: string;
}

export interface Health {
  db: boolean;
  uptimeSec: number;
  mlProvider: string;
  wsListeners: number;
  jobsQueued: number;
  jobsRunning: number;
  version: string;
}

// ─── Аналитика по группе ────────────────────────────────────────────

export interface GroupBrief {
  id: string;
  title: string;
  teacherFio: string;
  members: number;
}

export interface Problem {
  label: string;
  kind: string;
  count: number;
}

export interface StudentRow {
  userId: string;
  fio: string;
  sessions: number;
  avgScore: number | null;
  overNorm: number;
  guessed: number;
}

export interface GroupStats {
  groupId: string;
  groupTitle: string;
  members: number;
  sessions: number;
  avgScore: number | null;
  avgElapsedSec: number | null;
  avgNormSec: number | null;
  overNorm: number;
  overNormShare: number;
  guessed: number;
  ok: number;
  /** Доля угаданного среди совпавшего с эталоном. */
  guessedShare: number;
  topProblems: Problem[];
  students: StudentRow[];
  heatmap: Array<{ label: string; kind: string; count: number }>;
  timeline: Array<{
    at: string; fio: string; elapsedSec: number;
    normSec: number; score: number; overNorm: boolean;
  }>;
  funnel: { learned: number; guessed: number; wrong: number; missing: number };
  byKind: Array<{ kind: string; count: number }>;
}

export interface JobView {
  id: string;
  kind: string;
  status: 'queued' | 'running' | 'done' | 'failed';
  createdAt: string;
  finishedAt: string | null;
  output?: unknown;
  error?: string;
}

// ─── Подготовка учебных вызовов ─────────────────────────────────────

export interface HiddenRow {
  key: string;
  question: string;
  answer: string;
  unlocks: string[];
  /** Ответ утверждением — для сверки описания кросс-энкодером. null — ещё не подготовлен. */
  statement?: string | null;
}

export interface ScenarioFull {
  id: string;
  ticket: number | null;
  callNo: number | null;
  legend: string;
  applicantName: string;
  applicantPhone: string;
  applicantStatus: string;
  normSec: number;
  status: 'draft' | 'approved' | 'archived';
  /** Откуда взялся: из билета моделью, руками, засеян миграцией. */
  source: string;
  expected: {
    typeIds?: string[];
    answers?: Record<string, string[]>;
    services?: string[];
    outcome?: string | null;
    address?: Record<string, string>;
  };
  /** Чего не хватило классификатору. Непустое — утверждать рано. */
  classifyReason: string | null;
  hidden: HiddenRow[];
  /** Первая фраза утверждениями, по одному на сведение. null — ещё не подготовлены. */
  legendStatements?: string[] | null;
}

export interface ChatMessage {
  id: number;
  role: 'teacher' | 'model' | 'system';
  content: string;
  patch: Record<string, unknown> | null;
  at: string;
}

export interface TurnResult {
  reply: string;
  applied: boolean;
  scenario: ScenarioFull;
}

/**
 * Бригада диспетчера ДДС. Бригады учебные — «Бригада 1», «Бригада 2»:
 * настоящих составов служб у нас нет.
 */
export interface BrigadeReport {
  stage: string;
  text: string;
  /** Когда этап наступил на самом деле. Курсанту не приходит. */
  dueAt?: string | null;
  deliveredAt: string | null;
  /** incoming — старший позвонил сам, outgoing — диспетчер позвонил ему. */
  channel: 'incoming' | 'outgoing' | null;
}

export type DdsFindingKind = 'ok' | 'late' | 'notByFact' | 'missed' | 'noBrigade'
  // Приём карточки: нормативы из ответа заказчика в чате 27.09
  | 'openedLate' | 'firstRecordLate' | 'firstRecordNoText';

export interface DdsFinding {
  kind: DdsFindingKind;
  stage: string;
  knownAt: string | null;
  statusAt: string | null;
  lagSec: number | null;
  consequence: string | null;
}

export interface Brigade {
  serviceId: string;
  brigadeCount: number;
  canAssign: boolean;
  assignment: { brigadeNo: number; assignedAt: string } | null;
  reports: BrigadeReport[];
  /** `said` — что диспетчер сказал старшему голосом; без голоса нет. */
  calls: { at: string; learned: string | null; said: string | null }[];
  debrief: { findings: DdsFinding[]; score: number | null; pending: number };
}

// ─── Разбор попытки: нейросетевой анализ текста и экспертная оценка ──

export type TextFindingKind =
  | 'missing' | 'contradiction' | 'invented'   // оператор 112
  | 'no_reason' | 'inconsistent';              // диспетчер ДДС

export interface ReviewedFinding {
  kind: TextFindingKind;
  text: string;
  /** Фрагмент текста курсанта, дословно */
  quoteInput: string | null;
  /** Фрагмент того, что курсант слышал: реплика заявителя, доклад */
  quoteSource: string | null;
  verdict: 'confirmed' | 'rejected' | null;
}

export interface TextAnalysis {
  id: string;
  status: 'queued' | 'ready' | 'unavailable' | 'failed';
  findings: ReviewedFinding[];
  /** Сколько замечаний модели отброшено: цитата не нашлась */
  dropped: number;
  note: string | null;
  modelMs: number | null;
  /** Чем разобрано: 'nli' — кросс-энкодер OpenJEV, 'llm' — языковая модель. */
  engine?: 'nli' | 'llm' | null;
  finishedAt: string | null;
}

export interface ExpertGrade {
  score: number;
  comment: string;
  teacher: string;
  updatedAt: string;
}

export interface Attempt {
  kind: 'card112' | 'dds';
  serviceId: string | null;
  student: { id: string; fio: string } | null;
  autoScore: number | null;
  analysis: TextAnalysis | null;
  expert: ExpertGrade | null;
  /** Доклад начальнику — только у попыток ДДС, последний звонок. */
  chief?: ChiefReport | null;
}

/** Реплика в звонке начальнику. */
export interface ChiefLine {
  who: 'dispatcher' | 'chief';
  text: string;
  /** О каком сведении спросил начальник; нет — приветствие или «Принял». */
  topic: string | null;
  modelMs: number | null;
  at: string;
}

/** Сведение доклада: что в карточке и как доложено. */
export interface ChiefItem {
  key: string;
  title: string;
  expected: string;
  status: 'reported' | 'missing' | 'wrong';
  quote: string | null;
}

export interface ChiefReviewResult {
  items: ChiefItem[];
  /** model — доложенное отмечала модель, rules — ключевые слова. */
  mode: 'model' | 'rules';
  dropped: number;
  reported: number;
  total: number;
  note: string | null;
  /** Разбор пока по ключевым словам — модель его ещё уточняет. */
  pending: boolean;
  modelMs: number | null;
}

/** Звонок диспетчера ДДС вышестоящему начальнику. */
export interface ChiefReport {
  id: string;
  serviceId: string;
  student: string;
  startedAt: string;
  finishedAt: string | null;
  lines: ChiefLine[];
  review: ChiefReviewResult | null;
  maxQuestions: number;
}

export interface CardReview {
  attempts: Attempt[];
  canGrade: boolean;
}

/** Что из голоса подключено на сервере. */
export interface VoiceStatus {
  asr: boolean;
  tts: boolean;
  voices: string[];
}

/**
 * Собеседник: старший бригады, вышестоящий начальник (у ДДС) или
 * заявитель — мужчина или женщина (у оператора 112).
 */
export type VoiceRole = 'brigade' | 'chief' | 'applicant_m' | 'applicant_f';

/** Ответ заявителя на свободный вопрос. */
export interface AskFreeResult {
  answer: string;
  atSec: number;
  /** Про какой факт вопрос; `null` — не понят, заявитель переспросил */
  factId: string | null;
  unlocks: string[];
}

