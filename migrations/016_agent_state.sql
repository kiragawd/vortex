-- T-025: Agent state store (persistent key-value across DAG runs)
CREATE TABLE IF NOT EXISTS agent_state (
    agent_id    TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    ttl_expires TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (agent_id, key)
);
CREATE INDEX IF NOT EXISTS idx_agent_state_expires ON agent_state(ttl_expires) WHERE ttl_expires IS NOT NULL;

-- T-026: Agent decision log
CREATE TABLE IF NOT EXISTS agent_logs (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT NOT NULL,
    message    TEXT NOT NULL,
    context    TEXT NOT NULL DEFAULT '{}',
    level      TEXT NOT NULL DEFAULT 'info',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_agent_logs_agent ON agent_logs(agent_id, created_at DESC);
