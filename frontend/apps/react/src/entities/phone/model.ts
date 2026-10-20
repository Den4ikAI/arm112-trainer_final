/**
 * Параметры IP-телефона: включён ли он, пишется ли голос, свой номер.
 *
 * Решает сервер (параметры `ui.phone`, `phone.*` у администратора).
 * Спрашиваем один раз после входа; администратор поменял параметр —
 * перечитываем сразу.
 */
import { create } from 'zustand';
import { phone, type PhoneConfig } from '@/shared/api';

interface PhoneState {
  config: PhoneConfig | null;
  load(): Promise<void>;
}

export const usePhone = create<PhoneState>((set) => ({
  config: null,
  load: async () => {
    try {
      set({ config: await phone.config() });
    } catch {
      // Сервер без телефона — звонки кнопками в карточке, как раньше
      set({
        config: {
          enabled: false, record: false, ringSec: 20, answerSec: 2,
          asr: false, tts: false, extension: null,
        },
      });
    }
  },
}));
