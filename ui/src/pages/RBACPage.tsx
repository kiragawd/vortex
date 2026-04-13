import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2, ShieldOff } from 'lucide-react';
import { rbacApi, type RbacRole, type ApiToken, type IpAllowlistRule } from '../api/client';

type Tab = 'roles' | 'tokens' | 'network';

const TOKEN_SCOPE_OPTIONS = [
  { value: 'dag:read',         label: 'dag:read',         description: 'View DAGs and runs' },
  { value: 'dag:write',        label: 'dag:write',        description: 'Create and modify DAGs' },
  { value: 'dag:execute',      label: 'dag:execute',      description: 'Trigger DAG runs' },
  { value: 'dag:delete',       label: 'dag:delete',       description: 'Delete DAGs' },
  { value: 'secrets:read',     label: 'secrets:read',     description: 'Read secrets (masked)' },
  { value: 'secrets:write',    label: 'secrets:write',    description: 'Create/update secrets' },
  { value: 'connectors:read',  label: 'connectors:read',  description: 'View connectors' },
  { value: 'connectors:write', label: 'connectors:write', description: 'Manage connectors' },
  { value: 'tokens:manage',    label: 'tokens:manage',    description: 'Manage API tokens' },
  { value: 'admin',            label: 'admin',            description: 'Full system access' },
] as const;

