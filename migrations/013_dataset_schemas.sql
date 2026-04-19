-- T-016: Dataset schema tracking
CREATE TABLE IF NOT EXISTS dataset_schemas (
    id         TEXT PRIMARY KEY,
    dataset_id TEXT NOT NULL,
    schema_json TEXT NOT NULL DEFAULT '[]',
    version    INTEGER NOT NULL DEFAULT 1,
    captured_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    source_run_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_dataset_schemas_dataset ON dataset_schemas(dataset_id, version DESC);
