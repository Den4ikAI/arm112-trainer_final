/**
 * Занятие обучающегося.
 *
 * Карточка, список оповещения и оценка живут на сервере. Здесь только
 * то, что нужно нарисовать, плюс черновик полей, которые курсант
 * сейчас печатает.
 *
 * Почему не считаем ничего на клиенте: список оповещения даёт
 * настоящий классификатор (1283 позиции, 22 484 строки матрицы), а
 * оценка сравнивает три слоя, из которых два — стенограмма и скрытый
 * эталон — на клиент вообще не приезжают. Иначе метрику «выяснил или
 * угадал» можно было бы посмотреть в DevTools.
 */
import { create } from 'zustand';
import {
  sessions, ApiError,
  type Card, type ClassifyResult, type Grade, type ScenarioPublic, type SessionState,
} from '@/shared/api';

/** Пауза перед отправкой правок. Он же буфер на случай обрыва сети. */
const DEBOUNCE_MS = 500;
/** Через сколько повторить отправку, если сеть отвалилась. */
const RETRY_MS = 3000;

/**
 * Где помним текущий вызов между обновлениями страницы.
 *
 * sessionStorage, а не localStorage: он живёт, пока открыта вкладка,
 * и переживает F5 — ровно то, что нужно. Сам вызов хранит сервер,
 * здесь только его номер; без номера вызов находится запросом
 * «незаконченный вызов курсанта».
 */
const STORAGE_KEY = 'arm112.sessionId';
const remember = (id: string | null) => {
  try {
    if (id) sessionStorage.setItem(STORAGE_KEY, id);
    else sessionStorage.removeItem(STORAGE_KEY);
  } catch { /* приватный режим: продолжение найдётся запросом к серверу */ }
};
const remembered = (): string | null => {
  try { return sessionStorage.getItem(STORAGE_KEY); } catch { return null; }
};

interface TrainingState {
  sessionId: string | null;
  card: Card | null;
  scenario: ScenarioPublic | null;
  classify: ClassifyResult | null;
  /** Реплики: легенда, вопросы курсанта и ответы заявителя. */
  transcript: Array<{ who: string; text: string; atSec: number }>;
  revealed: string[];
  grade: Grade | null;
  /**
   * Карточка сохранена, а разбор закрыт: идёт марафон, и разбор
   * откроется после последнего вызова. Экран уходит в журнал к
   * следующему вызову, а не на разбор.
   */
  closed: boolean;
  /** Когда вызов начат — от этого момента идёт таймер, и после F5 тоже. */
  startedAt: string | null;
  saving: boolean;
  error: string | null;
}

interface Actions {
  start(scenarioId: string): Promise<void>;
  /**
   * Продолжить вызов после обновления страницы, повторного входа или
   * в новой вкладке. `number` — номер карточки из адреса страницы:
   * чужой по номеру вызов не подставляем. `false` — продолжать нечего.
   */
  resume(number?: string): Promise<boolean>;
  /** Локальная правка + отложенная отправка. */
  edit(patch: Partial<Card>): void;
  editAddress(field: string, value: string): void;
  toggleType(typeId: string): void;
  setAnswer(questionId: string, value: string[] | string): void;
  setManualServices(ids: string[]): void;
  ask(factId: string): Promise<void>;
  /**
   * Вопрос своими словами. Возвращает ответ заявителя — его озвучивают;
   * `null`, если занятия нет.
   */
  askFree(text: string): Promise<import('@/shared/api').AskFreeResult | null>;
  save(): Promise<void>;
  /** Немедленно дослать отложенное — перед сохранением и уходом со страницы. */
  flush(): Promise<void>;
  reset(): void;
}

let timer: ReturnType<typeof setTimeout> | null = null;
let pending: Record<string, unknown> = {};

