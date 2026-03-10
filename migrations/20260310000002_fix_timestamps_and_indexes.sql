-- Migration: Fix TEXT timestamp columns → TIMESTAMPTZ
-- Bugs 28, 29, 30: three columns were stored as TEXT, losing timezone info
-- and preventing efficient time-based queries/sorting.
--
-- Bug 28: dag_versions.created_at  TEXT → TIMESTAMPTZ
-- Bug 29: task_xcom.timestamp      TEXT → TIMESTAMPTZ
-- Bug 30: pool_slots.acquired_at   TEXT → TIMESTAMPTZ

-- Bug 28
ALTER TABLE dag_versions
    ALTER COLUMN created_at TYPE TIMESTAMPTZ
    USING created_at::TIMESTAMPTZ;

-- Bug 29
ALTER TABLE task_xcom
    ALTER COLUMN timestamp TYPE TIMESTAMPTZ
    USING timestamp::TIMESTAMPTZ;

-- Bug 30
ALTER TABLE pool_slots
    ALTER COLUMN acquired_at TYPE TIMESTAMPTZ
    USING acquired_at::TIMESTAMPTZ;

-- Bug 33: add missing indexes so task_instance queries by dag_id, run_id,
-- and state don't perform full-table scans on large deployments.
CREATE INDEX IF NOT EXISTS idx_task_instances_dag_id ON task_instances(dag_id);
CREATE INDEX IF NOT EXISTS idx_task_instances_run_id ON task_instances(run_id);
CREATE INDEX IF NOT EXISTS idx_task_instances_state  ON task_instances(state);
