import { create } from 'zustand';

interface User {
  id: string;
  username: string;
  roles: string[];
}

interface AppState {
  user: User | null;
  theme: 'light' | 'dark';
  sidebarOpen: boolean;
  setUser: (user: User | null) => void;
  logout: () => void;
  toggleTheme: () => void;
  toggleSidebar: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  user: null,
  theme: (localStorage.getItem('ryuo_theme') as 'light' | 'dark') || 'light',
  sidebarOpen: true,
  setUser: (user) => set({ user }),
  logout: () => {
    localStorage.removeItem('ryuo_token');
    set({ user: null });
    window.location.href = '/login';
  },
  toggleTheme: () =>
    set((state) => {
      const next = state.theme === 'light' ? 'dark' : 'light';
      localStorage.setItem('ryuo_theme', next);
      document.documentElement.classList.toggle('dark', next === 'dark');
      return { theme: next };
    }),
  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
}));
