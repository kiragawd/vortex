import { useQuery } from '@tanstack/react-query';
import { healthApi, swarmApi } from '../api/client';

export function MonitoringPage() {
  const health = useQuery({
    queryKey: ['health'],
    queryFn: healthApi.check,
    refetchInterval: 5000,
  });
  const swarmStatus = useQuery({
    queryKey: ['swarm-status'],
    queryFn: swarmApi.status,
    refetchInterval: 5000,
  });

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Monitoring</h1>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        {/* System Health */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">System Health</h2>
          <dl className="mt-4 space-y-3">
            <div>
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Status</dt>
              <dd className="mt-1">
                {health.data?.status ? (
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-emerald-100 px-3 py-1 text-xs font-medium text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400">
                    <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" />
                    {health.data.status}
                  </span>
                ) : health.isLoading ? (
                  <span className="text-sm text-gray-400">Checking…</span>
                ) : (
                  <span className="inline-flex items-center gap-1.5 rounded-full bg-red-100 px-3 py-1 text-xs font-medium text-red-700 dark:bg-red-500/10 dark:text-red-400">
                    Unreachable
                  </span>
                )}
              </dd>
            </div>
            <div>
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Version</dt>
              <dd className="mt-1 rounded-md bg-gray-100 px-2 py-0.5 font-mono text-sm text-gray-900 dark:bg-gray-800 dark:text-gray-100">
                {health.data?.version ?? '…'}
              </dd>
            </div>
            <div>
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Database</dt>
              <dd className="mt-1 text-sm text-gray-700 dark:text-gray-300">
                {health.data?.db ?? '…'}
              </dd>
            </div>
          </dl>
        </div>

        {/* Swarm Status */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">Swarm / Executor</h2>
          <dl className="mt-4 space-y-3">
            <div>
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Mode</dt>
              <dd className="mt-1 text-sm font-medium text-gray-900 dark:text-white">
                {swarmStatus.isLoading ? '…' : swarmStatus.data?.enabled ? 'Swarm (distributed)' : 'Local executor'}
              </dd>
            </div>
            <div>
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Active Workers</dt>
              <dd className="mt-1 text-2xl font-bold text-ryuo-600 dark:text-ryuo-400">
                {swarmStatus.isLoading ? '…' : (swarmStatus.data?.active_workers ?? 0)}
              </dd>
            </div>
            <div>
              <dt className="text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">Queue Depth</dt>
              <dd className="mt-1 text-sm text-gray-700 dark:text-gray-300">
                {swarmStatus.isLoading ? '…' : (swarmStatus.data?.queue_depth ?? 0)} tasks pending
              </dd>
            </div>
          </dl>
        </div>

        {/* Metrics endpoints */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">Observability Stack</h2>
          <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
            Prometheus metrics at{' '}
            <code className="rounded bg-gray-100 px-1.5 py-0.5 font-mono text-xs dark:bg-gray-800">/metrics</code>.
          </p>
          <div className="mt-4 space-y-2">
            {[
              { label: 'Prometheus', port: ':9090' },
              { label: 'Grafana', port: ':3000' },
              { label: 'OpenTelemetry', port: ':4317 (gRPC)' },
              { label: 'Jaeger UI', port: ':16686' },
            ].map((s) => (
              <div key={s.label} className="flex items-center justify-between rounded-lg bg-gray-50 px-3 py-2 dark:bg-gray-800">
                <span className="text-sm text-gray-700 dark:text-gray-300">{s.label}</span>
                <span className="rounded bg-gray-200 px-1.5 py-0.5 font-mono text-xs text-gray-500 dark:bg-gray-700 dark:text-gray-400">
                  {s.port}
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* OpenAPI */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <h2 className="text-base font-semibold text-gray-900 dark:text-white">API Documentation</h2>
        <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
          The full OpenAPI 3.1 specification is available at{' '}
          <code className="rounded bg-gray-100 px-1.5 py-0.5 font-mono text-xs dark:bg-gray-800">/api/openapi.json</code>.
          Import it into Swagger UI, Postman, or any OpenAPI-compatible client.
        </p>
        <a
          href="/api/openapi.json"
          target="_blank"
          rel="noopener noreferrer"
          className="mt-4 inline-flex items-center gap-1.5 rounded-lg bg-ryuo-600 px-4 py-2 text-sm font-medium text-white shadow-sm transition-colors hover:bg-ryuo-700"
        >
          View OpenAPI Spec →
        </a>
      </div>
    </div>
  );
}
