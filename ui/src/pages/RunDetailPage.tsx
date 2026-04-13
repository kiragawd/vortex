import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, RotateCcw, Terminal } from 'lucide-react';
import { dagsApi } from '../api/client';
import { StatusBadge } from '../components/StatusBadge';
import { DagGraph, instanceStateByName } from '../components/DagGraph';
import { useState } from 'react';

export function RunDetailPage() {
  const { dagId, runId } = useParams<{ dagId: string; runId: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [selectedTask, setSelectedTask] = useState<string | null>(null);

  const { data: tasksData, isLoading } = useQuery({
    queryKey: ['dag-tasks', dagId],
    queryFn: () => dagsApi.getTasks(dagId!),
    enabled: !!dagId,
  });

  const retryMutation = useMutation({
    mutationFn: () => dagsApi.retry(dagId!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['runs'] });
      queryClient.invalidateQueries({ queryKey: ['dag-runs', dagId] });
    },
  });

  const tasks = tasksData?.tasks ?? [];
  const dependencies = tasksData?.dependencies ?? [];
  const allInstances = tasksData?.instances ?? [];

  // Filter instances to only this run
  const runInstances = allInstances.filter((i) => i.run_id === runId);

  // State map for the graph
  const stateByName = instanceStateByName(allInstances, tasks, runId);

  // Find run-level info from instances
  const runStart = runInstances.reduce<string | null>((min, i) => {
    if (!i.start_time) return min;
    return min === null || i.start_time < min ? i.start_time : min;
  }, null);
  const runEnd = runInstances.reduce<string | null>((max, i) => {
    if (!i.end_time) return max;
    return max === null || i.end_time > max ? i.end_time : max;
  }, null);

  const failedCount = runInstances.filter((i) => i.state === 'failed').length;
  const successCount = runInstances.filter((i) => i.state === 'success').length;

  // Selected task instance detail
  const selectedInst = selectedTask
    ? runInstances.find((i) => {
        const task = tasks.find((t) => t.name === selectedTask);
        return task && i.task_id === task.id;
      })
    : null;

  return (
    <div className="space-y-6">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
        <button
          onClick={() => navigate(`/dags/${encodeURIComponent(dagId!)}`)}
          className="flex items-center gap-1 hover:text-gray-700 dark:hover:text-gray-200"
        >
          <ArrowLeft className="h-4 w-4" />
          {dagId}
        </button>
        <span>/</span>
        <span className="font-mono text-gray-700 dark:text-gray-300">{runId?.slice(0, 12)}…</span>
      </div>

      {/* Header */}
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold text-gray-900 dark:text-white">Run Detail</h1>
          <p className="mt-1 font-mono text-xs text-gray-500 dark:text-gray-400">{runId}</p>
        </div>
        <button
          onClick={() => retryMutation.mutate()}
          disabled={retryMutation.isPending}
          className="inline-flex items-center gap-1.5 rounded-lg bg-ryuo-600 px-3 py-1.5 text-xs font-medium text-white shadow-sm transition-colors hover:bg-ryuo-700 disabled:opacity-50"
        >
          <RotateCcw className="h-3.5 w-3.5" />
          {retryMutation.isPending ? 'Retrying…' : 'Re-run DAG'}
        </button>
      </div>

      {/* Summary cards */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {[
          { label: 'Total Tasks', value: String(runInstances.length || tasks.length) },
          { label: 'Succeeded', value: String(successCount), color: 'text-emerald-600 dark:text-emerald-400' },
          { label: 'Failed', value: String(failedCount), color: 'text-red-600 dark:text-red-400' },
          {
            label: 'Duration',
            value:
              runStart && runEnd
                ? `${Math.round((new Date(runEnd).getTime() - new Date(runStart).getTime()) / 1000)}s`
                : '—',
          },
        ].map((c) => (
          <div key={c.label} className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">{c.label}</p>
            <p className={`mt-1 text-xl font-bold ${c.color ?? 'text-gray-900 dark:text-white'}`}>{c.value}</p>
          </div>
        ))}
      </div>

      {/* Graph */}
      {isLoading ? (
        <div className="flex items-center justify-center py-12">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
        </div>
      ) : (
        <>
          <div>
            <h2 className="mb-3 text-sm font-semibold text-gray-700 dark:text-gray-300">
              Task Graph — click a node to see its output
            </h2>
            <DagGraph
              tasks={tasks}
              dependencies={dependencies}
              instancesByTask={stateByName}
              onTaskClick={(name) => setSelectedTask(name === selectedTask ? null : name)}
            />
          </div>

          {/* Task instances table */}
          <div className="overflow-x-auto rounded-xl border border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr className="bg-gray-50 dark:bg-gray-800/50">
                  {['Task', 'Status', 'Started', 'Finished', 'Duration', 'Retries', ''].map((h) => (
                    <th key={h} className="px-5 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                {runInstances.length === 0 ? (
                  <tr>
                    <td colSpan={7} className="px-5 py-8 text-center text-sm text-gray-400">
                      No task instances for this run yet.
                    </td>
                  </tr>
                ) : (
                  runInstances.map((inst) => {
                    const task = tasks.find((t) => t.id === inst.task_id);
                    const isSel = selectedTask && task?.name === selectedTask;
                    return (
                      <tr
                        key={inst.id}
                        onClick={() => setSelectedTask(task?.name === selectedTask ? null : (task?.name ?? null))}
                        className={`cursor-pointer transition-colors hover:bg-gray-50 dark:hover:bg-gray-800/50 ${isSel ? 'bg-ryuo-50 dark:bg-ryuo-950/20' : ''}`}
                      >
                        <td className="whitespace-nowrap px-5 py-3 text-sm font-medium text-gray-900 dark:text-gray-100">
                          {task?.name ?? inst.task_id.slice(0, 12)}
                        </td>
                        <td className="px-5 py-3"><StatusBadge status={inst.state} /></td>
                        <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                          {inst.start_time ? new Date(inst.start_time).toLocaleString() : '—'}
                        </td>
                        <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                          {inst.end_time ? new Date(inst.end_time).toLocaleString() : '—'}
                        </td>
                        <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                          {inst.duration_ms != null ? `${(inst.duration_ms / 1000).toFixed(1)}s` : '—'}
                        </td>
                        <td className="px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                          {inst.retry_count}
                        </td>
                        <td className="px-5 py-3">
                          {(inst.stdout || inst.stderr) && (
                            <Terminal className="h-4 w-4 text-gray-400" />
                          )}
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>

          {/* Selected task output */}
          {selectedInst && (selectedInst.stdout || selectedInst.stderr) && (
            <div className="rounded-xl border border-gray-200 bg-gray-950 p-4 dark:border-gray-800">
              <div className="mb-2 flex items-center gap-2">
                <Terminal className="h-4 w-4 text-gray-400" />
                <span className="text-sm font-medium text-gray-300">
                  {selectedTask} — output
                </span>
                <StatusBadge status={selectedInst.state} className="ml-auto" />
              </div>
              {selectedInst.stdout && (
                <pre className="max-h-64 overflow-y-auto rounded bg-black p-3 text-xs text-green-400">
                  {selectedInst.stdout}
                </pre>
              )}
              {selectedInst.stderr && (
                <pre className="mt-2 max-h-64 overflow-y-auto rounded bg-black p-3 text-xs text-red-400">
                  {selectedInst.stderr}
                </pre>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
