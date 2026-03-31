import { useQuery } from '@tanstack/react-query';
import { Activity, GitBranch, Play, AlertTriangle } from 'lucide-react';
import { healthApi, dagsApi, runsApi } from '../api/client';
import { StatusBadge } from '../components/StatusBadge';

function StatCard({
  title,
  value,
  icon: Icon,
  gradient,
}: {
  title: string;
  value: string | number;
  icon: React.ElementType;
  gradient: string;
}) {
  return (
    <div className="group rounded-xl border border-gray-200 bg-white p-6 transition-shadow hover:shadow-md dark:border-gray-800 dark:bg-gray-900">
      <div className="flex items-center justify-between">
        <div>
          <p className="text-sm font-medium text-gray-500 dark:text-gray-400">{title}</p>
          <p className="mt-2 text-3xl font-bold text-gray-900 dark:text-white">{value}</p>
        </div>
        <div className={`rounded-xl p-3 ${gradient} shadow-lg shadow-black/5`}>
          <Icon className="h-6 w-6 text-white" />
        </div>
      </div>
    </div>
  );
}

export function DashboardPage() {
  const health = useQuery({ queryKey: ['health'], queryFn: healthApi.check });
  const dags = useQuery({ queryKey: ['dags'], queryFn: dagsApi.list });
  const runs = useQuery({ queryKey: ['runs'], queryFn: runsApi.list });

  const activeDags = dags.data?.filter((d) => !d.is_paused).length ?? 0;
  const totalRuns = runs.data?.length ?? 0;
  const failedRuns = runs.data?.filter((r) => r.state === 'failed').length ?? 0;

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Dashboard</h1>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          Vortex Orchestration Platform — {health.data?.version ?? 'loading...'}
        </p>
      </div>

      <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          title="System Status"
          value={health.data?.status ?? '...'}
          icon={Activity}
          gradient="bg-gradient-to-br from-emerald-500 to-emerald-600"
        />
        <StatCard
          title="Active DAGs"
          value={activeDags}
          icon={GitBranch}
          gradient="bg-gradient-to-br from-vortex-500 to-vortex-700"
        />
        <StatCard
          title="Total Runs"
          value={totalRuns}
          icon={Play}
          gradient="bg-gradient-to-br from-blue-500 to-blue-600"
        />
        <StatCard
          title="Failed Runs"
          value={failedRuns}
          icon={AlertTriangle}
          gradient="bg-gradient-to-br from-red-500 to-red-600"
        />
      </div>

      <div className="rounded-xl border border-gray-200 bg-white p-6 dark:border-gray-800 dark:bg-gray-900">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white">Recent Runs</h2>
        {runs.isLoading ? (
          <div className="mt-6 flex items-center justify-center py-8">
            <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
          </div>
        ) : runs.data && runs.data.length > 0 ? (
          <div className="mt-4 overflow-x-auto">
            <table className="w-full table-fixed divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr>
                  <th className="px-3 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">DAG</th>
                  <th className="px-3 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">Run ID</th>
                  <th className="px-3 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">Status</th>
                  <th className="px-3 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">Started</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                {runs.data.slice(0, 10).map((run) => (
                  <tr key={run.id} className="transition-colors hover:bg-gray-50 dark:hover:bg-gray-800/50">
                    <td className="px-3 py-3 text-sm font-medium text-gray-900 dark:text-white">{run.dag_id}</td>
                    <td className="max-w-[120px] truncate px-3 py-3 font-mono text-xs text-gray-500 dark:text-gray-400" title={run.id}>{run.id.slice(0, 8)}</td>
                    <td className="px-3 py-3"><StatusBadge status={run.state} /></td>
                    <td className="px-3 py-3 text-xs text-gray-500 dark:text-gray-400">{run.start_time ? new Date(run.start_time).toLocaleString() : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="mt-6 text-center text-sm text-gray-500 dark:text-gray-400">No runs yet.</p>
        )}
      </div>
    </div>
  );
}
