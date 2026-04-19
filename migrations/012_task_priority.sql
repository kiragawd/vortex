-- T-014: Task queue priority and scheduler pause state
ALTER TABLE task_instances ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_task_instances_priority ON task_instances(priority DESC, execution_date ASC);

-- Scheduler state (pause/resume)
CREATE TABLE IF NOT EXISTS scheduler_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO scheduler_state (key, value) VALUES ('paused', 'false') ON CONFLICT (key) DO NOTHING;
