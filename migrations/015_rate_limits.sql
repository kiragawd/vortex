-- T-021: Agent action rate limiting
CREATE TABLE IF NOT EXISTS rate_limit_counters (
    actor    TEXT NOT NULL,
    action   TEXT NOT NULL,
    "window" TIMESTAMPTZ NOT NULL,
    count    INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (actor, action, "window")
);
CREATE INDEX IF NOT EXISTS idx_rate_limit_actor ON rate_limit_counters(actor, "window");
