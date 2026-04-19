-- Migration 019: Schema hardening — missing constraints, indexes, and FKs
-- Fixes: BUG-MIG01 through BUG-MIG06

-- BUG-MIG01: dag_versions missing UNIQUE on (dag_id, version)
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'uq_dag_versions_dag_version') THEN
        ALTER TABLE dag_versions ADD CONSTRAINT uq_dag_versions_dag_version UNIQUE (dag_id, version);
    END IF;
END $$;

-- BUG-MIG02: dag_versions.dag_id missing FK to dags(id)
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_dag_versions_dag_id') THEN
        ALTER TABLE dag_versions ADD CONSTRAINT fk_dag_versions_dag_id FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE;
    END IF;
END $$;

-- BUG-MIG03: CHECK constraints on state/enum columns
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'chk_task_instances_state') THEN
        ALTER TABLE task_instances ADD CONSTRAINT chk_task_instances_state 
        CHECK (state IN ('Queued', 'Running', 'Success', 'Failed', 'Upstream_Failed', 'Skipped', 'Removed'));
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'chk_dag_runs_state') THEN
        ALTER TABLE dag_runs ADD CONSTRAINT chk_dag_runs_state 
        CHECK (state IN ('Queued', 'Pending', 'Running', 'Success', 'Failed'));
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'chk_workers_state') THEN
        ALTER TABLE workers ADD CONSTRAINT chk_workers_state 
        CHECK (state IN ('Active', 'Online', 'Offline', 'Draining'));
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'chk_users_role') THEN
        ALTER TABLE users ADD CONSTRAINT chk_users_role 
        CHECK (role IN ('Admin', 'Operator', 'Viewer'));
    END IF;
END $$;

-- BUG-MIG04: Missing FKs on reference columns (only add if referenced tables exist)
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'event_triggers')
       AND NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_event_triggers_dag_id') THEN
        ALTER TABLE event_triggers ADD CONSTRAINT fk_event_triggers_dag_id FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE;
    END IF;
END $$;

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'dataset_schemas')
       AND EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'datasets')
       AND NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_dataset_schemas_dataset_id') THEN
        ALTER TABLE dataset_schemas ADD CONSTRAINT fk_dataset_schemas_dataset_id FOREIGN KEY (dataset_id) REFERENCES datasets(id) ON DELETE CASCADE;
    END IF;
END $$;

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'dataset_triggers')
       AND NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'fk_dataset_triggers_dag_id') THEN
        ALTER TABLE dataset_triggers ADD CONSTRAINT fk_dataset_triggers_dag_id FOREIGN KEY (dag_id) REFERENCES dags(id) ON DELETE CASCADE;
    END IF;
END $$;

-- BUG-MIG05: dag_runs missing index on (dag_id, state)
CREATE INDEX IF NOT EXISTS idx_dag_runs_dag_state ON dag_runs(dag_id, state);

-- BUG-MIG06: dag_callbacks.updated_at is TEXT, should be TIMESTAMPTZ
-- Only alter if the column is currently TEXT type
DO $$ BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'dag_callbacks' AND column_name = 'updated_at' AND data_type = 'text'
    ) THEN
        ALTER TABLE dag_callbacks ALTER COLUMN updated_at TYPE TIMESTAMPTZ USING updated_at::timestamptz;
    END IF;
END $$;
