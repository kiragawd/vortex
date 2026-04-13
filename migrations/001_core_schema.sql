-- ============================================================================
-- Ryuo Core Schema (Consolidated Migration #1)
-- Covers: Core tables, teams, leader election, timestamp fixes, and indexes.
-- ============================================================================

-- ─── Core Tables ─────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS dags (
    id                TEXT        PRIMARY KEY,
    created_at        TIMESTAMPTZ NOT NULL,
    schedule_interval TEXT,
    last_run          TIMESTAMPTZ,
    is_paused         BOOLEAN     NOT NULL DEFAULT FALSE,
    timezone          TEXT        NOT NULL DEFAULT 'UTC',
    max_active_runs   INTEGER     NOT NULL DEFAULT 1,
    catchup           BOOLEAN     NOT NULL DEFAULT FALSE,
    next_run          TIMESTAMPTZ,
    is_dynamic        BOOLEAN     DEFAULT FALSE,
    team_id           TEXT
);

CREATE TABLE IF NOT EXISTS teams (
    id                    TEXT PRIMARY KEY,
    name                  TEXT NOT NULL,
    description           TEXT,
    max_concurrent_tasks  INTEGER DEFAULT 100,
    max_dags              INTEGER DEFAULT 50
);

-- Add FK for dags.team_id after teams is created
DO $$ BEGIN
    ALTER TABLE dags ADD CONSTRAINT fk_dags_team FOREIGN KEY (team_id) REFERENCES teams(id);
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS tasks (
    id               TEXT    NOT NULL,
    dag_id           TEXT    NOT NULL REFERENCES dags(id),
    name             TEXT    NOT NULL,
    command          TEXT    NOT NULL,
    task_type        TEXT    NOT NULL DEFAULT 'bash',
    config           TEXT    NOT NULL DEFAULT '{}',
    max_retries      INTEGER NOT NULL DEFAULT 0,
    retry_delay_secs INTEGER NOT NULL DEFAULT 30,
    pool             TEXT    NOT NULL DEFAULT 'default',
    task_group       TEXT,
    execution_timeout INTEGER,
    PRIMARY KEY (id, dag_id)
);

CREATE TABLE IF NOT EXISTS task_instances (
    id             TEXT        PRIMARY KEY,
    dag_id         TEXT        NOT NULL REFERENCES dags(id),
    task_id        TEXT        NOT NULL,
    state          TEXT        NOT NULL,
    execution_date TIMESTAMPTZ NOT NULL,
    start_time     TIMESTAMPTZ,
    end_time       TIMESTAMPTZ,
    try_number     INTEGER     NOT NULL DEFAULT 1,
    worker_id      TEXT,
    stdout         TEXT,
    stderr         TEXT,
    duration_ms    BIGINT,
    retry_count    INTEGER     NOT NULL DEFAULT 0,
    run_id         TEXT
);

CREATE INDEX IF NOT EXISTS idx_task_instances_dag_id         ON task_instances(dag_id);
CREATE INDEX IF NOT EXISTS idx_task_instances_run_id          ON task_instances(run_id);
CREATE INDEX IF NOT EXISTS idx_task_instances_state           ON task_instances(state);
CREATE INDEX IF NOT EXISTS idx_task_instances_execution_date  ON task_instances(execution_date);

CREATE TABLE IF NOT EXISTS dag_runs (
    id             TEXT        PRIMARY KEY,
    dag_id         TEXT        NOT NULL REFERENCES dags(id),
    state          TEXT        NOT NULL,
    execution_date TIMESTAMPTZ NOT NULL,
    start_time     TIMESTAMPTZ,
    end_time       TIMESTAMPTZ,
    triggered_by   TEXT        NOT NULL DEFAULT 'scheduler',
    sla_missed     BOOLEAN     DEFAULT FALSE,
    sla_seconds    INTEGER
);

CREATE TABLE IF NOT EXISTS users (
    username      TEXT PRIMARY KEY,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL,
    api_key       TEXT UNIQUE,
    team_id       TEXT REFERENCES teams(id),
    auth_provider TEXT DEFAULT 'local',
    external_id   TEXT,
    email         TEXT,
    display_name  TEXT,
    last_login    TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS secrets (
    key        TEXT        PRIMARY KEY,
    value      TEXT        NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS workers (
    id             TEXT        PRIMARY KEY,
    hostname       TEXT        NOT NULL,
    capacity       INTEGER     NOT NULL,
    active_tasks   INTEGER     NOT NULL DEFAULT 0,
    last_heartbeat TIMESTAMPTZ NOT NULL,
    state          TEXT        NOT NULL,
    labels         TEXT
);

CREATE TABLE IF NOT EXISTS dag_versions (
    id         TEXT         PRIMARY KEY,
    dag_id     TEXT         NOT NULL,
    version    BIGINT       NOT NULL,
    file_path  TEXT         NOT NULL,
    created_at TIMESTAMPTZ  NOT NULL
);

CREATE TABLE IF NOT EXISTS task_xcom (
    id             TEXT        PRIMARY KEY,
    dag_id         TEXT        NOT NULL,
    task_id        TEXT        NOT NULL,
    run_id         TEXT        NOT NULL,
    key            TEXT        NOT NULL,
    value          TEXT        NOT NULL,
    timestamp      TIMESTAMPTZ NOT NULL,
    UNIQUE (dag_id, task_id, run_id, key)
);

CREATE TABLE IF NOT EXISTS pools (
    name        TEXT    PRIMARY KEY,
    slots       INTEGER NOT NULL DEFAULT 128,
    description TEXT    DEFAULT ''
);

CREATE TABLE IF NOT EXISTS pool_slots (
    id               TEXT        PRIMARY KEY,
    pool_name        TEXT        NOT NULL REFERENCES pools(name),
    task_instance_id TEXT        NOT NULL,
    acquired_at      TIMESTAMPTZ NOT NULL,
    UNIQUE (pool_name, task_instance_id)
);

CREATE TABLE IF NOT EXISTS dag_callbacks (
    dag_id     TEXT PRIMARY KEY REFERENCES dags(id),
    config     TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- ─── Leader Election ─────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS leader_election (
    lock_key   INTEGER PRIMARY KEY DEFAULT 1,
    node_id    TEXT        NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);

-- ─── Seed Data ───────────────────────────────────────────────────────────────

INSERT INTO pools (name, slots, description)
VALUES ('default', 128, 'Default pool')
ON CONFLICT (name) DO NOTHING;
