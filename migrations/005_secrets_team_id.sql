-- Migration 004: Add team_id column to secrets table for multi-tenant isolation (BUG-C5)
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS team_id TEXT;
CREATE INDEX IF NOT EXISTS idx_secrets_team_id ON secrets(team_id);
