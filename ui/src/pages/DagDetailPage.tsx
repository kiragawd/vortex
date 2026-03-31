import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Play, RotateCcw, ChevronRight, Save } from 'lucide-react';
import { dagsApi, type DagRun } from '../api/client';
import { StatusBadge } from '../components/StatusBadge';
import { DagGraph } from '../components/DagGraph';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { showToast } from '../components/Toast';

type Tab = 'graph' | 'runs' | 'info' | 'source';

export function DagDetailPage() {
  const { dagId } = useParams<{ dagId: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [tab, setTab] = useState<Tab>('graph');
  const [confirmAction, setConfirmAction] = useState<'trigger' | 'retry' | null>(null);
  const [sourceCode, setSourceCode] = useState('');
  const [sourceLoaded, setSourceLoaded] = useState(false);
  const [sourceSaving, setSourceSaving] = useState(false);
  const [selectedTask, setSelectedTask] = useState<string | null>(null);

  const { data: tasksData, isLoading: tasksLoading, error: tasksError } = useQuery({
    queryKey: ['dag-tasks', dagId],
    queryFn: () => dagsApi.getTasks(dagId!),
    enabled: !!dagId,
  });

  const dag = tasksData?.dag as import('../api/client').Dag | undefined;

  const { data: runs = [], isLoading: runsLoading } = useQuery({
    queryKey: ['dag-runs', dagId],
    queryFn: () => dagsApi.getRuns(dagId!, 20),
    enabled: !!dagId,
  });

  const { data: sourceData, isLoading: sourceLoading } = useQuery({
    queryKey: ['dag-source', dagId],
    queryFn: () => dagsApi.getSource(dagId!),
    enabled: !!dagId && tab === 'source',
  });

  // Sync source code when data loads
  if (sourceData && !sourceLoaded) {
    setSourceCode(sourceData.source);
    setSourceLoaded(true);
  }

  const triggerMutation = useMutation({
    mutationFn: () => dagsApi.trigger(dagId!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['dag-runs', dagId] });
      queryClient.invalidateQueries({ queryKey: ['runs'] });
      showToast('success', `DAG "${dagId}" triggered successfully`);
      setConfirmAction(null);
    },
    onError: (err) => {
      showToast('error', `Failed to trigger DAG: ${err}`);
      setConfirmAction(null);
    },
  });

  const retryMutation = useMutation({
    mutationFn: () => dagsApi.retry(dagId!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['dag-runs', dagId] });
      showToast('success', `Retry triggered for DAG "${dagId}"`);
      setConfirmAction(null);
    },
    onError: (err) => {
      showToast('error', `Failed to retry DAG: ${err}`);
      setConfirmAction(null);
    },
  });

  if (tasksLoading) {
    return (
      <div className="flex items-center justify-center py-16">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
      </div>
    );
  }
  if (tasksError || !dag) {
    return (
      <p className="text-sm text-red-500 dark:text-red-400">
        {tasksError ? String(tasksError) : 'DAG not found.'}
      </p>
    );
  }

  const tasks = tasksData?.tasks ?? [];
  const dependencies = tasksData?.dependencies ?? [];
  const instances = tasksData?.instances ?? [];

  // Show DAG structure by default (no run state overlay).
  // Only overlay run states when user explicitly views a specific run via Runs tab.
  const graphStateByName: Record<string, string> = {};

  // Detect file type from source path to choose correct reparse API
  const isPythonDag = sourceData?.file_path?.endsWith('.py') ?? true;

  const handleSaveSource = async () => {
    if (!dagId) return;
    setSourceSaving(true);
    try {
      if (isPythonDag) {
        await dagsApi.updateSource(dagId, sourceCode);
      } else {
        await dagsApi.updateSourceRust(dagId, sourceCode);
      }
      showToast('success', `Source saved and re-parsed (${isPythonDag ? 'Python' : 'Config/Rust'})`);
      // Refresh tasks/graph after reparse
      queryClient.invalidateQueries({ queryKey: ['dag-tasks', dagId] });
      queryClient.invalidateQueries({ queryKey: ['dag-source', dagId] });
      setSourceLoaded(false);
    } catch (err) {
      showToast('error', `Failed to save source: ${err}`);
    } finally {
      setSourceSaving(false);
    }
  };

  const tabs: { id: Tab; label: string }[] = [
    { id: 'graph', label: 'Graph' },
    { id: 'runs', label: `Runs (${runs.length})` },
    { id: 'source', label: 'Source' },
    { id: 'info', label: 'Info' },
  ];

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">{dag.id}</h1>
          <StatusBadge status={dag.is_paused ? 'inactive' : 'active'} />
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => setConfirmAction('retry')}
            disabled={retryMutation.isPending}
            className="inline-flex items-center gap-1.5 rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-xs font-medium text-gray-700 shadow-sm transition-colors hover:bg-gray-50 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-300 dark:hover:bg-gray-800 disabled:opacity-50"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            Retry Last
          </button>
          <button
            onClick={() => setConfirmAction('trigger')}
            disabled={triggerMutation.isPending}
            className="inline-flex items-center gap-1.5 rounded-lg bg-vortex-600 px-3 py-1.5 text-xs font-medium text-white shadow-sm transition-colors hover:bg-vortex-700 active:bg-vortex-800 disabled:opacity-50"
          >
            <Play className="h-3.5 w-3.5" />
            Trigger
          </button>
        </div>
      </div>

      {/* Quick stats */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {[
          { label: 'Schedule', value: dag.schedule_interval ?? 'Manual' },
          { label: 'Last Run', value: dag.last_run ? new Date(dag.last_run).toLocaleDateString() : 'Never' },
          { label: 'Next Run', value: dag.next_run ? new Date(dag.next_run).toLocaleDateString() : '—' },
          { label: 'Tasks', value: String(tasks.length) },
        ].map((s) => (
          <div key={s.label} className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <p className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">{s.label}</p>
            <p className="mt-1 font-mono text-sm font-semibold text-gray-900 dark:text-white">{s.value}</p>
          </div>
        ))}
      </div>

      {/* Tabs */}
      <div className="border-b border-gray-200 dark:border-gray-800">
        <nav className="-mb-px flex gap-6">
          {tabs.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={`pb-3 text-sm font-medium transition-colors ${
                tab === t.id
                  ? 'border-b-2 border-vortex-600 text-vortex-600 dark:border-vortex-400 dark:text-vortex-400'
                  : 'text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200'
              }`}
            >
              {t.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Tab content */}
      {tab === 'graph' && (
        <div className="space-y-3">
          <p className="text-xs text-gray-500 dark:text-gray-400">
            Showing DAG structure — task types are color-coded. Click a task node to explore downstream dependencies.
          </p>
          {dependencies.length === 0 && tasks.length > 0 && (
            <p className="text-xs text-amber-600 dark:text-amber-400">
              No explicit dependencies defined — showing inferred sequential layout.
            </p>
          )}
          {tasksLoading ? (
            <div className="flex items-center justify-center py-12">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
            </div>
          ) : (
            <DagGraph
              tasks={tasks}
              dependencies={dependencies}
              instancesByTask={graphStateByName}
              onTaskClick={(name) => setSelectedTask(name === selectedTask ? null : name)}
            />
          )}
          {/* Inline task detail panel */}
          {selectedTask && (() => {
            const task = tasks.find((t) => t.name === selectedTask);
            if (!task) return null;
            const inst = instances.find((i) => i.task_id === task.id);
            const upstreams = dependencies.filter(([, d]) => d === selectedTask).map(([u]) => u);
            const downstreams = dependencies.filter(([u]) => u === selectedTask).map(([, d]) => d);
            return (
              <div className="rounded-xl border border-indigo-200 bg-indigo-50/50 p-4 dark:border-indigo-800 dark:bg-indigo-950/30">
                <div className="flex items-start justify-between">
                  <h3 className="text-sm font-semibold text-gray-900 dark:text-white">{selectedTask}</h3>
                  <button onClick={() => setSelectedTask(null)} className="text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-200">✕</button>
                </div>
                <div className="mt-2 grid grid-cols-2 gap-x-6 gap-y-1 text-xs sm:grid-cols-4">
                  <div><span className="text-gray-500 dark:text-gray-400">Type:</span> <span className="font-medium text-gray-900 dark:text-white">{task.task_type || 'bash'}</span></div>
                  {inst && <div><span className="text-gray-500 dark:text-gray-400">Status:</span> <span className="font-medium text-gray-900 dark:text-white">{inst.state}</span></div>}
                  <div><span className="text-gray-500 dark:text-gray-400">Upstream:</span> <span className="font-medium text-gray-900 dark:text-white">{upstreams.length > 0 ? upstreams.join(', ') : 'none (root)'}</span></div>
                  <div><span className="text-gray-500 dark:text-gray-400">Downstream:</span> <span className="font-medium text-gray-900 dark:text-white">{downstreams.length > 0 ? downstreams.join(', ') : 'none (leaf)'}</span></div>
                </div>
                {inst?.run_id && (
                  <button
                    onClick={() => navigate(`/dags/${encodeURIComponent(dagId!)}/runs/${encodeURIComponent(inst.run_id!)}`)}
                    className="mt-3 inline-flex items-center gap-1 rounded-lg bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-700"
                  >
                    <ChevronRight className="h-3 w-3" /> View Run
                  </button>
                )}
              </div>
            );
          })()}
        </div>
      )}

      {tab === 'runs' && (
        <div className="overflow-x-auto rounded-xl border border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900">
          {runsLoading ? (
            <div className="flex items-center justify-center py-12">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
            </div>
          ) : runs.length === 0 ? (
            <div className="p-12 text-center text-sm text-gray-500 dark:text-gray-400">
              No runs recorded yet. Trigger a run to start.
            </div>
          ) : (
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr className="bg-gray-50 dark:bg-gray-800/50">
                  {['Run ID', 'Status', 'Triggered', 'Started', 'Finished', 'SLA', ''].map((h) => (
                    <th key={h} className="px-5 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                {runs.map((run: DagRun) => (
                  <tr
                    key={run.id}
                    onClick={() => navigate(`/dags/${encodeURIComponent(dagId!)}/runs/${encodeURIComponent(run.id)}`)}
                    className="cursor-pointer transition-colors hover:bg-gray-50 dark:hover:bg-gray-800/50"
                  >
                    <td className="whitespace-nowrap px-5 py-3">
                      <span className="rounded bg-gray-100 px-1.5 py-0.5 font-mono text-xs dark:bg-gray-800">
                        {run.id.slice(0, 8)}…
                      </span>
                    </td>
                    <td className="px-5 py-3"><StatusBadge status={run.state} /></td>
                    <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {run.triggered_by}
                    </td>
                    <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {run.start_time ? new Date(run.start_time).toLocaleString() : '—'}
                    </td>
                    <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {run.end_time ? new Date(run.end_time).toLocaleString() : '—'}
                    </td>
                    <td className="px-5 py-3">
                      {run.sla_missed && (
                        <span className="rounded-full bg-red-100 px-2 py-0.5 text-xs font-medium text-red-700 dark:bg-red-500/10 dark:text-red-400">
                          SLA Missed
                        </span>
                      )}
                    </td>
                    <td className="px-5 py-3 text-right">
                      <ChevronRight className="h-4 w-4 text-gray-400" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

      {tab === 'info' && (
        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
          <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <h2 className="text-base font-semibold text-gray-900 dark:text-white">DAG Properties</h2>
            <dl className="mt-4 space-y-3">
              {[
                ['ID', dag.id],
                ['Schedule', dag.schedule_interval ?? 'Manual'],
                ['Status', dag.is_paused ? 'Paused' : 'Active'],
                ['Last Run', dag.last_run ? new Date(dag.last_run).toLocaleString() : 'Never'],
                ['Next Run', dag.next_run ? new Date(dag.next_run).toLocaleString() : '—'],
                ['Created', dag.created_at ? new Date(dag.created_at).toLocaleString() : '—'],
                ['Team', dag.team_id ?? 'Global'],
              ].map(([k, v]) => (
                <div key={k} className="flex items-start justify-between gap-4">
                  <dt className="text-sm font-medium text-gray-500 dark:text-gray-400">{k}</dt>
                  <dd className="text-right font-mono text-sm text-gray-900 dark:text-gray-100">{v}</dd>
                </div>
              ))}
            </dl>
          </div>

          <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <h2 className="text-base font-semibold text-gray-900 dark:text-white">
              Tasks ({tasks.length})
            </h2>
            {tasks.length === 0 ? (
              <p className="mt-4 text-sm text-gray-400">No tasks defined.</p>
            ) : (
              <ul className="mt-4 space-y-2">
                {tasks.map((t) => (
                  <li key={t.id} className="flex items-center gap-2 rounded-lg border border-gray-100 px-3 py-2 dark:border-gray-800">
                    <span className="h-2 w-2 rounded-full bg-vortex-500" />
                    <span className="flex-1 text-sm text-gray-800 dark:text-gray-200">{t.name}</span>
                    <span className="rounded bg-gray-100 px-1.5 py-0.5 text-xs text-gray-500 dark:bg-gray-800 dark:text-gray-400">
                      {t.task_type ?? 'task'}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      {tab === 'source' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <h2 className="text-base font-semibold text-gray-900 dark:text-white">
                DAG Source
              </h2>
              {sourceData?.file_path && (
                <span className="rounded-md bg-gray-100 px-2 py-0.5 font-mono text-xs text-gray-500 dark:bg-gray-800 dark:text-gray-400">
                  {sourceData.file_path}
                </span>
              )}
              <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${
                isPythonDag
                  ? 'bg-purple-100 text-purple-700 dark:bg-purple-500/10 dark:text-purple-400'
                  : 'bg-orange-100 text-orange-700 dark:bg-orange-500/10 dark:text-orange-400'
              }`}>
                {isPythonDag ? 'Python' : 'Config/Rust'}
              </span>
            </div>
            <button
              onClick={handleSaveSource}
              disabled={sourceSaving || sourceLoading}
              className="inline-flex items-center gap-1.5 rounded-lg bg-vortex-600 px-3 py-1.5 text-xs font-medium text-white shadow-sm transition-colors hover:bg-vortex-700 active:bg-vortex-800 disabled:opacity-50"
            >
              <Save className="h-3.5 w-3.5" />
              {sourceSaving ? 'Saving & Reparsing…' : 'Save & Reparse'}
            </button>
          </div>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            Edit the DAG source below. Saving will write the file and reparse it using the{' '}
            {isPythonDag ? 'Python (PyO3)' : 'Config (JSON/YAML)'} parser.
          </p>
          {sourceLoading ? (
            <div className="flex items-center justify-center py-12">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
            </div>
          ) : (
            <textarea
              value={sourceCode}
              onChange={(e) => setSourceCode(e.target.value)}
              spellCheck={false}
              className="h-[500px] w-full rounded-xl border border-gray-200 bg-gray-950 p-4 font-mono text-sm text-green-400 focus:border-vortex-500 focus:outline-none focus:ring-1 focus:ring-vortex-500 dark:border-gray-700"
            />
          )}
        </div>
      )}

      {/* Confirmation Dialogs */}
      <ConfirmDialog
        open={confirmAction === 'trigger'}
        title="Trigger DAG"
        message={`Are you sure you want to trigger DAG "${dagId}"? This will start a new execution run.`}
        confirmLabel="Trigger"
        variant="primary"
        loading={triggerMutation.isPending}
        onConfirm={() => triggerMutation.mutate()}
        onCancel={() => setConfirmAction(null)}
      />
      <ConfirmDialog
        open={confirmAction === 'retry'}
        title="Retry Failed Tasks"
        message={`Are you sure you want to retry failed tasks in DAG "${dagId}"? This will re-run the last failed execution.`}
        confirmLabel="Retry"
        variant="danger"
        loading={retryMutation.isPending}
        onConfirm={() => retryMutation.mutate()}
        onCancel={() => setConfirmAction(null)}
      />
    </div>
  );
}