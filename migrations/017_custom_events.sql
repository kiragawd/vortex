-- T-027/T-028: Custom events for inter-agent communication and event watching
CREATE TABLE IF NOT EXISTS custom_events (
    id          TEXT PRIMARY KEY,
    event_type  TEXT NOT NULL,
    source      TEXT NOT NULL,
    payload     TEXT NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_custom_events_type   ON custom_events(event_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_custom_events_source ON custom_events(source, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_custom_events_time   ON custom_events(created_at DESC);
