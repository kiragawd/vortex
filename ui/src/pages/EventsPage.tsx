import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { complianceApi, incidentApi, type IncidentConfig } from '../api/client';
import { Bell, Plus, Trash2, Zap, Radio, FileSearch, Globe, Database, GitBranch } from 'lucide-react';
import { clsx } from 'clsx';

const channelColor: Record<string, string> = {
  pagerduty: 'bg-green-100 text-green-700 dark:bg-green-500/10 dark:text-green-400',
  slack: 'bg-purple-100 text-purple-700 dark:bg-purple-500/10 dark:text-purple-400',
  email: 'bg-blue-100 text-blue-700 dark:bg-blue-500/10 dark:text-blue-400',
  webhook: 'bg-amber-100 text-amber-700 dark:bg-amber-500/10 dark:text-amber-400',
};

const eventActionColor: Record<string, string> = {
  'dag.trigger': 'bg-blue-100 text-blue-700 dark:bg-blue-500/10 dark:text-blue-400',
  'dag.retry': 'bg-amber-100 text-amber-700 dark:bg-amber-500/10 dark:text-amber-400',
  'dag.backfill': 'bg-indigo-100 text-indigo-700 dark:bg-indigo-500/10 dark:text-indigo-400',
  'dag.pause': 'bg-gray-100 text-gray-700 dark:bg-gray-500/10 dark:text-gray-400',
  'dag.unpause': 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400',
  'user.login': 'bg-teal-100 text-teal-700 dark:bg-teal-500/10 dark:text-teal-400',
  'secret.store': 'bg-red-100 text-red-700 dark:bg-red-500/10 dark:text-red-400',
};

const SENSOR_TYPES = [
  {
    key: 'file',
    label: 'File Sensor',
    description: 'Waits for a file or directory to exist at a given path.',
    icon: FileSearch,
    color: 'from-blue-500/10 to-blue-600/10',
    iconColor: 'text-blue-600 dark:text-blue-400',
    params: ['filepath', 'mode (poke/reschedule)', 'timeout_secs'],
  },
  {
    key: 'http',
    label: 'HTTP Sensor',
    description: 'Polls a URL until it returns a success status code.',
    icon: Globe,
    color: 'from-emerald-500/10 to-emerald-600/10',
    iconColor: 'text-emerald-600 dark:text-emerald-400',
    params: ['url', 'expected_status', 'method', 'timeout_secs'],
  },
  {
    key: 'external_task',
    label: 'External Task Sensor',
    description: 'Blocks until a task in another DAG reaches a target state.',
    icon: GitBranch,
    color: 'from-purple-500/10 to-purple-600/10',
    iconColor: 'text-purple-600 dark:text-purple-400',
    params: ['dag_id', 'task_id', 'target_state', 'execution_delta'],
  },
  {
    key: 'sql',
    label: 'SQL Sensor',
    description: 'Executes a query and succeeds when it returns a truthy result.',
    icon: Database,
    color: 'from-sky-500/10 to-sky-600/10',
    iconColor: 'text-sky-600 dark:text-sky-400',
    params: ['connection_string', 'sql', 'timeout_secs'],
  },
];

