import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from 'react-router-dom';
import { Play } from 'lucide-react';
import { dagsApi, type Dag } from '../api/client';
import { StatusBadge } from '../components/StatusBadge';
import { DataTable } from '../components/DataTable';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { showToast } from '../components/Toast';

export function DagsPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { data: dags = [], isLoading } = useQuery({ queryKey: ['dags'], queryFn: dagsApi.list });
  const [triggerTarget, setTriggerTarget] = useState<string | null>(null);

  const triggerMutation = useMutation({
    mutationFn: (name: string) => dagsApi.trigger(name),
    onSuccess: (_data, name) => {
      queryClient.invalidateQueries({ queryKey: ['runs'] });
      showToast('success', `DAG "${name}" triggered successfully`);
      setTriggerTarget(null);
    },
    onError: (err) => {
      showToast('error', `Failed to trigger DAG: ${err}`);
      setTriggerTarget(null);
    },
  });

  const columns = [
    { key: 'id', header: 'DAG Name' },
    {
      key: 'schedule_interval',
      header: 'Schedule',
      render: (dag: Dag) => (
        <span className="rounded-md bg-gray-100 px-2 py-0.5 font-mono text-xs dark:bg-gray-800">
          {dag.schedule_interval ?? 'Manual'}
        </span>
      ),
    },
    {
      key: 'is_paused',
      header: 'Status',
      render: (dag: Dag) => <StatusBadge status={dag.is_paused ? 'inactive' : 'active'} />,
    },
    {
      key: 'next_run',
      header: 'Next Run',
      render: (dag: Dag) => (
        <span className="text-gray-500 dark:text-gray-400">
          {dag.next_run ? new Date(dag.next_run).toLocaleString() : '—'}
        </span>
      ),
    },
    {
      key: 'actions',
      header: 'Actions',
      render: (dag: Dag) => (
        <button
          onClick={(e) => {
            e.stopPropagation();
            setTriggerTarget(dag.id);
          }}
          className="inline-flex items-center gap-1.5 rounded-lg bg-vortex-600 px-3 py-1.5 text-xs font-medium text-white shadow-sm transition-colors hover:bg-vortex-700 active:bg-vortex-800"
        >
          <Play className="h-3 w-3" /> Trigger
        </button>
      ),
    },
  ];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">DAGs</h1>
        <span className="rounded-full bg-gray-100 px-3 py-1 text-sm font-medium text-gray-600 dark:bg-gray-800 dark:text-gray-300">
          {dags.length} total
        </span>
      </div>
      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
        </div>
      ) : (
        <DataTable
          columns={columns}
          data={dags}
          onRowClick={(row) => navigate(`/dags/${encodeURIComponent(String(row.id))}`)}
          emptyMessage="No DAGs registered yet."
        />
      )}

      <ConfirmDialog
        open={triggerTarget !== null}
        title="Trigger DAG"
        message={`Are you sure you want to trigger DAG "${triggerTarget}"? This will start a new execution run.`}
        confirmLabel="Trigger"
        variant="primary"
        loading={triggerMutation.isPending}
        onConfirm={() => triggerTarget && triggerMutation.mutate(triggerTarget)}
        onCancel={() => setTriggerTarget(null)}
      />
    </div>
  );
}
