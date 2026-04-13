import { Moon, Sun, Bell, PanelLeftClose, PanelLeft, LogOut } from 'lucide-react';
import { useAppStore } from '../store';

export function Header() {
  const { theme, toggleTheme, toggleSidebar, sidebarOpen, user, logout } = useAppStore();

  return (
    <header className="flex h-16 items-center justify-between border-b border-gray-200 bg-white/80 px-6 backdrop-blur-sm dark:border-gray-800 dark:bg-gray-900/80">
      <div className="flex items-center gap-3">
        <button
          onClick={toggleSidebar}
          className="rounded-lg p-2 text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800"
          aria-label="Toggle sidebar"
        >
          {sidebarOpen ? <PanelLeftClose className="h-5 w-5" /> : <PanelLeft className="h-5 w-5" />}
        </button>
      </div>

      <div className="flex items-center gap-2">
        <button
          onClick={toggleTheme}
          className="rounded-lg p-2 text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800"
          aria-label="Toggle theme"
        >
          {theme === 'light' ? <Moon className="h-5 w-5" /> : <Sun className="h-5 w-5 text-amber-400" />}
        </button>
        <button
          className="relative rounded-lg p-2 text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800"
          aria-label="Notifications"
        >
          <Bell className="h-5 w-5" />
          <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-ryuo-500" />
        </button>
        {user && (
          <div className="ml-2 flex items-center gap-2 rounded-lg border border-gray-200 px-3 py-1.5 dark:border-gray-700">
            <div className="flex h-7 w-7 items-center justify-center rounded-full bg-gradient-to-br from-ryuo-500 to-ryuo-700">
              <span className="text-xs font-semibold text-white">
                {user.username.charAt(0).toUpperCase()}
              </span>
            </div>
            <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{user.username}</span>
          </div>
        )}
        <button
          onClick={logout}
          className="rounded-lg p-2 text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800"
          aria-label="Sign out"
          title="Sign out"
        >
          <LogOut className="h-5 w-5" />
        </button>
      </div>
    </header>
  );
}
