import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Database, Search } from 'lucide-react';
import { lineageApi, dagsApi, type LineageDataset } from '../api/client';

export function LineagePage() {
  const [selectedDag, setSelectedDag] = useState<string>('');

  const { data: datasets = [], isLoading: datasetsLoading } = useQuery({
    queryKey: ['lineage-datasets'],
    queryFn: () => lineageApi.datasets(100),
  });

  const { data: dags = [] } = useQuery({
    queryKey: ['dags'],
    queryFn: dagsApi.list,
  });

  const { data: events = [], isLoading: eventsLoading } = useQuery({
    queryKey: ['lineage-events', selectedDag],
    queryFn: () => lineageApi.events(selectedDag, 50),
    enabled: !!selectedDag,
  });

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Data Lineage</h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            OpenLineage-compliant dataset tracking across your pipelines.
          </p>
        </div>
      </div>

      {/* Dataset grid */}
      <div>
        <h2 className="mb-3 text-sm font-semibold text-gray-700 dark:text-gray-300">
          Tracked Datasets ({datasets.length})
        </h2>
        {datasetsLoading ? (
          <div className="flex items-center justify-center py-12">
            <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
          </div>
        ) : datasets.length === 0 ? (
          <div className="rounded-xl border border-dashed border-gray-300 p-12 text-center dark:border-gray-700">
            <Database className="mx-auto h-10 w-10 text-gray-300 dark:text-gray-700" />
            <p className="mt-3 text-sm text-gray-500 dark:text-gray-400">
              No datasets tracked yet. Lineage events are emitted automatically as tasks run.
            </p>
          </div>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {datasets.map((ds: LineageDataset, i) => (
              <div
                key={ds.id ?? i}
                className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-800 dark:bg-gray-900"
              >
                <div className="flex items-start gap-3">
                  <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-blue-50 dark:bg-blue-950/30">
                    <Database className="h-4 w-4 text-blue-600 dark:text-blue-400" />
                  </div>
                  <div className="min-w-0">
                    <p className="truncate font-medium text-gray-900 dark:text-white">{ds.name}</p>
                    <p className="truncate text-xs text-gray-500 dark:text-gray-400">{ds.namespace}</p>
                    {ds.created_at && (
                      <p className="mt-1 text-xs text-gray-400 dark:text-gray-600">
                        {new Date(ds.created_at).toLocaleDateString()}
                      </p>
                    )}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Lineage events by DAG */}
      <div className="rounded-xl border border-gray-200 bg-white shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="flex flex-wrap items-center gap-3 border-b border-gray-200 px-6 py-4 dark:border-gray-800">
          <h2 className="text-base font-semibold text-gray-900 dark:text-white">Lineage Events</h2>
          <div className="flex flex-1 items-center gap-2">
            <Search className="h-4 w-4 text-gray-400" />
            <select
              value={selectedDag}
              onChange={(e) => setSelectedDag(e.target.value)}
              className="flex-1 rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
            >
              <option value="">— Select a DAG —</option>
              {dags.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.id}
                </option>
              ))}
            </select>
          </div>
        </div>

        {!selectedDag ? (
          <div className="p-12 text-center text-sm text-gray-400">
            Select a DAG above to view its lineage events.
          </div>
        ) : eventsLoading ? (
          <div className="flex items-center justify-center py-12">
            <div className="h-6 w-6 animate-spin rounded-full border-2 border-vortex-500 border-t-transparent" />
          </div>
        ) : events.length === 0 ? (
          <div className="p-8 text-center text-sm text-gray-400">
            No lineage events recorded for <strong>{selectedDag}</strong> yet.
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr className="bg-gray-50 dark:bg-gray-800/50">
                  {['Event Type', 'Run ID', 'Job', 'Event Time'].map((h) => (
                    <th key={h} className="px-5 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                {(events as Record<string, unknown>[]).map((ev, i) => (
                  <tr key={i} className="hover:bg-gray-50 dark:hover:bg-gray-800/50">
                    <td className="px-5 py-3">
                      <span className={`inline-flex rounded-full px-2.5 py-0.5 text-xs font-medium ${
                        String(ev.event_type) === 'COMPLETE'
                          ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400'
                          : String(ev.event_type) === 'FAIL'
                          ? 'bg-red-100 text-red-700 dark:bg-red-500/10 dark:text-red-400'
                          : 'bg-blue-100 text-blue-700 dark:bg-blue-500/10 dark:text-blue-400'
                      }`}>
                        {String(ev.event_type ?? 'UNKNOWN')}
                      </span>
                    </td>
                    <td className="whitespace-nowrap px-5 py-3 font-mono text-xs text-gray-600 dark:text-gray-400">
                      {ev.run_id ? String(ev.run_id).slice(0, 12) + '…' : '—'}
                    </td>
                    <td className="px-5 py-3 text-sm text-gray-700 dark:text-gray-300">
                      {ev.job_name ? String(ev.job_name) : selectedDag}
                    </td>
                    <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                      {ev.event_time ? new Date(String(ev.event_time)).toLocaleString() : '—'}
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
