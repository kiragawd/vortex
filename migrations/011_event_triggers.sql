-- ============================================================================
-- Event Trigger Definitions for CLI CRUD (T-011)
-- ============================================================================

CREATE TABLE IF NOT EXISTS event_triggers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    event_type  TEXT NOT NULL,
    filter_json TEXT NOT NULL DEFAULT '{}',
    dag_id      TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    team_id     TEXT REFERENCES teams(id)
);

CREATE INDEX IF NOT EXISTS idx_event_triggers_event_type ON event_triggers(event_type);
CREATE INDEX IF NOT EXISTS idx_event_triggers_team ON event_triggers(team_id);
