import { create } from 'zustand';

export type Role = 'Обучающийся' | 'Преподаватель' | 'Администратор';

interface SessionState {
  login: string;
  workstation: string;
  role: Role;
  authorized: boolean;
  signIn(login: string, workstation: string): void;
  signOut(): void;
}

export const useSession = create<SessionState>((set) => ({
  login: '',
  workstation: '4',
  role: 'Обучающийся',
  authorized: false,
  signIn: (login, workstation) => set({ login, workstation, authorized: true }),
  signOut: () => set({ login: '', authorized: false }),
}));
