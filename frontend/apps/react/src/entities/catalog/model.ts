/**
 * Справочники с сервера: типы происшествия, службы, опросные карты.
 *
 * Раньше это были константы в доменном пакете. Теперь единственный
 * источник — импортированный классификатор: когда заказчик пришлёт
 * новую версию, интерфейс подхватит её сам, без пересборки.
 *
 * Опросные карты грузятся по одной, когда курсант выбирает тип:
 * их 1283, тянуть всё сразу незачем.
 */
import { create } from 'zustand';
import { catalog, type IncidentType, type Service, type SurveyCard } from '@/shared/api';

interface CatalogState {
  types: IncidentType[];
  services: Service[];
  applicantStatuses: string[];
  cards: Record<string, SurveyCard>;
  loaded: boolean;
  load(): Promise<void>;
  loadCard(typeId: string): Promise<void>;
  serviceTitle(id: string): string;
  typeTitle(id: string): string;
}

export const useCatalog = create<CatalogState>((set, get) => ({
  types: [],
  services: [],
  applicantStatuses: [],
  cards: {},
  loaded: false,

  load: async () => {
    if (get().loaded) return;
    const [types, services, applicantStatuses] = await Promise.all([
      catalog.incidentTypes(),
      catalog.services(),
      catalog.applicantStatuses(),
    ]);
    set({ types, services, applicantStatuses, loaded: true });
  },

  loadCard: async (typeId) => {
    if (get().cards[typeId]) return;
    const card = await catalog.surveyCard(typeId);
    set((st) => ({ cards: { ...st.cards, [typeId]: card } }));
  },

  // Идентификатор вместо названия — заметная поломка, а не тихая
  serviceTitle: (id) => get().services.find((s) => s.id === id)?.title ?? id,
  typeTitle: (id) => get().types.find((t) => t.id === id)?.title ?? id,
}));
