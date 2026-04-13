-- ============================================================================
-- Ryuo Migration #4: XCom Uniqueness Constraint (BUG-H6)
--
-- Ensures the UNIQUE constraint on (dag_id, task_id, run_id, key) exists on
-- the task_xcom table. This prevents duplicate XCom entries for the same
-- logical key and guarantees xcom_pull() returns a deterministic result.
--
-- Idempotent: safe to run on databases where migration 001 already created
-- the constraint.
-- ============================================================================

-- Add UNIQUE constraint if it does not already exist.
-- PostgreSQL does not support IF NOT EXISTS for constraints directly, so we
-- use a DO block to check the system catalog first.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'xcom_unique_key'
          AND conrelid = 'task_xcom'::regclass
    ) THEN
        ALTER TABLE task_xcom
            ADD CONSTRAINT xcom_unique_key
            UNIQUE (dag_id, task_id, run_id, key);
    END IF;
END $$;
