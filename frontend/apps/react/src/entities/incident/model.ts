import { create } from 'zustand';
import {
  emptyCard, classify, grade, getScenario,
  type IncidentCard, type Scenario, type Grade, type ResponseEntry,
} from '@arm112/domain';

interface TrainingState {
  /** Текущая карточка. */
  card: IncidentCard;
  /** Активный учебный сценарий (null — свободный режим). */
  scenario: Scenario | null;
  /** id скрытых фактов, которые курсант выяснил у заявителя. */
  revealed: string[];
  /** Лента разговора для разбора. */
  transcript: Array<{ who: 'оператор' | 'заявитель'; text: string; at: number }>;
  startedAt: number;
  saved: boolean;
  result: Grade | null;
}

interface Actions {
  startScenario(scenarioId: string, cardNumber: string): void;
  patch(patch: Partial<IncidentCard>): void;
  patchAddress(field: string, value: string): void;
  toggleType(typeId: string): void;
  setAnswer(questionId: string, value: string[] | string): void;
  setServices(ids: string[]): void;
  /** Автоподбор служб по классификатору — как в боевой системе. */
  recalcServices(): void;
  ask(factId: string): void;
  addResponse(serviceId: string, entry: ResponseEntry): void;
  save(): void;
  reset(): void;
}

export const useTraining = create<TrainingState & Actions>((set, get) => ({
  card: emptyCard('36814851'),
  scenario: null,
  revealed: [],
  transcript: [],
  startedAt: Date.now(),
  saved: false,
  result: null,

  startScenario: (scenarioId, cardNumber) => {
    const scenario = getScenario(scenarioId) ?? null;
    set({
      scenario,
      card: emptyCard(cardNumber),
      revealed: [],
      transcript: scenario
        ? [{ who: 'заявитель', text: scenario.legend, at: 0 }]
        : [],
      startedAt: Date.now(),
      saved: false,
      result: null,
    });
  },

  patch: (patch) => set((st) => ({ card: { ...st.card, ...patch } })),

  patchAddress: (field, value) =>
    set((st) => ({ card: { ...st.card, address: { ...st.card.address, [field]: value } } })),

  toggleType: (typeId) => {
    const { card } = get();
    const typeIds = card.typeIds.includes(typeId)
      ? card.typeIds.filter((t) => t !== typeId)
      : [...card.typeIds, typeId];
    set({ card: { ...card, typeIds } });
    get().recalcServices();
  },

  setAnswer: (questionId, value) => {
    set((st) => ({ card: { ...st.card, answers: { ...st.card.answers, [questionId]: value } } }));
    get().recalcServices();
  },

  setServices: (ids) => set((st) => ({ card: { ...st.card, serviceIds: ids } })),

  recalcServices: () => {
    const { card } = get();
    const auto = classify(card).services;
    // Автоподобранные службы добавляются, вручную добавленные сохраняются.
    const merged = [...new Set([...auto, ...card.serviceIds])];
    set({ card: { ...card, serviceIds: merged } });
  },

  ask: (factId) => {
    const { scenario, revealed, transcript } = get();
    if (!scenario || revealed.includes(factId)) return;
    const fact = scenario.hidden.find((h) => h.id === factId);
    if (!fact) return;
    const at = Math.floor((Date.now() - get().startedAt) / 1000);
    set({
      revealed: [...revealed, factId],
      transcript: [
        ...transcript,
        { who: 'оператор', text: fact.question, at },
        { who: 'заявитель', text: fact.answer, at },
      ],
    });
  },

  addResponse: (serviceId, entry) =>
    set((st) => {
      const responses = [...st.card.responses];
      const i = responses.findIndex((r) => r.serviceId === serviceId);
      if (i === -1) responses.push({ serviceId, entries: [entry] });
      else responses[i] = { ...responses[i], entries: [...responses[i].entries, entry] };
      return { card: { ...st.card, responses } };
    }),

  save: () => {
    const { card, scenario, revealed, startedAt } = get();
    const elapsed = Math.floor((Date.now() - startedAt) / 1000);
    set({
      saved: true,
      result: scenario ? grade(card, scenario, elapsed, revealed) : null,
    });
  },

  reset: () => set({
    card: emptyCard('36814851'), scenario: null, revealed: [],
    transcript: [], startedAt: Date.now(), saved: false, result: null,
  }),
}));
