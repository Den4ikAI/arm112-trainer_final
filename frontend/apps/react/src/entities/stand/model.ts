/**
 * Что известно о стенде до входа: свёрнут ли приём вызовов 112 и какие
 * учебные учётные записи показать на экране входа.
 *
 * Обратная связь экспертов 18.10: тренажёр — для специалиста ДДС, 112
 * «свернуть, чтобы глаза не мозолил»; логины и пароли — на экран входа.
 * Решает сервер (параметр `ui.op112`, переменная `DEMO_PASSWORD`),
 * интерфейс только рисует.
 */
import { create } from 'zustand';
import { stand, type Stand } from '@/shared/api';

interface StandState {
  /** Показывать рабочее место оператора 112 */
  op112: boolean;
  demo: Stand['demo'];
  loaded: boolean;
  load(): Promise<void>;
}

export const useStand = create<StandState>((set) => ({
  // До ответа сервера 112 не показываем: иначе вкладки мелькнут и пропадут
  op112: false,
  demo: null,
  loaded: false,

  load: async () => {
    try {
      const st = await stand.get();
      set({ op112: st.op112, demo: st.demo, loaded: true });
    } catch {
      // Сервер старше этого экрана — 112 у него не сворачивается
      set({ op112: true, demo: null, loaded: true });
    }
  },
}));
