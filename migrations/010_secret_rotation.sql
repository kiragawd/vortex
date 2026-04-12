-- 010: Secret rotation tracking (ENT-12)
-- Track vault key version and rotation history for compliance and audit.

-- Add key versioning columns to the secrets table so that each row records
-- which vault key generation encrypted it, and when it was last rotated.
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS key_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS rotated_at TIMESTAMPTZ;

-- Record every bulk rotation event for compliance audit trails.
CREATE TABLE IF NOT EXISTS vault_key_rotations (
    id           BIGSERIAL    PRIMARY KEY,
    rotated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    rotated_by   TEXT         NOT NULL,
    key_version  INTEGER      NOT NULL,
    secret_count INTEGER      NOT NULL DEFAULT 0,
    notes        TEXT,
    CONSTRAINT vault_key_rotations_version_check CHECK (key_version > 0)
);

-- Index to support queries like "show all rotations after date X".
CREATE INDEX IF NOT EXISTS idx_vault_key_rotations_rotated_at
    ON vault_key_rotations (rotated_at DESC);

-- Index to support filtering secrets that are still on an old key version.
CREATE INDEX IF NOT EXISTS idx_secrets_key_version
    ON secrets (key_version);
