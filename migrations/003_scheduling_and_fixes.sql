-- ============================================================================
-- Vortex Advanced Scheduling & Audit Fixes (Consolidated Migration #3)
-- Covers: Dataset-aware scheduling, cross-DAG deps, dynamic mapping, audit indexes
-- ============================================================================

-- ─── Advanced Scheduling & Data-Aware Orchestration ───────────────

CREATE TABLE IF NOT EXISTS datasets (
    id         TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    uri        TEXT NOT NULL UNIQUE,
    extra      JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS dataset_events (
    id            TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    dataset_id    TEXT NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    source_dag_id TEXT NOT NULL,
    source_task_id TEXT,
    source_run_id TEXT NOT NULL,
    extra         JSONB DEFAULT '{}',
    timestamp     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_dataset_events_dataset   ON dataset_events(dataset_id);
CREATE INDEX IF NOT EXISTS idx_dataset_events_timestamp ON dataset_events(timestamp);

CREATE TABLE IF NOT EXISTS dataset_triggers (
    id         TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    dag_id     TEXT NOT NULL,
    dataset_id TEXT NOT NULL REFERENCES datasets(id) ON DELETE CASCADE,
    condition  TEXT NOT NULL DEFAULT 'any',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(dag_id, dataset_id)
);

CREATE INDEX IF NOT EXISTS idx_dataset_triggers_dag     ON dataset_triggers(dag_id);
CREATE INDEX IF NOT EXISTS idx_dataset_triggers_dataset ON dataset_triggers(dataset_id);

CREATE TABLE IF NOT EXISTS cross_dag_dependencies (
    id                  TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    downstream_dag_id   TEXT NOT NULL,
    downstream_task_id  TEXT,
    upstream_dag_id     TEXT NOT NULL,
    upstream_task_id    TEXT,
    condition           TEXT NOT NULL DEFAULT 'success',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(downstream_dag_id, downstream_task_id, upstream_dag_id, upstream_task_id)
);

CREATE INDEX IF NOT EXISTS idx_cross_dag_downstream ON cross_dag_dependencies(downstream_dag_id);
CREATE INDEX IF NOT EXISTS idx_cross_dag_upstream   ON cross_dag_dependencies(upstream_dag_id);

CREATE TABLE IF NOT EXISTS task_map_templates (
    id            TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    dag_id        TEXT NOT NULL,
    task_id       TEXT NOT NULL,
    map_type      TEXT NOT NULL DEFAULT 'static',
    map_values    JSONB NOT NULL DEFAULT '[]',
    runtime_query TEXT,
    concurrency   INT NOT NULL DEFAULT 16,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(dag_id, task_id)
);

-- ─── Additional Audit & Compliance Indexes ────────────────────────

-- Composite index for common audit queries by type + time
CREATE INDEX IF NOT EXISTS idx_audit_event_type_time ON audit_log(event_type, timestamp);

-- Index for audit queries filtered by team and action
CREATE INDEX IF NOT EXISTS idx_audit_team_action ON audit_log(team_id, action);

-- Index for compliance control lookups by framework and status
CREATE INDEX IF NOT EXISTS idx_compliance_framework_status ON compliance_controls(framework, status);

-- Index for approval request lookups by requester
CREATE INDEX IF NOT EXISTS idx_approval_requests_requester ON approval_requests(requester);
