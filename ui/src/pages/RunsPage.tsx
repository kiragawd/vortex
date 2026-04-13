import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { RotateCcw } from 'lucide-react';
import { runsApi, type DagRun } from '../api/client';
import { StatusBadge } from '../components/StatusBadge';
import { DataTable } from '../components/DataTable';

export function RunsPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: runs = [], isLoading } = useQuery({ queryKey: ['runs'], queryFn: runsApi.list });

  const retryMutation = useMutation({
    mutationFn: (dagId: string) => runsApi.retry(dagId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['runs'] }),
  });

  const columns = [
    { key: 'dag_id', header: 'DAG' },
    {
      key: 'id',
      header: 'Run ID',
      render: (run: DagRun) => (
        <span className="rounded-md bg-gray-100 px-2 py-0.5 font-mono text-xs dark:bg-gray-800" title={run.id}>
          {run.id.slice(0, 8)}
        </span>
      ),
    },
    {
      key: 'state',
      header: 'Status',
      render: (run: DagRun) => <StatusBadge status={run.state} />,
    },
    {
      key: 'start_time',
      header: 'Started',
      render: (run: DagRun) => (
        <span className="text-xs text-gray-500 dark:text-gray-400">
          {run.start_time ? new Date(run.start_time).toLocaleString() : '—'}
        </span>
      ),
    },
    {
      key: 'end_time',
      header: 'Finished',
      render: (run: DagRun) => (
        <span className="text-xs text-gray-500 dark:text-gray-400">
          {run.end_time ? new Date(run.end_time).toLocaleString() : '—'}
        </span>
      ),
    },
    {
      key: 'actions',
      header: '',
      render: (run: DagRun) => (
        <div className="flex items-center gap-2">
          {run.sla_missed && (
            <span className="rounded-full bg-red-100 px-2 py-0.5 text-xs font-medium text-red-700 dark:bg-red-500/10 dark:text-red-400">
              SLA
            </span>
          )}
          {(run.state === 'failed' || run.state === 'success') && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                retryMutation.mutate(run.dag_id);
              }}
              disabled={retryMutation.isPending}
              className="inline-flex items-center gap-1 rounded-lg border border-gray-200 bg-white px-2 py-1 text-xs font-medium text-gray-600 shadow-sm transition-colors hover:bg-gray-50 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-400 dark:hover:bg-gray-800 disabled:opacity-50"
              title="Re-run DAG"
            >
              <RotateCcw className="h-3 w-3" />
              Retry
            </button>
          )}
        </div>
      ),
    },
  ];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Runs</h1>
        <span className="rounded-full bg-gray-100 px-3 py-1 text-sm font-medium text-gray-600 dark:bg-gray-800 dark:text-gray-300">
          {runs.length} total
        </span>
      </div>
      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
        </div>
      ) : (
        <DataTable
          columns={columns}
          data={runs}
          onRowClick={(run) => navigate(`/dags/${encodeURIComponent(run.dag_id)}/runs/${encodeURIComponent(run.id)}`)}
          emptyMessage="No runs recorded yet."
        />
      )}
    </div>
  );
}
