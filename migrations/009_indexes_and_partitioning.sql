-- ============================================================================
-- Migration 009: Indexes, Partitioning Notes & Schema Fixes
-- Fixes: DB-5, DB-8, DB-9, DB-10, DB-11, DB-12
--
-- All statements are idempotent — safe to re-run on any database state.
-- ============================================================================


-- ─── DB-5: Missing Indexes for Common Query Patterns ─────────────────────────

-- dags: filter active (unpaused) DAGs
CREATE INDEX IF NOT EXISTS idx_dags_is_paused
    ON dags(is_paused);

-- dags: list active DAGs scoped by team (multi-tenant queries)
CREATE INDEX IF NOT EXISTS idx_dags_team_paused
    ON dags(team_id, is_paused);

-- task_instances: get queued/running tasks for a specific DAG
CREATE INDEX IF NOT EXISTS idx_task_instances_dag_state
    ON task_instances(dag_id, state);

-- task_instances: find tasks assigned to a specific worker by state
CREATE INDEX IF NOT EXISTS idx_task_instances_worker_state
    ON task_instances(worker_id, state);

-- workers: find stale or unhealthy workers by state and heartbeat
CREATE INDEX IF NOT EXISTS idx_workers_state_heartbeat
    ON workers(state, last_heartbeat);

-- dag_versions: query versions created after a given date
CREATE INDEX IF NOT EXISTS idx_dag_versions_created_at
    ON dag_versions(created_at);

-- lineage_events: OpenLineage API queries by namespace + job + time
CREATE INDEX IF NOT EXISTS idx_lineage_job_event_time
    ON lineage_events(job_namespace, job_name, event_time);

-- audit_log: "what did user X do?" — actor filtered by time range
CREATE INDEX IF NOT EXISTS idx_audit_actor_timestamp
    ON audit_log(actor, timestamp);

-- audit_log: "history of resource X" — resource_id filtered by time range
CREATE INDEX IF NOT EXISTS idx_audit_resource_timestamp
    ON audit_log(resource_id, timestamp);

-- pool_slots: find and cleanup stale slot reservations
CREATE INDEX IF NOT EXISTS idx_pool_slots_acquired_at
    ON pool_slots(acquired_at);

-- datasets: time-based dataset queries (recently created/updated)
CREATE INDEX IF NOT EXISTS idx_datasets_created_updated
    ON datasets(created_at, updated_at);

-- dataset_triggers: look up DAGs triggered by a specific dataset + condition
CREATE INDEX IF NOT EXISTS idx_dataset_triggers_dataset_condition
    ON dataset_triggers(dataset_id, condition);

-- api_tokens: find unused or stale tokens for cleanup
CREATE INDEX IF NOT EXISTS idx_api_tokens_last_used
    ON api_tokens(last_used_at);


-- ─── DB-8: Audit Log Partitioning Guidance ───────────────────────────────────
-- The audit_log table grows unbounded and should be partitioned by time
-- (monthly) with automated retention. This CANNOT be done in a migration
-- because:
--   1. Converting an existing table to partitioned requires pg_partman or
--      manual ATTACH PARTITION, which is a DBA-managed operation.
--   2. pg_partman configuration depends on environment (retention days,
--      partition interval, premake count).
--
-- Recommended setup (to be executed by a DBA or infrastructure automation):
--
--   CREATE EXTENSION IF NOT EXISTS pg_partman;
--   SELECT partman.create_parent(
--       p_parent_table := 'public.audit_log',
--       p_control       := 'created_at',
--       p_type          := 'native',
--       p_interval      := 'monthly',
--       p_premake       := 3
--   );
--   UPDATE partman.part_config
--      SET retention = '12 months',
--          retention_keep_table = false
--    WHERE parent_table = 'public.audit_log';
--
-- Until partitioning is enabled, the retention_policies table (migration 002)
-- can be used to schedule batch deletes of old audit records.

COMMENT ON TABLE audit_log IS
    'Immutable audit log — DELETE/TRUNCATE blocked by trigger. '
    'Should be partitioned by created_at (monthly) via pg_partman. '
    'See migration 009 comments for recommended setup.';


-- ─── DB-9: Audit Log — Duplicate Timestamp Fields ────────────────────────────
-- Both `timestamp` and `created_at` columns exist with DEFAULT NOW().
-- Keep `created_at` as the canonical column; sync `timestamp` via trigger
-- so existing queries continue to work, then deprecate `timestamp`.

