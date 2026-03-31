-- ============================================================================
-- Vortex Core Features (Consolidated Migration #2)
-- Covers: IAM/SSO, Lineage, Compliance, RBAC, API Tokens, IP Allowlist
-- ============================================================================

-- ─── IAM / SSO / OIDC / SAML / LDAP ──────────────────────────────────

CREATE TABLE IF NOT EXISTS auth_providers (
    id            TEXT PRIMARY KEY,
    provider_type TEXT NOT NULL CHECK (provider_type IN ('oidc', 'saml', 'ldap', 'local')),
    name          TEXT NOT NULL,
    config        TEXT NOT NULL DEFAULT '{}',
    enabled       BOOLEAN NOT NULL DEFAULT TRUE,
    priority      INTEGER NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_sessions (
    session_id    TEXT PRIMARY KEY,
    username      TEXT NOT NULL REFERENCES users(username) ON DELETE CASCADE,
    provider_id   TEXT REFERENCES auth_providers(id),
    access_token  TEXT,
    refresh_token TEXT,
    id_token      TEXT,
    expires_at    TIMESTAMPTZ NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address    TEXT,
    user_agent    TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_username ON user_sessions(username);
CREATE INDEX IF NOT EXISTS idx_sessions_expires  ON user_sessions(expires_at);

CREATE TABLE IF NOT EXISTS ldap_group_mappings (
    id          TEXT PRIMARY KEY,
    ldap_group  TEXT NOT NULL,
    team_id     TEXT REFERENCES teams(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'Viewer',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(ldap_group, team_id)
);

INSERT INTO auth_providers (id, provider_type, name, config, enabled, priority)
VALUES ('local', 'local', 'Local Database', '{}', true, 0)
ON CONFLICT (id) DO NOTHING;

-- ─── Observability & Data Lineage ───────────────────────────────────

CREATE TABLE IF NOT EXISTS lineage_events (
    id            TEXT PRIMARY KEY,
    event_type    TEXT NOT NULL CHECK (event_type IN ('START', 'RUNNING', 'COMPLETE', 'FAIL', 'ABORT')),
    event_time    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    run_id        TEXT NOT NULL,
    dag_id        TEXT NOT NULL,
    task_id       TEXT,
    job_namespace TEXT NOT NULL DEFAULT 'vortex',
    job_name      TEXT NOT NULL,
    producer      TEXT NOT NULL DEFAULT 'vortex',
    inputs        JSONB NOT NULL DEFAULT '[]',
    outputs       JSONB NOT NULL DEFAULT '[]',
    facets        JSONB NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_lineage_run_id     ON lineage_events(run_id);
CREATE INDEX IF NOT EXISTS idx_lineage_dag_id     ON lineage_events(dag_id);
CREATE INDEX IF NOT EXISTS idx_lineage_event_time ON lineage_events(event_time);

CREATE TABLE IF NOT EXISTS lineage_datasets (
    id          TEXT PRIMARY KEY,
    namespace   TEXT NOT NULL,
    name        TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'unknown',
    facets      JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(namespace, name)
);

CREATE TABLE IF NOT EXISTS incident_configs (
    id         TEXT PRIMARY KEY,
    team_id    TEXT REFERENCES teams(id) ON DELETE CASCADE,
    provider   TEXT NOT NULL CHECK (provider IN ('pagerduty', 'opsgenie', 'datadog', 'webhook')),
    name       TEXT NOT NULL,
    config     JSONB NOT NULL DEFAULT '{}',
    enabled    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_incident_team ON incident_configs(team_id);

-- ─── Compliance, Governance & Change Management ───────────────────

CREATE TABLE IF NOT EXISTS audit_log (
    id            BIGSERIAL PRIMARY KEY,
    event_type    TEXT,
    actor         TEXT NOT NULL,
    actor_ip      TEXT,
    resource_type TEXT,
    resource_id   TEXT,
    action        TEXT NOT NULL,
    details       JSONB DEFAULT '{}',
    team_id       TEXT,
    timestamp     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    target_type   TEXT NOT NULL DEFAULT '',
    target_id     TEXT NOT NULL DEFAULT '',
    metadata      TEXT NOT NULL DEFAULT '{}',
    ip_address    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_actor        ON audit_log(actor);
CREATE INDEX IF NOT EXISTS idx_audit_action       ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp    ON audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_log_event_type ON audit_log(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_log_resource ON audit_log(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_created  ON audit_log(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_log_team     ON audit_log(team_id);

CREATE TABLE IF NOT EXISTS approval_gates (
    id                 TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    name               TEXT NOT NULL,
    resource_type      TEXT NOT NULL,
    resource_pattern   TEXT NOT NULL,
    required_approvers INT NOT NULL DEFAULT 1,
    approver_roles     TEXT[] NOT NULL DEFAULT '{}',
    enabled            BOOLEAN NOT NULL DEFAULT TRUE,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS approval_requests (
    id                 TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    gate_id            TEXT NOT NULL REFERENCES approval_gates(id),
    requester          TEXT NOT NULL,
    resource_type      TEXT NOT NULL,
    resource_id        TEXT NOT NULL,
    change_description TEXT,
    change_diff        JSONB DEFAULT '{}',
    status             TEXT NOT NULL DEFAULT 'pending',
    approvals          JSONB DEFAULT '[]',
    rejections         JSONB DEFAULT '[]',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at        TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_approval_requests_status ON approval_requests(status);
CREATE INDEX IF NOT EXISTS idx_approval_requests_gate   ON approval_requests(gate_id);

CREATE TABLE IF NOT EXISTS retention_policies (
    id                TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    name              TEXT NOT NULL,
    target_table      TEXT NOT NULL UNIQUE,
    retention_days    INT NOT NULL,
    delete_batch_size INT NOT NULL DEFAULT 1000,
    enabled           BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at       TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS compliance_controls (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    framework   TEXT NOT NULL,
    control_id  TEXT NOT NULL,
    description TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'not_assessed',
    evidence    JSONB DEFAULT '{}',
    assessed_by TEXT,
    assessed_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(framework, control_id)
);

-- ─── Fine-Grained RBAC, Token Scoping & Network Security ─────────────

CREATE TABLE IF NOT EXISTS rbac_permissions (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    category    TEXT NOT NULL DEFAULT 'general'
);

CREATE TABLE IF NOT EXISTS rbac_roles (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    is_system   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS rbac_role_permissions (
    role_id       TEXT NOT NULL REFERENCES rbac_roles(id) ON DELETE CASCADE,
    permission_id TEXT NOT NULL REFERENCES rbac_permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE IF NOT EXISTS rbac_user_roles (
    user_id    TEXT NOT NULL,
    role_id    TEXT NOT NULL REFERENCES rbac_roles(id) ON DELETE CASCADE,
    team_id    TEXT,
    granted_by TEXT,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_rbac_user_roles_unique ON rbac_user_roles(user_id, role_id, COALESCE(team_id, '__global__'));
CREATE INDEX IF NOT EXISTS idx_rbac_user_roles_user         ON rbac_user_roles(user_id);

CREATE TABLE IF NOT EXISTS api_tokens (
    id           TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL,
    user_id      TEXT NOT NULL,
    scopes       TEXT[] NOT NULL DEFAULT '{}',
    team_id      TEXT,
    expires_at   TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked      BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_api_tokens_user       ON api_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_api_tokens_hash       ON api_tokens(token_hash);
CREATE INDEX IF NOT EXISTS idx_api_tokens_expires_at ON api_tokens(expires_at);

CREATE TABLE IF NOT EXISTS ip_allowlist (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    cidr        TEXT NOT NULL,
    description TEXT,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ─── RBAC Seed Data ─────────────────────────────────────────────────────────

INSERT INTO rbac_permissions (id, name, description, category) VALUES
    ('perm_dag_read',     'dag.read',          'View DAGs and their runs',      'dag'),
    ('perm_dag_write',    'dag.write',         'Create and modify DAGs',        'dag'),
    ('perm_dag_execute',  'dag.execute',       'Trigger DAG runs',              'dag'),
    ('perm_dag_delete',   'dag.delete',        'Delete DAGs',                   'dag'),
    ('perm_admin_users',  'admin.users',       'Manage users and roles',        'admin'),
    ('perm_admin_system', 'admin.system',      'System configuration',          'admin'),
    ('perm_secrets_read', 'secrets.read',      'Read secrets (masked)',         'secrets'),
    ('perm_secrets_write','secrets.write',     'Create and update secrets',     'secrets'),
    ('perm_conn_read',    'connectors.read',   'View connectors',              'connectors'),
    ('perm_conn_write',   'connectors.write',  'Manage connectors',            'connectors'),
    ('perm_audit_read',   'audit.read',        'View audit logs',              'compliance'),
    ('perm_compliance',   'compliance.manage', 'Manage compliance controls',   'compliance')
ON CONFLICT (name) DO NOTHING;

INSERT INTO rbac_roles (id, name, description, is_system) VALUES
    ('role_admin',  'Admin',  'Full system access',                          TRUE),
    ('role_editor', 'Editor', 'Read/write DAGs and connectors',             TRUE),
    ('role_viewer', 'Viewer', 'Read-only access to DAGs and runs',          TRUE),
    ('role_ops',    'Ops',    'Operational access: execute, secrets, audit', TRUE)
ON CONFLICT (name) DO NOTHING;

INSERT INTO rbac_role_permissions (role_id, permission_id)
SELECT 'role_admin', id FROM rbac_permissions
ON CONFLICT DO NOTHING;

INSERT INTO rbac_role_permissions (role_id, permission_id) VALUES
    ('role_editor', 'perm_dag_read'),
    ('role_editor', 'perm_dag_write'),
    ('role_editor', 'perm_dag_execute'),
    ('role_editor', 'perm_conn_read'),
    ('role_editor', 'perm_conn_write'),
    ('role_editor', 'perm_secrets_read')
ON CONFLICT DO NOTHING;

INSERT INTO rbac_role_permissions (role_id, permission_id) VALUES
    ('role_viewer', 'perm_dag_read'),
    ('role_viewer', 'perm_conn_read')
ON CONFLICT DO NOTHING;

INSERT INTO rbac_role_permissions (role_id, permission_id) VALUES
    ('role_ops', 'perm_dag_read'),
    ('role_ops', 'perm_dag_execute'),
    ('role_ops', 'perm_secrets_read'),
    ('role_ops', 'perm_secrets_write'),
    ('role_ops', 'perm_audit_read')
ON CONFLICT DO NOTHING;
