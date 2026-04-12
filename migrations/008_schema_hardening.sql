-- ============================================================================
-- Migration 008: Schema Hardening
-- Fixes: DB-1, DB-2, DB-3, DB-4, DB-6, DB-7
--
-- All statements are idempotent — safe to re-run on any database state.
-- ============================================================================


-- ─── DB-1: FK task_instances(task_id, dag_id) → tasks(id, dag_id) ────────────
-- task_instances references a task by (task_id, dag_id) but had no FK enforcing
-- referential integrity against the tasks table (which has PK (id, dag_id)).
-- Using CASCADE so removing a task definition also removes its instance records.

DO $$ BEGIN
    ALTER TABLE task_instances
        ADD CONSTRAINT fk_task_instances_task
        FOREIGN KEY (task_id, dag_id) REFERENCES tasks(id, dag_id)
        ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;


-- ─── DB-2: FK dataset_events.source_dag_id → dags.id ────────────────────────
-- dataset_events tracks which DAG produced a dataset event, but the
-- source_dag_id column had no FK constraint ensuring the DAG exists.
-- Using CASCADE so that deleting a DAG also removes its dataset events.

DO $$ BEGIN
    ALTER TABLE dataset_events
        ADD CONSTRAINT fk_dataset_events_source_dag
        FOREIGN KEY (source_dag_id) REFERENCES dags(id)
        ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;


-- ─── DB-3: FKs on cross_dag_dependencies → dags.id ──────────────────────────
-- Both upstream_dag_id and downstream_dag_id should reference valid DAGs.
-- Using CASCADE so removing a DAG cleans up its dependency edges.

DO $$ BEGIN
    ALTER TABLE cross_dag_dependencies
        ADD CONSTRAINT fk_cross_dag_upstream
        FOREIGN KEY (upstream_dag_id) REFERENCES dags(id)
        ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

DO $$ BEGIN
    ALTER TABLE cross_dag_dependencies
        ADD CONSTRAINT fk_cross_dag_downstream
        FOREIGN KEY (downstream_dag_id) REFERENCES dags(id)
        ON DELETE CASCADE;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;


-- ─── DB-4: UNIQUE on task_instances(dag_id, task_id, run_id, try_number) ─────
-- Prevents duplicate task instance records for the same execution attempt.
-- try_number distinguishes retries of the same (dag_id, task_id, run_id).

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'uq_task_instance_identity'
          AND conrelid = 'task_instances'::regclass
    ) THEN
        ALTER TABLE task_instances
            ADD CONSTRAINT uq_task_instance_identity
            UNIQUE (dag_id, task_id, run_id, try_number);
    END IF;
END $$;


-- ─── DB-6: NOT NULL with defaults on users.auth_provider ─────────────────────
-- auth_provider was nullable (DEFAULT 'local' but without NOT NULL).
-- Backfill any existing NULLs, then enforce NOT NULL going forward.

UPDATE users SET auth_provider = 'local' WHERE auth_provider IS NULL;

DO $$
BEGIN
    -- Check if the column is already NOT NULL to make this idempotent
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_name = 'users'
          AND column_name = 'auth_provider'
          AND is_nullable = 'YES'
    ) THEN
        ALTER TABLE users ALTER COLUMN auth_provider SET NOT NULL;
        ALTER TABLE users ALTER COLUMN auth_provider SET DEFAULT 'local';
    END IF;
END $$;


-- ─── DB-7a: CHECK on tasks.task_type ─────────────────────────────────────────
-- Restrict task_type to the known operator set. New operator types require a
-- migration to extend this constraint.

DO $$ BEGIN
    ALTER TABLE tasks
        ADD CONSTRAINT chk_tasks_task_type
        CHECK (task_type IN ('bash', 'python', 'sql', 'sensor', 'http', 'k8s'));
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;


-- ─── DB-7b: CHECK on ldap_group_mappings.role ────────────────────────────────
-- Restrict LDAP-mapped roles to the defined RBAC role set.

DO $$ BEGIN
    ALTER TABLE ldap_group_mappings
        ADD CONSTRAINT chk_ldap_role
        CHECK (role IN ('Admin', 'Viewer', 'Editor', 'Ops'));
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;
