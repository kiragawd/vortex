-- Migration: Leader Election Table
-- Replaces session-scoped pg_try_advisory_lock with a heartbeat-based row lock.
-- Bug 15 fix: pg_try_advisory_lock is session-level; with connection pooling,
-- the session (and thus the lock) can be silently recycled, causing split-brain.
-- This table stores a single row (lock_key = 1) that acts as the leader lease.
-- The holder must renew it every ~10s; an expired lease can be stolen.

CREATE TABLE IF NOT EXISTS leader_election (
    lock_key   INTEGER PRIMARY KEY DEFAULT 1,
    node_id    TEXT        NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
