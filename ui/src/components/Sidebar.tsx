import { Link, useLocation } from 'react-router-dom';
import { clsx } from 'clsx';
import {
  LayoutDashboard,
  GitBranch,
  Play,
  Shield,
  Users,
  Settings,
  Activity,
  Server,
  GitMerge,
  Plug,
  Zap,
} from 'lucide-react';
import { useAppStore } from '../store';

const navigation = [
  { name: 'Dashboard', href: '/', icon: LayoutDashboard },
  { name: 'DAGs', href: '/dags', icon: GitBranch },
  { name: 'Runs', href: '/runs', icon: Play },
  { name: 'Events', href: '/events', icon: Zap },
  { name: 'Connectors', href: '/connectors', icon: Plug },
  { name: 'Lineage', href: '/lineage', icon: GitMerge },
  { name: 'Swarm', href: '/swarm', icon: Server },
  { name: 'Compliance', href: '/compliance', icon: Shield },
  { name: 'RBAC', href: '/rbac', icon: Users },
  { name: 'Monitoring', href: '/monitoring', icon: Activity },
  { name: 'Settings', href: '/settings', icon: Settings },
];

export function Sidebar() {
  const location = useLocation();
  const sidebarOpen = useAppStore((s) => s.sidebarOpen);

  if (!sidebarOpen) return null;

  return (
    <aside className="flex w-64 flex-col border-r border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900">
      <div className="flex h-16 items-center gap-3 border-b border-gray-200 px-6 dark:border-gray-800">
        <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-gradient-to-br from-vortex-500 to-vortex-700 shadow-md shadow-vortex-500/20">
          <span className="text-sm font-bold text-white">V</span>
        </div>
        <div>
          <span className="text-lg font-bold text-gray-900 dark:text-white">Vortex</span>
          <p className="text-[10px] font-medium uppercase tracking-widest text-gray-400 dark:text-gray-500">Enterprise</p>
        </div>
      </div>
      <nav className="flex-1 space-y-1 px-3 py-4">
        {navigation.map((item) => {
          const active =
            item.href === '/'
              ? location.pathname === '/'
              : location.pathname.startsWith(item.href);
          return (
            <Link
              key={item.name}
              to={item.href}
              className={clsx(
                'flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all',
                active
                  ? 'bg-vortex-50 text-vortex-700 shadow-sm dark:bg-vortex-950 dark:text-vortex-300'
                  : 'text-gray-600 hover:bg-gray-50 hover:text-gray-900 dark:text-gray-400 dark:hover:bg-gray-800 dark:hover:text-gray-200',
              )}
            >
              <item.icon className={clsx('h-5 w-5', active && 'text-vortex-600 dark:text-vortex-400')} />
              {item.name}
            </Link>
          );
        })}
      </nav>
      <div className="border-t border-gray-200 p-4 dark:border-gray-800">
        <div className="rounded-lg bg-gradient-to-r from-vortex-500/10 to-vortex-600/10 p-3 dark:from-vortex-500/5 dark:to-vortex-600/5">
          <p className="text-xs font-medium text-vortex-700 dark:text-vortex-300">Vortex v0.7.0</p>
          <p className="mt-0.5 text-[11px] text-vortex-600/70 dark:text-vortex-400/70">Enterprise Edition</p>
        </div>
      </div>
    </aside>
  );
}