export function RBACPage() {
  const [tab, setTab] = useState<Tab>('roles');
  const [newCidr, setNewCidr] = useState('');
  const [newCidrDesc, setNewCidrDesc] = useState('');
  const [tokenName, setTokenName] = useState('');
  const [tokenScopes, setTokenScopes] = useState<string[]>(['dag:read']);
  const [scopesOpen, setScopesOpen] = useState(false);
  const [createdToken, setCreatedToken] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const { data: roles = [], isLoading: rolesLoading } = useQuery({
    queryKey: ['rbac-roles'],
    queryFn: rbacApi.roles,
  });

  const { data: tokens = [], isLoading: tokensLoading } = useQuery({
    queryKey: ['api-tokens'],
    queryFn: rbacApi.tokens,
    enabled: tab === 'tokens',
  });

  const { data: ipRules = [], isLoading: ipLoading } = useQuery({
    queryKey: ['ip-allowlist'],
    queryFn: rbacApi.ipAllowlist,
    enabled: tab === 'network',
  });

  const revokeTokenMutation = useMutation({
    mutationFn: (id: string) => rbacApi.revokeToken(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['api-tokens'] }),
  });

  const createTokenMutation = useMutation({
    mutationFn: () =>
      rbacApi.createToken({
        name: tokenName || 'New Token',
        scopes: tokenScopes,
      }),
    onSuccess: (data) => {
      setCreatedToken(data.token);
      setTokenName('');
      queryClient.invalidateQueries({ queryKey: ['api-tokens'] });
    },
  });

  const addIpMutation = useMutation({
    mutationFn: () => rbacApi.addIpRule(newCidr, newCidrDesc || undefined),
    onSuccess: () => {
      setNewCidr('');
      setNewCidrDesc('');
      queryClient.invalidateQueries({ queryKey: ['ip-allowlist'] });
    },
  });

  const deleteIpMutation = useMutation({
    mutationFn: (id: string) => rbacApi.deleteIpRule(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['ip-allowlist'] }),
  });

  const tabs: { id: Tab; label: string }[] = [
    { id: 'roles', label: 'Roles & Permissions' },
    { id: 'tokens', label: 'API Tokens' },
    { id: 'network', label: 'IP Allowlist' },
  ];

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">RBAC &amp; Access Control</h1>

      {/* Tabs */}
      <div className="border-b border-gray-200 dark:border-gray-800">
        <nav className="-mb-px flex gap-6">
          {tabs.map((t) => (
            <button
              key={t.id}
              onClick={() => setTab(t.id)}
              className={`pb-3 text-sm font-medium transition-colors ${
                tab === t.id
                  ? 'border-b-2 border-ryuo-600 text-ryuo-600 dark:border-ryuo-400 dark:text-ryuo-400'
                  : 'text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200'
              }`}
            >
              {t.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Roles tab */}
      {tab === 'roles' && (
        <div className="space-y-4">
          {rolesLoading ? (
            <div className="flex items-center justify-center py-12">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
            </div>
          ) : roles.length === 0 ? (
            <div className="rounded-xl border border-dashed border-gray-300 p-12 text-center text-sm text-gray-400 dark:border-gray-700">
              No roles defined. Roles are seeded from the database migrations.
            </div>
          ) : (
            <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
              {roles.map((role: RbacRole) => (
                <div key={role.id} className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-800 dark:bg-gray-900">
                  <div className="flex items-start justify-between">
                    <div>
                      <p className="font-semibold text-gray-900 dark:text-white capitalize">{role.name}</p>
                      <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                        {role.description || 'No description'}
                      </p>
                    </div>
                    <span className="rounded-full bg-ryuo-50 px-2 py-0.5 text-xs font-medium text-ryuo-700 dark:bg-ryuo-950 dark:text-ryuo-300">
                      {role.is_system ? 'system' : 'custom'}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* API Tokens tab */}
      {tab === 'tokens' && (
        <div className="space-y-6">
          {/* Create token form */}
          <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <h2 className="text-base font-semibold text-gray-900 dark:text-white">Create API Token</h2>
            <div className="mt-4 flex flex-wrap gap-3">
              <input
                type="text"
                placeholder="Token name"
                value={tokenName}
                onChange={(e) => setTokenName(e.target.value)}
                className="flex-1 min-w-[160px] rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 placeholder-gray-400 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
              />
              {/* Scope multi-select */}
              <div className="relative min-w-[200px]">
                <button
                  type="button"
                  onClick={() => setScopesOpen((o) => !o)}
                  className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-left text-sm text-gray-900 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                  {tokenScopes.length === 0
                    ? 'Select scopes…'
                    : tokenScopes.join(', ')}
                  <span className="float-right text-gray-400">▾</span>
                </button>
                {scopesOpen && (
                  <div className="absolute z-20 mt-1 w-full rounded-xl border border-gray-200 bg-white shadow-lg dark:border-gray-700 dark:bg-gray-900">
                    {TOKEN_SCOPE_OPTIONS.map((opt) => (
                      <label
                        key={opt.value}
                        className="flex cursor-pointer items-start gap-3 px-4 py-2.5 hover:bg-gray-50 dark:hover:bg-gray-800"
                      >
                        <input
                          type="checkbox"
                          checked={tokenScopes.includes(opt.value)}
                          onChange={(e) =>
                            setTokenScopes((prev) =>
                              e.target.checked
                                ? [...prev, opt.value]
                                : prev.filter((s) => s !== opt.value)
                            )
                          }
                          className="mt-0.5 h-4 w-4 rounded border-gray-300 text-ryuo-600"
                        />
                        <span>
                          <span className="block font-mono text-xs font-semibold text-gray-800 dark:text-gray-200">
                            {opt.label}
                          </span>
                          <span className="block text-xs text-gray-500 dark:text-gray-400">
                            {opt.description}
                          </span>
                        </span>
                      </label>
                    ))}
                    <div className="border-t border-gray-100 px-4 py-2 dark:border-gray-800">
                      <button
                        type="button"
                        onClick={() => setScopesOpen(false)}
                        className="text-xs text-ryuo-600 hover:underline dark:text-ryuo-400"
                      >
                        Done
                      </button>
                    </div>
                  </div>
                )}
              </div>
              <button
                onClick={() => createTokenMutation.mutate()}
                disabled={createTokenMutation.isPending}
                className="inline-flex items-center gap-1.5 rounded-lg bg-ryuo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-ryuo-700 disabled:opacity-50"
              >
                <Plus className="h-4 w-4" />
                Generate
              </button>
            </div>
            {createdToken && (
              <div className="mt-4 rounded-lg border border-emerald-200 bg-emerald-50 p-4 dark:border-emerald-800 dark:bg-emerald-950/20">
                <p className="text-xs font-semibold text-emerald-800 dark:text-emerald-300">
                  ⚠ Copy this token now — it will not be shown again
                </p>
                <pre className="mt-2 break-all font-mono text-xs text-emerald-700 dark:text-emerald-400">
                  {createdToken}
                </pre>
                <button
                  onClick={() => setCreatedToken(null)}
                  className="mt-2 text-xs text-emerald-600 hover:text-emerald-800 dark:text-emerald-400"
                >
                  Dismiss
                </button>
              </div>
            )}
          </div>

          {/* Token list */}
          {tokensLoading ? (
            <div className="flex items-center justify-center py-8">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
            </div>
          ) : tokens.length === 0 ? (
            <div className="rounded-xl border border-dashed border-gray-300 p-8 text-center text-sm text-gray-400 dark:border-gray-700">
              No API tokens. Generate one above.
            </div>
          ) : (
            <div className="overflow-x-auto rounded-xl border border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900">
              <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
                <thead>
                  <tr className="bg-gray-50 dark:bg-gray-800/50">
                    {['Name', 'Scopes', 'Created', 'Expires', 'Status', ''].map((h) => (
                      <th key={h} className="px-5 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                  {tokens.map((tok: ApiToken) => (
                    <tr key={tok.id} className="hover:bg-gray-50 dark:hover:bg-gray-800/50">
                      <td className="whitespace-nowrap px-5 py-3 text-sm font-medium text-gray-900 dark:text-gray-100">
                        {tok.name}
                      </td>
                      <td className="px-5 py-3">
                        <div className="flex flex-wrap gap-1">
                          {(tok.scopes ?? []).map((s) => (
                            <span key={s} className="rounded bg-gray-100 px-1.5 py-0.5 font-mono text-xs dark:bg-gray-800">
                              {s}
                            </span>
                          ))}
                        </div>
                      </td>
                      <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                        {tok.created_at ? new Date(tok.created_at).toLocaleDateString() : '—'}
                      </td>
                      <td className="whitespace-nowrap px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                        {tok.expires_at ? new Date(tok.expires_at).toLocaleDateString() : 'Never'}
                      </td>
                      <td className="px-5 py-3">
                        {tok.revoked ? (
                          <span className="inline-flex items-center gap-1 rounded-full bg-red-100 px-2 py-0.5 text-xs text-red-700 dark:bg-red-500/10 dark:text-red-400">
                            Revoked
                          </span>
                        ) : (
                          <span className="inline-flex items-center gap-1 rounded-full bg-emerald-100 px-2 py-0.5 text-xs text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400">
                            Active
                          </span>
                        )}
                      </td>
                      <td className="px-5 py-3 text-right">
                        {!tok.revoked && (
                          <button
                            onClick={() => revokeTokenMutation.mutate(tok.id)}
                            disabled={revokeTokenMutation.isPending}
                            className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950/20 disabled:opacity-50"
                          >
                            <ShieldOff className="h-3.5 w-3.5" />
                            Revoke
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {/* IP Allowlist tab */}
      {tab === 'network' && (
        <div className="space-y-6">
          {/* Add rule form */}
          <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
            <h2 className="text-base font-semibold text-gray-900 dark:text-white">Add IP Allowlist Rule</h2>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
              Restrict API access to specific CIDR ranges. Leave empty to allow all IPs.
            </p>
            <div className="mt-4 flex flex-wrap gap-3">
              <input
                type="text"
                placeholder="CIDR (e.g. 10.0.0.0/8)"
                value={newCidr}
                onChange={(e) => setNewCidr(e.target.value)}
                className="flex-1 min-w-[160px] rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm font-mono text-gray-900 placeholder-gray-400 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
              />
              <input
                type="text"
                placeholder="Description (optional)"
                value={newCidrDesc}
                onChange={(e) => setNewCidrDesc(e.target.value)}
                className="flex-1 min-w-[160px] rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 placeholder-gray-400 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
              />
              <button
                onClick={() => addIpMutation.mutate()}
                disabled={addIpMutation.isPending || !newCidr}
                className="inline-flex items-center gap-1.5 rounded-lg bg-ryuo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-ryuo-700 disabled:opacity-50"
              >
                <Plus className="h-4 w-4" />
                Add Rule
              </button>
            </div>
          </div>

          {/* Rules list */}
          {ipLoading ? (
            <div className="flex items-center justify-center py-8">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-ryuo-500 border-t-transparent" />
            </div>
          ) : ipRules.length === 0 ? (
            <div className="rounded-xl border border-dashed border-gray-300 p-8 text-center text-sm text-gray-400 dark:border-gray-700">
              No IP allowlist rules. All IPs are permitted (open access).
            </div>
          ) : (
            <div className="overflow-x-auto rounded-xl border border-gray-200 bg-white dark:border-gray-800 dark:bg-gray-900">
              <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-800">
                <thead>
                  <tr className="bg-gray-50 dark:bg-gray-800/50">
                    {['CIDR', 'Description', 'Status', ''].map((h) => (
                      <th key={h} className="px-5 py-3 text-left text-xs font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-100 dark:divide-gray-800">
                  {ipRules.map((rule: IpAllowlistRule) => (
                    <tr key={rule.id} className="hover:bg-gray-50 dark:hover:bg-gray-800/50">
                      <td className="whitespace-nowrap px-5 py-3 font-mono text-sm text-gray-900 dark:text-gray-100">
                        {rule.cidr}
                      </td>
                      <td className="px-5 py-3 text-sm text-gray-500 dark:text-gray-400">
                        {rule.description ?? '—'}
                      </td>
                      <td className="px-5 py-3">
                        <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${
                          rule.enabled
                            ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400'
                            : 'bg-gray-100 text-gray-500 dark:bg-gray-700 dark:text-gray-400'
                        }`}>
                          {rule.enabled ? 'Enabled' : 'Disabled'}
                        </span>
                      </td>
                      <td className="px-5 py-3 text-right">
                        <button
                          onClick={() => deleteIpMutation.mutate(rule.id)}
                          disabled={deleteIpMutation.isPending}
                          className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950/20 disabled:opacity-50"
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                          Delete
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

