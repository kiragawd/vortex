import { clsx } from 'clsx';

interface StatusBadgeProps {
  status: string;
  className?: string;
}

const statusStyles: Record<string, string> = {
  success: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400',
  running: 'bg-blue-100 text-blue-700 dark:bg-blue-500/10 dark:text-blue-400',
  failed: 'bg-red-100 text-red-700 dark:bg-red-500/10 dark:text-red-400',
  pending: 'bg-amber-100 text-amber-700 dark:bg-amber-500/10 dark:text-amber-400',
  queued: 'bg-gray-100 text-gray-700 dark:bg-gray-500/10 dark:text-gray-400',
  skipped: 'bg-gray-100 text-gray-500 dark:bg-gray-500/10 dark:text-gray-500',
  active: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400',
  inactive: 'bg-gray-100 text-gray-500 dark:bg-gray-500/10 dark:text-gray-500',
};

const statusDots: Record<string, string> = {
  success: 'bg-emerald-500',
  running: 'bg-blue-500 animate-pulse',
  failed: 'bg-red-500',
  pending: 'bg-amber-500',
  queued: 'bg-gray-400',
  active: 'bg-emerald-500',
};

export function StatusBadge({ status, className }: StatusBadgeProps) {
  const style = statusStyles[status.toLowerCase()] || 'bg-gray-100 text-gray-700 dark:bg-gray-500/10 dark:text-gray-400';
  const dot = statusDots[status.toLowerCase()];
  return (
    <span
      className={clsx(
        'inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium capitalize',
        style,
        className,
      )}
    >
      {dot && <span className={clsx('h-1.5 w-1.5 rounded-full', dot)} />}
      {status}
    </span>
  );
}
