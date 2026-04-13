import { useQuery, useMutation } from '@tanstack/react-query';
import { Server, Wifi, WifiOff, ZapOff } from 'lucide-react';
import { swarmApi, type SwarmWorker } from '../api/client';

export function SwarmPage() {
  const { data: status, isLoading: statusLoading } = useQuery({
    queryKey: ['swarm-status'],
    queryFn: swarmApi.status,
    refetchInterval: 5000,
  });

  const { data: workers = [], isLoading: workersLoading } = useQuery({
    queryKey: ['swarm-workers'],
    queryFn: swarmApi.workers,
    refetchInterval: 5000,
  });

  const drainMutation = useMutation({
    mutationFn: (id: string) => swarmApi.drainWorker(id),
  });

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Swarm &amp; Workers</h1>

      {/* Status cards */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-ryuo-50 dark:bg-ryuo-950/30">
              <Server className="h-5 w-5 text-ryuo-600 dark:text-ryuo-400" />
            </div>
            <div>
              <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Swarm Mode</p>
              <p className="mt-0.5 text-sm font-semibold text-gray-900 dark:text-white">
                {statusLoading ? '…' : status?.enabled ? 'Enabled' : 'Disabled'}
              </p>
            </div>
          </div>
        </div>

        <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-emerald-50 dark:bg-emerald-950/30">
              <Wifi className="h-5 w-5 text-emerald-600 dark:text-emerald-400" />
            </div>
            <div>
              <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Active Workers</p>
              <p className="mt-0.5 text-2xl font-bold text-gray-900 dark:text-white">
                {statusLoading ? '…' : (status?.active_workers ?? 0)}
              </p>
            </div>
          </div>
        </div>

        <div className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-amber-50 dark:bg-amber-950/30">
              <ZapOff className="h-5 w-5 text-amber-600 dark:text-amber-400" />
            </div>
            <div>
              <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Queue Depth</p>
              <p className="mt-0.5 text-2xl font-bold text-gray-900 dark:text-white">
                {statusLoading ? '…' : (status?.queue_depth ?? 0)}
              </p>
            </div>
          </div>
        </div>
      </div>

      {/* Workers table */}
      <div className="rounded-xl border border-gray-200 bg-white shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="border-b border-gray-200 px-6 py-4 dark:border-gray-800">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">Workers</h2>
        </div>
        {workersLoading ? (
          <div className="flex items-center justify-center py-12">
            <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
          </div>
        ) : workers.length === 0 ? (
          <div className="p-12 text-center">
            <WifiOff className="mx-auto h-10 w-10 text-gray-300 dark:text-gray-700" />
            <p className="mt-3 text-sm text-gray-500 dark:text-gray-400">
              No workers registered. Start workers with <code className="rounded bg-gray-100 px-1.5 font-mono text-xs dark:bg-gray-800">ryuo worker</code>.
            </p>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr className="bg-gray-50 dark:bg-gray-800/50">
                  {['Worker ID', 'Host / Address', 'Status', 'Current Task', 'Active Tasks', 'Capacity', 'Last Heartbeat', ''].map((h) => (
                    <th key={h} className="px-5 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                {workers.map((w: SwarmWorker) => (
                  <tr key={w.worker_id} className="hover:bg-gray-50 dark:hover:bg-gray-800/50">
                    <td className="whitespace-nowrap px-5 py-3 font-mono text-xs text-gray-700 dark:text-gray-300">
                      {w.worker_id ? w.worker_id.slice(0, 12) + '…' : '—'}
                    </td>
                    <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-600 dark:text-gray-400">
                      {w.hostname ?? w.address ?? '—'}
                    </td>
                    <td className="px-5 py-3">
                      <span className={`inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-xs font-medium ${
                        w.status === 'idle' || w.status === 'ready'
                          ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400'
                          : w.status === 'busy' || w.status === 'running'
                          ? 'bg-blue-100 text-blue-700 dark:bg-blue-500/10 dark:text-blue-400'
                          : 'bg-gray-100 text-gray-500 dark:bg-gray-700 dark:text-gray-400'
                      }`}>
                        <span className="h-1.5 w-1.5 rounded-full bg-current" />
                        {w.status ?? 'unknown'}
                      </span>
                    </td>
                    <td className="px-5 py-3 font-mono text-xs text-gray-500 dark:text-gray-400">
                      {w.current_task ?? '—'}
                    </td>
                    <td className="px-5 py-3 text-center text-sm text-emerald-600 dark:text-emerald-400">
                      {w.active_tasks ?? 0}
                    </td>
                    <td className="px-5 py-3 text-center text-sm text-gray-500 dark:text-gray-400">
                      {w.capacity ?? '—'}
                    </td>
                    <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {w.last_heartbeat ? new Date(w.last_heartbeat).toLocaleString() : '—'}
                    </td>
                    <td className="px-5 py-3 text-right">
                      <button
                        onClick={() => drainMutation.mutate(w.worker_id)}
                        disabled={drainMutation.isPending}
                        className="rounded px-2 py-1 text-xs text-amber-600 hover:bg-amber-50 dark:text-amber-400 dark:hover:bg-amber-950/20 disabled:opacity-50"
                      >
                        Drain
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
