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
  type Card, type ClassifyResult, type Grade, type ScenarioPublic,
} from '@/shared/api';

/** Пауза перед отправкой правок. Он же буфер на случай обрыва сети. */
const DEBOUNCE_MS = 500;
/** Через сколько повторить отправку, если сеть отвалилась. */
const RETRY_MS = 3000;

interface TrainingState {
  sessionId: string | null;
  card: Card | null;
  scenario: ScenarioPublic | null;
  classify: ClassifyResult | null;
  /** Реплики: легенда, вопросы курсанта и ответы заявителя. */
  transcript: Array<{ who: string; text: string; atSec: number }>;
  revealed: string[];
  grade: Grade | null;
  saving: boolean;
  error: string | null;
}

interface Actions {
  start(scenarioId: string): Promise<void>;
  /** Локальная правка + отложенная отправка. */
  edit(patch: Partial<Card>): void;
  editAddress(field: string, value: string): void;
  toggleType(typeId: string): void;
  setAnswer(questionId: string, value: string[] | string): void;
  setManualServices(ids: string[]): void;
  ask(factId: string): Promise<void>;
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

  return {
    sessionId: null,
    card: null,
    scenario: null,
    classify: null,
    transcript: [],
    revealed: [],
    grade: null,
    saving: false,
    error: null,

    start: async (scenarioId) => {
      const r = await sessions.create(scenarioId);
      pending = {};
      if (timer) clearTimeout(timer);
      set({
        sessionId: r.session.id,
        card: r.card,
        scenario: r.scenario,
        classify: null,
        // Первую фразу заявитель говорит сам, без вопроса
        transcript: [{ who: 'заявитель', text: r.scenario.legend, atSec: 0 }],
        revealed: [],
        grade: null,
        error: null,
      });
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
        set({ card: r.card, grade: r.grade, saving: false });
      } catch (e) {
        set({ saving: false, error: e instanceof ApiError ? e.message : 'не удалось сохранить' });
        throw e;
      }
    },

    reset: () => {
      if (timer) clearTimeout(timer);
      pending = {};
      set({
        sessionId: null, card: null, scenario: null, classify: null,
        transcript: [], revealed: [], grade: null, saving: false, error: null,
      });
    },
  };
});
