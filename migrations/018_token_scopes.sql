-- T-036: Agent-scoped API tokens
-- Add scope metadata, description, and expiry tracking to api_tokens
ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS scope_rules TEXT NOT NULL DEFAULT '[]';
ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