function NewIncidentForm({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const [name, setName] = useState('');
  const [channel, setChannel] = useState('slack');
  const [severity, setSeverity] = useState('');
  const [dagFilter, setDagFilter] = useState('');

  const createMut = useMutation({
    mutationFn: () =>
      incidentApi.createConfig({
        name,
        channel,
        enabled: true,
        config: {},
        severity_filter: severity || null,
        dag_filter: dagFilter || null,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['incident-configs'] });
      onClose();
    },
  });

  return (
    <div className="rounded-xl border border-vortex-200 bg-vortex-50/30 p-5 dark:border-vortex-800 dark:bg-vortex-950/20">
      <h3 className="mb-4 text-sm font-semibold text-gray-900 dark:text-white">New Alert Config</h3>
      <div className="grid gap-3 sm:grid-cols-2">
        <div>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">Name</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. on-call-pagerduty"
            className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 focus:border-vortex-500 focus:outline-none dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">Channel</label>
          <select
            value={channel}
            onChange={(e) => setChannel(e.target.value)}
            className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 focus:border-vortex-500 focus:outline-none dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100"
          >
            {['slack', 'pagerduty', 'email', 'webhook'].map((c) => (
              <option key={c} value={c}>{c}</option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">Severity Filter <span className="text-gray-400">(optional)</span></label>
          <input
            type="text"
            value={severity}
            onChange={(e) => setSeverity(e.target.value)}
            placeholder="critical, warning, info"
            className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 focus:border-vortex-500 focus:outline-none dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300">DAG Filter <span className="text-gray-400">(optional glob)</span></label>
          <input
            type="text"
            value={dagFilter}
            onChange={(e) => setDagFilter(e.target.value)}
            placeholder="e.g. prod_*"
            className="mt-1 w-full rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 focus:border-vortex-500 focus:outline-none dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100"
          />
        </div>
      </div>
      <div className="mt-4 flex gap-2">
        <button
          onClick={() => createMut.mutate()}
          disabled={!name || createMut.isPending}
          className="rounded-lg bg-vortex-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-vortex-700 disabled:opacity-50"
        >
          {createMut.isPending ? 'Creating…' : 'Create'}
        </button>
        <button onClick={onClose} className="rounded-lg px-4 py-1.5 text-sm font-medium text-gray-600 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800">
          Cancel
        </button>
      </div>
    </div>
  );
}

export function EventsPage() {
  const qc = useQueryClient();
  const [showNewForm, setShowNewForm] = useState(false);

  const auditLog = useQuery({
    queryKey: ['event-audit'],
    queryFn: () => complianceApi.auditLog(100),
    refetchInterval: 15000,
  });
  const incidentConfigs = useQuery({
    queryKey: ['incident-configs'],
    queryFn: incidentApi.configs,
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => incidentApi.deleteConfig(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['incident-configs'] }),
  });

  // DAG-related events only
  const eventFeed = (auditLog.data ?? []).filter(
    (e) => e.action.startsWith('dag.') || e.action.startsWith('user.') || e.action.startsWith('secret.'),
  );

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Events &amp; Sensors</h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            Event-driven scheduling, sensor triggers, and incident routing configuration.
          </p>
        </div>
      </div>

      {/* Sensor Types */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="flex items-center gap-2">
          <Radio className="h-5 w-5 text-vortex-600 dark:text-vortex-400" />
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">Available Sensor Types</h2>
        </div>
        <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
          Add a task with <code className="rounded bg-gray-100 px-1 dark:bg-gray-800">task_type: "sensor"</code> and include sensor-specific config.
        </p>
        <div className="mt-4 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {SENSOR_TYPES.map((s) => (
            <div key={s.key} className={clsx('rounded-xl bg-gradient-to-br p-4', s.color)}>
              <div className="flex items-center gap-2">
                <s.icon className={clsx('h-5 w-5', s.iconColor)} />
                <span className="text-sm font-semibold text-gray-900 dark:text-white">{s.label}</span>
              </div>
              <p className="mt-1.5 text-xs text-gray-600 dark:text-gray-300">{s.description}</p>
              <div className="mt-2 flex flex-wrap gap-1">
                {s.params.map((p) => (
                  <span key={p} className="rounded bg-white/70 px-1.5 py-0.5 font-mono text-[10px] text-gray-600 dark:bg-black/20 dark:text-gray-400">
                    {p}
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Incident / Alert Routing */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Bell className="h-5 w-5 text-vortex-600 dark:text-vortex-400" />
            <h2 className="text-base font-semibold text-gray-900 dark:text-white">Alert Routing</h2>
          </div>
          <button
            onClick={() => setShowNewForm((v) => !v)}
            className="flex items-center gap-1.5 rounded-lg bg-vortex-600 px-3 py-1.5 text-xs font-medium text-white shadow-sm hover:bg-vortex-700"
          >
            <Plus className="h-3.5 w-3.5" />
            New Config
          </button>
        </div>

        {showNewForm && (
          <div className="mt-4">
            <NewIncidentForm onClose={() => setShowNewForm(false)} />
          </div>
        )}

        {incidentConfigs.isLoading ? (
          <div className="mt-4 flex justify-center py-4">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
          </div>
        ) : (incidentConfigs.data?.length ?? 0) === 0 ? (
          <p className="mt-4 text-sm text-gray-500 dark:text-gray-400">No alert configurations yet. Create one above to route incidents to Slack, PagerDuty, or webhooks.</p>
        ) : (
          <div className="mt-4 space-y-2">
            {incidentConfigs.data!.map((cfg: IncidentConfig) => (
              <div key={cfg.id} className="flex items-center justify-between rounded-xl border border-gray-200 px-4 py-3 dark:border-gray-700">
                <div className="flex items-center gap-3">
                  <span className={clsx('rounded-full px-2.5 py-0.5 text-xs font-medium', channelColor[cfg.channel] ?? channelColor.webhook)}>
                    {cfg.channel}
                  </span>
                  <div>
                    <p className="text-sm font-medium text-gray-900 dark:text-white">{cfg.name}</p>
                    <p className="text-xs text-gray-500 dark:text-gray-400">
                      {cfg.severity_filter ? `severity: ${cfg.severity_filter}` : 'all severities'}
                      {cfg.dag_filter ? ` · DAGs: ${cfg.dag_filter}` : ''}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <span className={clsx('rounded-full px-2 py-0.5 text-xs font-medium', cfg.enabled ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400' : 'bg-gray-100 text-gray-500 dark:bg-gray-800 dark:text-gray-400')}>
                    {cfg.enabled ? 'enabled' : 'disabled'}
                  </span>
                  <button
                    onClick={() => deleteMut.mutate(cfg.id)}
                    disabled={deleteMut.isPending}
                    className="rounded-lg p-1.5 text-gray-400 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-900/20"
                  >
                    <Trash2 className="h-4 w-4" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Event Feed */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="flex items-center gap-2">
          <Zap className="h-5 w-5 text-vortex-600 dark:text-vortex-400" />
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">Event Feed</h2>
          <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-500 dark:bg-gray-800 dark:text-gray-400">
            last 100 events
          </span>
        </div>
        {auditLog.isLoading ? (
          <div className="mt-4 flex justify-center py-4">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
          </div>
        ) : eventFeed.length === 0 ? (
          <p className="mt-4 text-sm text-gray-500 dark:text-gray-400">No events recorded yet. Trigger a DAG to generate events.</p>
        ) : (
          <div className="mt-4 space-y-1.5">
            {eventFeed.slice(0, 50).map((entry) => (
              <div key={entry.id} className="flex items-start gap-3 rounded-lg px-3 py-2 hover:bg-gray-50 dark:hover:bg-gray-800/50">
                <span className={clsx('mt-0.5 shrink-0 rounded-full px-2 py-0.5 text-xs font-medium', eventActionColor[entry.action] ?? 'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-400')}>
                  {entry.action}
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm text-gray-700 dark:text-gray-300">
                    <span className="font-medium text-gray-900 dark:text-white">{entry.actor}</span>
                    {' → '}
                    {entry.resource_type ?? 'system'}/{entry.resource_id ?? '—'}
                  </p>
                </div>
                <span className="shrink-0 text-xs text-gray-400">{new Date(entry.created_at).toLocaleString()}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