export const useTraining = create<TrainingState & Actions>((set, get) => {
  const send = async () => {
    const { sessionId } = get();
    if (!sessionId || Object.keys(pending).length === 0) return;
    const body = pending;
    pending = {};
    try {
      const r = await sessions.patchCard(sessionId, body);
      set({ card: r.card, classify: r.classify, error: null });
    } catch (e) {
      // Сервер отказал осмысленно (занятие завершено, нет прав) —
      // повтор даст тот же ответ, держать правки незачем
      if (e instanceof ApiError && e.status < 500) {
        set({ error: e.message });
        return;
      }

      // Сеть отвалилась. Правки не теряем: возвращаем в очередь.
      // Порядок важен — то, что курсант набрал позже, должно победить
      const restored: Record<string, unknown> = { ...body, ...pending };
      for (const key of ['answers', 'address'] as const) {
        const older = body[key] as Record<string, unknown> | undefined;
        const newer = pending[key] as Record<string, unknown> | undefined;
        if (older && newer) restored[key] = { ...older, ...newer };
      }
      pending = restored;
      set({ error: 'нет связи с сервером, правки не потеряны' });

      // Сам себя не дошлёт: курсант мог больше ничего не нажать
      if (timer) clearTimeout(timer);
      timer = setTimeout(send, RETRY_MS);
    }
  };

  // Сеть вернулась — досылаем сразу, а не через паузу повтора
  if (typeof window !== 'undefined') {
    window.addEventListener('online', () => {
      if (Object.keys(pending).length === 0) return;
      if (timer) clearTimeout(timer);
      void send();
    });
  }

  /**
   * Копить правки до отправки.
   *
   * `answers` и `address` сливаются по полям, а не заменяются целиком:
   * курсант успевает нажать несколько плашек за одно окно дебаунса, и
   * простое присваивание оставило бы от них только последнюю.
   */
  const queue = (patch: Record<string, unknown>) => {
    const merged: Record<string, unknown> = { ...pending, ...patch };
    for (const key of ['answers', 'address'] as const) {
      const prev = pending[key] as Record<string, unknown> | undefined;
      const next = patch[key] as Record<string, unknown> | undefined;
      if (prev && next) merged[key] = { ...prev, ...next };
    }
    pending = merged;
    if (timer) clearTimeout(timer);
    timer = setTimeout(send, DEBOUNCE_MS);
  };

  /** Разложить пришедшее с сервера состояние вызова по экрану. */
  const restore = (st: SessionState) => {
    pending = {};
    if (timer) clearTimeout(timer);
    remember(st.session.id);
    set({
      sessionId: st.session.id,
      card: st.card,
      scenario: st.scenario,
      classify: st.classify,
      transcript: st.transcript,
      revealed: st.scenario.questions.filter((q) => q.revealed).map((q) => q.factId),
      grade: st.grade,
      // Сохранённый вызов без разбора — марафон: продолжать нечего
      closed: st.session.finishedAt != null && st.grade == null,
      startedAt: st.session.startedAt,
      error: null,
    });
  };

  return {
    sessionId: null,
    card: null,
    scenario: null,
    classify: null,
    transcript: [],
    revealed: [],
    grade: null,
    closed: false,
    startedAt: null,
    saving: false,
    error: null,

    // Новая попытка — всегда новая карточка. Продолжение незаконченного
    // вызова — отдельное действие: журнал предлагает его сам, и F5 на
    // карточке его восстанавливает (`resume`)
    start: async (scenarioId) => {
      const r = await sessions.create(scenarioId);
      pending = {};
      if (timer) clearTimeout(timer);
      remember(r.session.id);
      set({
        sessionId: r.session.id,
        card: r.card,
        scenario: r.scenario,
        classify: null,
        // Первую фразу заявитель говорит сам, без вопроса
        transcript: [{ who: 'заявитель', text: r.scenario.legend, atSec: 0 }],
        revealed: [],
        grade: null,
        closed: false,
        startedAt: r.session.startedAt,
        error: null,
      });
    },

    resume: async (number) => {
      // Сначала вызов этой вкладки — он может быть и сохранённым,
      // тогда вернётся разбор. Не подошёл — незаконченный вызов курсанта
      const fits = (st: SessionState | undefined) =>
        st && (!number || st.card.number === number) ? st : undefined;
      const id = remembered();
      const st = fits(id ? await sessions.state(id).catch(() => undefined) : undefined)
        ?? fits(await sessions.active().catch(() => undefined));
      if (!st) return false;
      restore(st);
      return true;
    },

    edit: (patch) => {
      set((st) => (st.card ? { card: { ...st.card, ...patch } } : {}));
      queue(patch as Record<string, unknown>);
    },

    editAddress: (field, value) => {
      set((st) => (st.card
        ? { card: { ...st.card, address: { ...st.card.address, [field]: value } } }
        : {}));
      // Шлём только изменившееся поле: сервер дописывает его к адресу
      queue({ address: { [field]: value } });
    },

    toggleType: (typeId) => {
      const card = get().card;
      if (!card) return;
      const typeIds = card.typeIds.includes(typeId)
        ? card.typeIds.filter((t) => t !== typeId)
        : [...card.typeIds, typeId];
      set({ card: { ...card, typeIds } });
      queue({ typeIds });
    },

    setAnswer: (questionId, value) => {
      const card = get().card;
      if (!card) return;
      set({ card: { ...card, answers: { ...card.answers, [questionId]: value } } });
      queue({ answers: { [questionId]: value } });
    },

    // Автоподобранную службу удалить нельзя — это решает сервер,
    // сюда приходят только добавленные руками
    setManualServices: (ids) => queue({ manualServiceIds: ids }),

    ask: async (factId) => {
      const { sessionId, revealed } = get();
      if (!sessionId || revealed.includes(factId)) return;
      const question = get().scenario?.questions.find((q) => q.factId === factId);
      const r = await sessions.ask(sessionId, factId);
      set((st) => ({
        revealed: [...st.revealed, factId],
        transcript: [
          ...st.transcript,
          ...(question ? [{ who: 'оператор', text: question.question, atSec: r.atSec }] : []),
          { who: 'заявитель', text: r.answer, atSec: r.atSec },
        ],
      }));
    },

    askFree: async (text) => {
      const { sessionId } = get();
      const question = text.trim();
      if (!sessionId || !question) return null;
      const r = await sessions.askFree(sessionId, question);
      set((st) => ({
        // Понятый вопрос засчитывается, как нажатая кнопка
        revealed: r.factId && !st.revealed.includes(r.factId)
          ? [...st.revealed, r.factId] : st.revealed,
        transcript: [
          ...st.transcript,
          { who: 'оператор', text: question, atSec: r.atSec },
          { who: 'заявитель', text: r.answer, atSec: r.atSec },
        ],
      }));
      return r;
    },

    flush: async () => {
      if (timer) clearTimeout(timer);
      await send();
    },

    save: async () => {
      const { sessionId } = get();
      if (!sessionId) return;
      set({ saving: true });
      try {
        await get().flush();
        const r = await sessions.save(sessionId);
        set({ card: r.card, grade: r.grade, closed: r.grade == null, saving: false });
      } catch (e) {
        set({ saving: false, error: e instanceof ApiError ? e.message : 'не удалось сохранить' });
        throw e;
      }
    },

    reset: () => {
      if (timer) clearTimeout(timer);
      pending = {};
      remember(null);
      set({
        sessionId: null, card: null, scenario: null, classify: null,
        transcript: [], revealed: [], grade: null, closed: false, startedAt: null,
        saving: false, error: null,
      });
    },
  };
});
