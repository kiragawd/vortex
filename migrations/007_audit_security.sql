-- ============================================================================
-- Migration 007: Audit & Secrets Security Hardening
-- Fixes: SEC-2, SEC-3, SEC-7, SEC-9
-- ============================================================================

-- ─── SEC-2: Secrets Audit Trail & Versioning ─────────────────────────────────
-- Add audit columns, version tracking, and soft-delete support to secrets table.

ALTER TABLE secrets ADD COLUMN IF NOT EXISTS created_by  TEXT;
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS updated_by  TEXT;
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS version     INTEGER NOT NULL DEFAULT 1;
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS deleted_at  TIMESTAMPTZ;
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW();

COMMENT ON COLUMN secrets.created_by IS 'Username who originally created this secret';
COMMENT ON COLUMN secrets.updated_by IS 'Username who last updated this secret';
COMMENT ON COLUMN secrets.version    IS 'Monotonically increasing version counter';
COMMENT ON COLUMN secrets.deleted_at IS 'Non-null indicates soft-deleted secret';

-- Index for filtering out soft-deleted secrets efficiently
CREATE INDEX IF NOT EXISTS idx_secrets_deleted_at ON secrets(deleted_at) WHERE deleted_at IS NULL;


-- ─── SEC-7: Audit Log Immutability ───────────────────────────────────────────
-- Prevent DELETE and TRUNCATE on audit_log to ensure immutability.
-- Using a PL/pgSQL trigger because RULES can be circumvented and REVOKE
-- depends on role setup; a trigger fires regardless of the caller's role.

CREATE OR REPLACE FUNCTION prevent_audit_log_delete()
RETURNS TRIGGER AS $$
BEGIN
    RAISE EXCEPTION 'DELETE on audit_log is prohibited — audit logs are immutable';
END;
$$ LANGUAGE plpgsql;

-- Drop and recreate to ensure idempotency
DROP TRIGGER IF EXISTS trg_audit_log_no_delete ON audit_log;
CREATE TRIGGER trg_audit_log_no_delete
    BEFORE DELETE ON audit_log
    FOR EACH ROW
    EXECUTE FUNCTION prevent_audit_log_delete();

-- Also prevent TRUNCATE
DROP TRIGGER IF EXISTS trg_audit_log_no_truncate ON audit_log;
CREATE TRIGGER trg_audit_log_no_truncate
    BEFORE TRUNCATE ON audit_log
    EXECUTE FUNCTION prevent_audit_log_delete();

COMMENT ON TABLE audit_log IS 'Immutable audit log — DELETE and TRUNCATE are blocked by trigger';


-- ─── SEC-9: IP Allowlist Change Tracking ─────────────────────────────────────
-- Add change-tracking columns to ip_allowlist for accountability.

ALTER TABLE ip_allowlist ADD COLUMN IF NOT EXISTS changed_by TEXT;
ALTER TABLE ip_allowlist ADD COLUMN IF NOT EXISTS changed_at TIMESTAMPTZ DEFAULT NOW();

COMMENT ON COLUMN ip_allowlist.changed_by IS 'Username who last toggled or modified this entry';
COMMENT ON COLUMN ip_allowlist.changed_at IS 'Timestamp of last modification';
