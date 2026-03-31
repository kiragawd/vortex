export function SettingsPage() {
  const providers = [
    { name: 'Local Auth', enabled: true },
    { name: 'OIDC', enabled: false },
    { name: 'SAML', enabled: false },
    { name: 'LDAP', enabled: false },
  ];

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Settings</h1>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white">General</h2>
          <dl className="mt-4 space-y-4">
            <div>
              <dt className="text-sm font-medium text-gray-500 dark:text-gray-400">Instance Name</dt>
              <dd className="mt-1">
                <input
                  type="text"
                  defaultValue="Vortex Production"
                  className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                  readOnly
                />
              </dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500 dark:text-gray-400">Timezone</dt>
              <dd className="mt-1">
                <input
                  type="text"
                  defaultValue="UTC"
                  className="w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                  readOnly
                />
              </dd>
            </div>
          </dl>
        </div>

        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white">Authentication Providers</h2>
          <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
            Configure SSO, OIDC, SAML, and LDAP providers via the{' '}
            <code className="rounded bg-gray-100 px-1.5 py-0.5 font-mono text-xs dark:bg-gray-800">/api/auth/providers</code> API.
          </p>
          <div className="mt-4 space-y-2">
            {providers.map((p) => (
              <div key={p.name} className="flex items-center justify-between rounded-lg bg-gray-50 px-3 py-2 dark:bg-gray-800">
                <span className="text-sm text-gray-700 dark:text-gray-300">{p.name}</span>
                {p.enabled ? (
                  <span className="inline-flex items-center gap-1 rounded-full bg-emerald-100 px-2 py-0.5 text-xs font-medium text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400">
                    <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                    Enabled
                  </span>
                ) : (
                  <span className="inline-flex rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-500 dark:bg-gray-700 dark:text-gray-400">
                    Not configured
                  </span>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
