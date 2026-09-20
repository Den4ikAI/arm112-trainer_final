/**
 * Текущий пользователь.
 *
 * Роль и служба приходят с сервера и здесь только хранятся: решение,
 * что человеку можно, принимает сервер. Интерфейс по ним лишь решает,
 * что рисовать.
 */
import { create } from 'zustand';
import { auth, getToken, setToken, type User } from '@/shared/api';

interface SessionState {
  user: User | null;
  /** Идёт первичная проверка сохранённого токена. */
  checking: boolean;
  error: string | null;
  signIn(login: string, password: string, workstation: string): Promise<boolean>;
  signOut(): Promise<void>;
  restore(): Promise<void>;
}

export const useSession = create<SessionState>((set) => ({
  user: null,
  checking: true,
  error: null,

  signIn: async (login, password, workstation) => {
    set({ error: null });
    try {
      const user = await auth.login(login, password, workstation);
      set({ user });
      return true;
    } catch {
      // Сервер намеренно не различает неизвестный логин и неверный
      // пароль, поэтому и сообщение одно
      set({ error: 'Неверный логин или пароль' });
      return false;
    }
  },

  signOut: async () => {
    await auth.logout();
    set({ user: null });
  },

  // Перезагрузка страницы посреди занятия не должна выкидывать на вход
  restore: async () => {
    if (!getToken()) {
      set({ checking: false });
      return;
    }
    try {
      set({ user: await auth.me(), checking: false });
    } catch {
      setToken(null);
      set({ user: null, checking: false });
    }
  },
}));

export const roleTitle = (role: User['role']): string =>
  ({ student: 'Обучающийся', teacher: 'Преподаватель', admin: 'Администратор' })[role];