COMMENT ON COLUMN audit_log.timestamp IS
    'DEPRECATED: Use created_at instead. '
    'This column is synced from created_at for backward compatibility.';

-- Trigger to keep `timestamp` in sync with `created_at` on INSERT/UPDATE.
-- This ensures any code reading `timestamp` sees the correct value while
-- callers migrate to `created_at`.
CREATE OR REPLACE FUNCTION sync_audit_log_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.timestamp := NEW.created_at;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_audit_log_sync_timestamp ON audit_log;
CREATE TRIGGER trg_audit_log_sync_timestamp
    BEFORE INSERT OR UPDATE ON audit_log
    FOR EACH ROW
    EXECUTE FUNCTION sync_audit_log_timestamp();


-- ─── DB-10: rbac_role_permissions — Add Audit Fields ─────────────────────────
-- Track when permissions were granted, last updated, and by whom.

ALTER TABLE rbac_role_permissions
    ADD COLUMN IF NOT EXISTS created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW();

ALTER TABLE rbac_role_permissions
    ADD COLUMN IF NOT EXISTS updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW();

ALTER TABLE rbac_role_permissions
    ADD COLUMN IF NOT EXISTS granted_by  TEXT;

COMMENT ON COLUMN rbac_role_permissions.created_at IS 'When this permission was granted';
COMMENT ON COLUMN rbac_role_permissions.updated_at IS 'Last modification timestamp';
COMMENT ON COLUMN rbac_role_permissions.granted_by IS 'Username who granted this permission';

-- Auto-update updated_at on modification
CREATE OR REPLACE FUNCTION update_rbac_role_permissions_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at := NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_rbac_role_permissions_updated ON rbac_role_permissions;
CREATE TRIGGER trg_rbac_role_permissions_updated
    BEFORE UPDATE ON rbac_role_permissions
    FOR EACH ROW
    EXECUTE FUNCTION update_rbac_role_permissions_timestamp();


-- ─── DB-11: pool_slots — Add released_at Timestamp ───────────────────────────
-- Track when slots were released so we can calculate hold duration and
-- identify slots that were never properly released.

ALTER TABLE pool_slots
    ADD COLUMN IF NOT EXISTS released_at TIMESTAMPTZ;

COMMENT ON COLUMN pool_slots.released_at IS
    'When the slot was released. NULL means slot is still held. '
    'Hold duration = released_at - acquired_at.';

-- Index for finding unreleased (active) slots efficiently
CREATE INDEX IF NOT EXISTS idx_pool_slots_unreleased
    ON pool_slots(pool_name) WHERE released_at IS NULL;


-- ─── DB-12: incident_configs — JSONB Schema Validation ───────────────────────
-- Validate that provider-specific config contains required fields:
--   - pagerduty: must have "api_key"
--   - webhook:   must have "url"
--   - opsgenie:  must have "api_key"
--   - datadog:   must have "api_key"

CREATE OR REPLACE FUNCTION validate_incident_config()
RETURNS TRIGGER AS $$
BEGIN
    CASE NEW.provider
        WHEN 'pagerduty' THEN
            IF NOT (NEW.config ? 'api_key') THEN
                RAISE EXCEPTION 'PagerDuty config must contain "api_key"';
            END IF;
        WHEN 'webhook' THEN
            IF NOT (NEW.config ? 'url') THEN
                RAISE EXCEPTION 'Webhook config must contain "url"';
            END IF;
        WHEN 'opsgenie' THEN
            IF NOT (NEW.config ? 'api_key') THEN
                RAISE EXCEPTION 'OpsGenie config must contain "api_key"';
            END IF;
        WHEN 'datadog' THEN
            IF NOT (NEW.config ? 'api_key') THEN
                RAISE EXCEPTION 'Datadog config must contain "api_key"';
            END IF;
        ELSE
            -- Unknown provider; allow but log concern via NOTICE
            RAISE NOTICE 'Unknown incident provider "%", skipping config validation', NEW.provider;
    END CASE;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_validate_incident_config ON incident_configs;
CREATE TRIGGER trg_validate_incident_config
    BEFORE INSERT OR UPDATE ON incident_configs
    FOR EACH ROW
    EXECUTE FUNCTION validate_incident_config();

COMMENT ON TABLE incident_configs IS
    'Incident routing configs per team. JSONB config is validated by trigger: '
    'pagerduty/opsgenie/datadog require "api_key", webhook requires "url".';
