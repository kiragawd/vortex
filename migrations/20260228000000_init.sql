-- 20260228000000_init.sql
-- Unified Schema for PostgreSQL and SQLite

CREATE TABLE IF NOT EXISTS dags (
    id                TEXT        PRIMARY KEY,
    created_at        TIMESTAMP   NOT NULL,
    schedule_interval TEXT,
    last_run          TIMESTAMP,
    is_paused         BOOLEAN     NOT NULL DEFAULT FALSE,
    timezone          TEXT        NOT NULL DEFAULT 'UTC',
    max_active_runs   INTEGER     NOT NULL DEFAULT 1,
    catchup           BOOLEAN     NOT NULL Default FALSE,
    next_run          TIMESTAMP
);

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
    execution_date TIMESTAMP   NOT NULL,
    start_time     TIMESTAMP,
    end_time       TIMESTAMP,
    try_number     INTEGER     NOT NULL DEFAULT 1,
    worker_id      TEXT,
    stdout         TEXT,
    stderr         TEXT,
    duration_ms    BIGINT,
    retry_count    INTEGER     NOT NULL DEFAULT 0,
    run_id         TEXT
);

CREATE TABLE IF NOT EXISTS dag_runs (
    id             TEXT        PRIMARY KEY,
    dag_id         TEXT        NOT NULL REFERENCES dags(id),
    state          TEXT        NOT NULL,
    execution_date TIMESTAMP   NOT NULL,
    start_time     TIMESTAMP,
    end_time       TIMESTAMP,
    triggered_by   TEXT        NOT NULL DEFAULT 'scheduler'
);

CREATE TABLE IF NOT EXISTS users (
    username      TEXT PRIMARY KEY,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL,
    api_key       TEXT UNIQUE
);

CREATE TABLE IF NOT EXISTS secrets (
    key        TEXT        PRIMARY KEY,
    value      TEXT        NOT NULL,
    updated_at TIMESTAMP   NOT NULL
);

CREATE TABLE IF NOT EXISTS workers (
    id             TEXT        PRIMARY KEY,
    hostname       TEXT        NOT NULL,
    capacity       INTEGER     NOT NULL,
    active_tasks   INTEGER     NOT NULL DEFAULT 0,
    last_heartbeat TIMESTAMP   NOT NULL,
    state          TEXT        NOT NULL,
    labels         TEXT
);

CREATE TABLE IF NOT EXISTS dag_versions (
    id         TEXT    PRIMARY KEY,
    dag_id     TEXT    NOT NULL,
    version    BIGINT  NOT NULL,
    file_path  TEXT    NOT NULL,
    created_at TEXT    NOT NULL
);

-- Note: BIGINT primary key or TEXT for compatibility on SQLite and PG.
CREATE TABLE IF NOT EXISTS task_xcom (
    id             TEXT        PRIMARY KEY,
    dag_id         TEXT        NOT NULL,
    task_id        TEXT        NOT NULL,
    run_id         TEXT        NOT NULL,
    key            TEXT        NOT NULL,
    value          TEXT        NOT NULL,
    timestamp      TEXT        NOT NULL,
    UNIQUE (dag_id, task_id, run_id, key)
);

CREATE TABLE IF NOT EXISTS pools (
    name        TEXT    PRIMARY KEY,
    slots       INTEGER NOT NULL DEFAULT 128,
    description TEXT    DEFAULT ''
);

-- Note: ID as TEXT GUID or similar cross-compatible.
CREATE TABLE IF NOT EXISTS pool_slots (
    id               TEXT      PRIMARY KEY,
    pool_name        TEXT      NOT NULL REFERENCES pools(name),
    task_instance_id TEXT      NOT NULL,
    acquired_at      TEXT      NOT NULL,
    UNIQUE (pool_name, task_instance_id)
);

CREATE TABLE IF NOT EXISTS dag_callbacks (
    dag_id     TEXT PRIMARY KEY REFERENCES dags(id),
    config     TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_log (
    id          TEXT     PRIMARY KEY,
    timestamp   TIMESTAMP NOT NULL,
    actor       TEXT     NOT NULL,
    action      TEXT     NOT NULL,
    target_type TEXT     NOT NULL,
    target_id   TEXT     NOT NULL,
    metadata    TEXT     NOT NULL DEFAULT '{}',
    ip_address  TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_actor     ON audit_log(actor);
CREATE INDEX IF NOT EXISTS idx_audit_action    ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);

-- Seed default pool
INSERT INTO pools (name, slots, description)
VALUES ('default', 128, 'Default pool')
ON CONFLICT (name) DO NOTHING;
