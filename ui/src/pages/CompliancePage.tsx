import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { complianceApi, type ApprovalRequest } from '../api/client';
import { CheckCircle, XCircle, Clock, ChevronDown, ChevronRight } from 'lucide-react';
import { clsx } from 'clsx';

const statusColor: Record<string, string> = {
  pending: 'bg-amber-100 text-amber-700 dark:bg-amber-500/10 dark:text-amber-400',
  approved: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400',
  rejected: 'bg-red-100 text-red-700 dark:bg-red-500/10 dark:text-red-400',
};

function ApprovalRow({ req }: { req: ApprovalRequest }) {
  const qc = useQueryClient();
  const [comment, setComment] = useState('');
  const [expanded, setExpanded] = useState(false);

  const approveMut = useMutation({
    mutationFn: () => complianceApi.approveRequest(req.id, comment || undefined),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['approval-requests'] }); },
  });
  const rejectMut = useMutation({
    mutationFn: () => complianceApi.rejectRequest(req.id, comment || undefined),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['approval-requests'] }); },
  });

  const isPending = req.status === 'pending';

  return (
    <>
      <tr
        className="cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800/50"
        onClick={() => setExpanded((e) => !e)}
      >
        <td className="px-4 py-3">
          {expanded ? <ChevronDown className="h-4 w-4 text-gray-400" /> : <ChevronRight className="h-4 w-4 text-gray-400" />}
        </td>
        <td className="px-4 py-3 text-sm font-medium text-gray-900 dark:text-gray-100">{req.resource_type}/{req.resource_id}</td>
        <td className="px-4 py-3 text-sm text-gray-600 dark:text-gray-300">{req.requested_by}</td>
        <td className="px-4 py-3">
          <span className={clsx('rounded-full px-2.5 py-0.5 text-xs font-medium', statusColor[req.status] ?? statusColor.pending)}>
            {req.status}
          </span>
        </td>
        <td className="px-4 py-3 text-sm text-gray-500 dark:text-gray-400">{new Date(req.created_at).toLocaleString()}</td>
        <td className="px-4 py-3">
          {isPending && (
            <div className="flex gap-2" onClick={(e) => e.stopPropagation()}>
              <button
                onClick={() => approveMut.mutate()}
                disabled={approveMut.isPending}
                className="flex items-center gap-1 rounded-lg bg-emerald-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-emerald-700 disabled:opacity-50"
              >
                <CheckCircle className="h-3 w-3" /> Approve
              </button>
              <button
                onClick={() => rejectMut.mutate()}
                disabled={rejectMut.isPending}
                className="flex items-center gap-1 rounded-lg bg-red-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-red-700 disabled:opacity-50"
              >
                <XCircle className="h-3 w-3" /> Reject
              </button>
            </div>
          )}
        </td>
      </tr>
      {expanded && (
        <tr className="bg-gray-50 dark:bg-gray-800/30">
          <td colSpan={6} className="px-6 py-3">
            <p className="text-xs text-gray-500 dark:text-gray-400">
              <strong>Description:</strong> {req.change_description ?? '—'}
            </p>
            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
              <strong>Gate:</strong> {req.gate_id}
            </p>
            {isPending && (
              <div className="mt-2 flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
                <input
                  type="text"
                  placeholder="Optional comment…"
                  value={comment}
                  onChange={(e) => setComment(e.target.value)}
                  className="rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-xs text-gray-900 focus:border-ryuo-500 focus:outline-none dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100"
                />
              </div>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

export function CompliancePage() {
  const auditLog = useQuery({ queryKey: ['audit-log'], queryFn: () => complianceApi.auditLog(50) });
  const gates = useQuery({ queryKey: ['approval-gates'], queryFn: complianceApi.approvalGates });
  const requests = useQuery({ queryKey: ['approval-requests'], queryFn: () => complianceApi.approvalRequests() });

  const pendingCount = requests.data?.filter((r) => r.status === 'pending').length ?? 0;

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Compliance &amp; Governance</h1>

      {/* Framework badges */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        {(['SOC 2', 'HIPAA', 'PCI-DSS'] as const).map((fw) => (
          <div key={fw} className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <h2 className="text-lg font-semibold text-gray-900 dark:text-white">{fw}</h2>
            <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">Compliance framework status</p>
            <span className="mt-4 inline-flex items-center gap-1.5 rounded-full bg-emerald-100 px-3 py-1 text-xs font-medium text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400">
              <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" /> Active
            </span>
          </div>
        ))}
      </div>

      {/* Approval Gates */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white">Approval Gates</h2>
          <span className="rounded-full bg-amber-100 px-2.5 py-0.5 text-xs font-medium text-amber-700 dark:bg-amber-500/10 dark:text-amber-400">
            {gates.data?.length ?? 0} configured
          </span>
        </div>
        {gates.isLoading ? (
          <div className="mt-4 flex justify-center py-4">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
          </div>
        ) : (gates.data?.length ?? 0) === 0 ? (
          <p className="mt-4 text-sm text-gray-500 dark:text-gray-400">No approval gates configured.</p>
        ) : (
          <div className="mt-4 grid gap-3 lg:grid-cols-2">
            {gates.data!.map((gate) => (
              <div key={gate.id} className="rounded-lg border border-gray-200 p-4 dark:border-gray-700">
                <div className="flex items-start justify-between">
                  <div>
                    <p className="text-sm font-medium text-gray-900 dark:text-white">{gate.name}</p>
                    <p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">{gate.resource_type} · pattern: <code>{gate.resource_pattern}</code></p>
                  </div>
                  <span className={clsx('rounded-full px-2 py-0.5 text-xs font-medium', gate.enabled ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400' : 'bg-gray-100 text-gray-500 dark:bg-gray-800 dark:text-gray-400')}>
                    {gate.enabled ? 'enabled' : 'disabled'}
                  </span>
                </div>
                <p className="mt-2 text-xs text-gray-500 dark:text-gray-400">Requires {gate.required_approvers} approver(s) from: {gate.approver_roles.join(', ')}</p>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Approval Requests */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <div className="flex items-center gap-3">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white">Approval Requests</h2>
          {pendingCount > 0 && (
            <span className="flex items-center gap-1 rounded-full bg-amber-100 px-2.5 py-0.5 text-xs font-medium text-amber-700 dark:bg-amber-500/10 dark:text-amber-400">
              <Clock className="h-3 w-3" /> {pendingCount} pending
            </span>
          )}
        </div>
        {requests.isLoading ? (
          <div className="mt-4 flex justify-center py-4">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
          </div>
        ) : (requests.data?.length ?? 0) === 0 ? (
          <p className="mt-4 text-sm text-gray-500 dark:text-gray-400">No approval requests found.</p>
        ) : (
          <div className="mt-4 overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr>
                  <th className="w-8 px-4 py-2" />
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Resource</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Requested By</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Status</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Created</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-gray-800">
                {requests.data!.map((req) => <ApprovalRow key={req.id} req={req} />)}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Audit Log */}
      <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white">Audit Log</h2>
        {auditLog.isLoading ? (
          <div className="mt-4 flex items-center justify-center py-8">
            <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
          </div>
        ) : auditLog.data && auditLog.data.length > 0 ? (
          <div className="mt-4 overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
              <thead>
                <tr>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Action</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Actor</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Resource</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500 dark:text-gray-400">Time</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-gray-800">
                {auditLog.data.map((entry) => (
                  <tr key={entry.id} className="hover:bg-gray-50 dark:hover:bg-gray-800/50">
                    <td className="px-4 py-2 text-sm font-medium text-gray-900 dark:text-gray-100">{entry.action}</td>
                    <td className="px-4 py-2 text-sm text-gray-600 dark:text-gray-300">{entry.actor}</td>
                    <td className="px-4 py-2 text-sm text-gray-500 dark:text-gray-400">
                      {entry.resource_type ?? 'system'}/{entry.resource_id ?? '—'}
                    </td>
                    <td className="px-4 py-2 text-sm text-gray-500 dark:text-gray-400">{new Date(entry.created_at).toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="mt-4 text-sm text-gray-500 dark:text-gray-400">No audit entries found.</p>
        )}
      </div>
    </div>
  );
}

