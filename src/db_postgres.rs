#![allow(dead_code)]
// db_postgres.rs — PostgreSQL backend for VORTEX
//
// Implements `DatabaseBackend` using `sqlx` with an async `PgPool`.
// Schema is created/migrated lazily in `PostgresDb::new()`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::db_trait::DatabaseBackend;

/// Validate password strength (SEC-12).
///
/// # Security
/// Enforces enterprise password policy: minimum 8 characters with at least
/// one uppercase letter, one lowercase letter, one digit, and one special
/// character. Returns a descriptive error listing all unmet requirements.
pub fn validate_password_strength(password: &str) -> Result<()> {
    let mut errors: Vec<&str> = Vec::new();

    if password.len() < 8 {
        errors.push("at least 8 characters");
    }
    if !password.chars().any(|c| c.is_uppercase()) {
        errors.push("at least one uppercase letter");
    }
    if !password.chars().any(|c| c.is_lowercase()) {
        errors.push("at least one lowercase letter");
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        errors.push("at least one digit");
    }
    if !password.chars().any(|c| "!@#$%^&*()_+-=[]{}|;':\",./<>?`~".contains(c)) {
        errors.push("at least one special character (!@#$%^&*...)");
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Password does not meet strength requirements: must contain {}",
            errors.join(", ")
        ))
    }
}

/// SECURITY (BUG-H7): Escape SQL LIKE metacharacters (`%`, `_`, `\`) in user
/// input before using it as a LIKE operand.  The backslash is escaped first to
/// avoid double-escaping the replacement backslashes.
pub fn escape_like_pattern(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

// ─── Connection pool ─────────────────────────────────────────────────────────

pub struct PostgresDb {
    pool: PgPool,
    /// Unique identifier for this controller instance, used as the HA leader lock owner.
    /// Bug 15 fix: we need node_id to identify who holds the leader_election row.
    node_id: String,
}

impl PostgresDb {
    /// Improvement 42: expose the inner connection pool so callers (e.g. health
    /// check handler) can run lightweight queries without the full trait.
    pub fn pool(&self) -> &PgPool { &self.pool }

    /// Create a new `PostgresDb`, connect to Postgres and run schema migrations.
    pub async fn new(
        url: &str,
        max_connections: u32,
        min_connections: u32,
        idle_timeout: std::time::Duration,
    ) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(min_connections)
            .idle_timeout(idle_timeout)
            .connect(url)
            .await
            .context("Failed to connect to PostgreSQL")?;

        let node_id = std::env::var("VORTEX_NODE_ID")
            .unwrap_or_else(|_| format!("node-{}", &uuid::Uuid::new_v4().to_string()[..8]));

        let db = Self { pool, node_id };
        db.init().await?;
        Ok(db)
    }

    /// Create all tables via migrations.
    async fn init(&self) -> Result<()> {
        // Allow Docker / production deployments to skip automatic migrations
        // when a dedicated `migrate` init-container has already applied them.
        let skip_migrate = std::env::var("VORTEX_SKIP_AUTO_MIGRATE")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);

        if !skip_migrate {
            sqlx::migrate!("./migrations")
                .run(&self.pool)
                .await
                .context("Failed to run PostgreSQL migrations")?;
        }

        // ── Sprint 2: Create task_events table if no migration exists yet ───────
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS task_events (
                id          BIGSERIAL PRIMARY KEY,
                ti_id       TEXT NOT NULL,
                dag_id      TEXT NOT NULL,
                task_id     TEXT NOT NULL,
                run_id      TEXT NOT NULL,
                event       TEXT NOT NULL,
                message     TEXT,
                worker_id   TEXT,
                created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );"
        ).execute(&self.pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_task_events_run ON task_events(run_id);")
            .execute(&self.pool).await?;

        // ── Sprint 3: Add sla_missed to dag_runs ─────────────────────────────────
        sqlx::query("ALTER TABLE dag_runs ADD COLUMN IF NOT EXISTS sla_missed BOOLEAN NOT NULL DEFAULT FALSE;")
            .execute(&self.pool).await?;


        // ── Seed data ─────────────────────────────────────────────────────────

        // Seed default pool (idempotent)
        sqlx::query(
            "INSERT INTO pools (name, slots, description)
             VALUES ('default', 128, 'Default pool')
             ON CONFLICT (name) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .context("Failed to seed default pool")?;

        // Seed admin user (idempotent)
        // SEC-8: Default admin is created with password_change_required = true
        // so users cannot operate with default credentials.
        let admin_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE username = 'admin')",
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check admin existence")?;

        if !admin_exists {
            let hashed = hash("admin", DEFAULT_COST).context("bcrypt hash failed")?;
            sqlx::query(
                "INSERT INTO users (username, password_hash, role, api_key, password_change_required)
                 VALUES ('admin', $1, 'Admin', 'vortex_admin_key', TRUE)
                 ON CONFLICT (username) DO NOTHING",
            )
            .bind(&hashed)
            .execute(&self.pool)
            .await
            .context("Failed to seed admin user")?;
        }

        // Seed RBAC permissions (idempotent — safety net for existing DBs that
        // ran migration 002 before seed data was added to the file).
        sqlx::query(
            "INSERT INTO rbac_permissions (id, name, description, category) VALUES
                 ('perm_dag_read',     'dag.read',          'View DAGs and their runs',      'dag'),
                 ('perm_dag_write',    'dag.write',         'Create and modify DAGs',        'dag'),
                 ('perm_dag_execute',  'dag.execute',       'Trigger DAG runs',              'dag'),
                 ('perm_dag_delete',   'dag.delete',        'Delete DAGs',                   'dag'),
                 ('perm_admin_users',  'admin.users',       'Manage users and roles',        'admin'),
                 ('perm_admin_system', 'admin.system',      'System configuration',          'admin'),
                 ('perm_secrets_read', 'secrets.read',      'Read secrets (masked)',         'secrets'),
                 ('perm_secrets_write','secrets.write',     'Create and update secrets',     'secrets'),
                 ('perm_conn_read',    'connectors.read',   'View connectors',               'connectors'),
                 ('perm_conn_write',   'connectors.write',  'Manage connectors',             'connectors'),
                 ('perm_audit_read',   'audit.read',        'View audit logs',               'compliance'),
                 ('perm_compliance',   'compliance.manage', 'Manage compliance controls',    'compliance')
             ON CONFLICT (name) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .context("Failed to seed RBAC permissions")?;

        // Seed RBAC system roles (idempotent)
        sqlx::query(
            "INSERT INTO rbac_roles (id, name, description, is_system) VALUES
                 ('role_admin',  'Admin',  'Full system access',                           TRUE),
                 ('role_editor', 'Editor', 'Read/write DAGs and connectors',              TRUE),
                 ('role_viewer', 'Viewer', 'Read-only access to DAGs and runs',           TRUE),
                 ('role_ops',    'Ops',    'Operational access: execute, secrets, audit', TRUE)
             ON CONFLICT (name) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .context("Failed to seed RBAC roles")?;

        // Seed Admin role → all permissions (idempotent)
        sqlx::query(
            "INSERT INTO rbac_role_permissions (role_id, permission_id)
             SELECT 'role_admin', id FROM rbac_permissions
             ON CONFLICT DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .context("Failed to seed Admin role permissions")?;

        Ok(())
    }
}

// ─── Trait implementation ─────────────────────────────────────────────────────

#[async_trait]
impl DatabaseBackend for PostgresDb {
    // ── DAG operations ────────────────────────────────────────────────────────

    async fn save_dag(&self, dag_id: &str, schedule_interval: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO dags (id, created_at, schedule_interval)
             VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE
                SET schedule_interval = EXCLUDED.schedule_interval",
        )
        .bind(dag_id)
        .bind(Utc::now())
        .bind(schedule_interval)
        .execute(&self.pool)
        .await
        .context("save_dag")?;
        Ok(())
    }

    /// BUG-C3 fix: All DAG registration operations (upsert, stale task deletion,
    /// and task upserts) are wrapped in a single transaction. If any step fails,
    /// the entire operation rolls back, preventing corrupted state (e.g., a DAG
    /// left with zero tasks after a partial insertion failure).
    async fn register_dag(&self, dag: &crate::scheduler::Dag) -> Result<()> {
        let mut tx = self.pool.begin().await.context("register_dag: begin tx")?;

        sqlx::query(
            "INSERT INTO dags (id, created_at, schedule_interval, timezone, max_active_runs, catchup, is_dynamic, team_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE
                SET schedule_interval = EXCLUDED.schedule_interval,
                    timezone          = EXCLUDED.timezone,
                    max_active_runs   = EXCLUDED.max_active_runs,
                    catchup           = EXCLUDED.catchup,
                    is_dynamic        = EXCLUDED.is_dynamic,
                    team_id           = EXCLUDED.team_id",
        )
        .bind(&dag.id)
        .bind(Utc::now())
        .bind(dag.schedule_interval.as_deref())
        .bind(&dag.timezone)
        .bind(dag.max_active_runs)
        .bind(dag.catchup)
        .bind(dag.is_dynamic)
        .bind(None::<String>)  // Dag struct has no team_id field; NULL by default
        .execute(&mut *tx)
        .await
        .context("register_dag: upsert dag")?;

        // Remove tasks that are no longer in the DAG definition
        let task_ids: Vec<String> = dag.tasks.keys().cloned().collect();
        if task_ids.is_empty() {
            sqlx::query("DELETE FROM tasks WHERE dag_id = $1")
                .bind(&dag.id)
                .execute(&mut *tx)
                .await
                .context("register_dag: delete stale tasks")?;
        } else {
            // Build a NOT IN ($2, $3, ...) clause dynamically
            let placeholders: String = (0..task_ids.len())
                .map(|i| format!("${}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let query = format!(
                "DELETE FROM tasks WHERE dag_id = $1 AND id NOT IN ({})",
                placeholders
            );
            let mut q = sqlx::query(&query).bind(&dag.id);
            for tid in &task_ids {
                q = q.bind(tid);
            }
            q.execute(&mut *tx)
                .await
                .context("register_dag: delete stale tasks")?;
        }

        // Upsert each task within the same transaction (inlined from save_task
        // to use the transaction executor instead of the connection pool)
        for task in dag.tasks.values() {
            sqlx::query(
                "INSERT INTO tasks (id, dag_id, name, command, task_type, config,
                                    max_retries, retry_delay_secs, pool, task_group, execution_timeout)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT (id, dag_id) DO UPDATE
                    SET name             = EXCLUDED.name,
                        command          = EXCLUDED.command,
                        task_type        = EXCLUDED.task_type,
                        config           = EXCLUDED.config,
                        max_retries      = EXCLUDED.max_retries,
                        retry_delay_secs = EXCLUDED.retry_delay_secs,
                        pool             = EXCLUDED.pool,
                        task_group       = EXCLUDED.task_group,
                        execution_timeout= EXCLUDED.execution_timeout",
            )
            .bind(&task.id)
            .bind(&dag.id)
            .bind(&task.name)
            .bind(&task.command)
            .bind(&task.task_type)
            .bind(&task.config.to_string())
            .bind(task.max_retries)
            .bind(task.retry_delay_secs)
            .bind(&task.pool)
            .bind(task.task_group.as_deref())
            .bind(task.execution_timeout)
            .execute(&mut *tx)
            .await
            .context("register_dag: upsert task")?;
        }

        tx.commit().await.context("register_dag: commit")?;
        Ok(())
    }

    async fn get_all_dags(&self, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)> {
        let rows = sqlx::query(
            "SELECT id, created_at, schedule_interval, last_run, is_paused, timezone,
                    max_active_runs, catchup, next_run, is_dynamic, team_id,
                    COUNT(*) OVER() as total_count
             FROM dags
             ORDER BY created_at DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_all_dags")?;

        use sqlx::Row;
        let total = rows.first().map(|r| r.get::<i64, _>("total_count")).unwrap_or(0);
        
        let dags = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":                r.get::<String, _>("id"),
                    "created_at":        r.get::<DateTime<Utc>, _>("created_at"),
                    "schedule_interval": r.get::<Option<String>, _>("schedule_interval"),
                    "last_run":          r.get::<Option<DateTime<Utc>>, _>("last_run"),
                    "is_paused":         r.get::<bool, _>("is_paused"),
                    "timezone":          r.get::<Option<String>, _>("timezone").unwrap_or_else(|| "UTC".to_string()),
                    "max_active_runs":   r.get::<i32, _>("max_active_runs"),
                    "catchup":           r.get::<bool, _>("catchup"),
                    "is_dynamic":        r.get::<bool, _>("is_dynamic"),
                    "next_run":          r.get::<Option<DateTime<Utc>>, _>("next_run"),
                    "team_id":           r.get::<Option<String>, _>("team_id"),
                })
            })
            .collect();
        Ok((dags, total))
    }

    async fn get_dag_by_id(&self, dag_id: &str) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT id, created_at, schedule_interval, last_run, is_paused, timezone,
                    max_active_runs, catchup, next_run, is_dynamic, team_id
             FROM dags WHERE id = $1",
        )
        .bind(dag_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_dag_by_id")?;

        use sqlx::Row;
        Ok(row.map(|r| {
            serde_json::json!({
                "id":                r.get::<String, _>("id"),
                "created_at":        r.get::<DateTime<Utc>, _>("created_at"),
                "schedule_interval": r.get::<Option<String>, _>("schedule_interval"),
                "last_run":          r.get::<Option<DateTime<Utc>>, _>("last_run"),
                "is_paused":         r.get::<bool, _>("is_paused"),
                "timezone":          r.get::<Option<String>, _>("timezone").unwrap_or_else(|| "UTC".to_string()),
                "max_active_runs":   r.get::<i32, _>("max_active_runs"),
                "catchup":           r.get::<bool, _>("catchup"),
                "is_dynamic":        r.get::<bool, _>("is_dynamic"),
                "next_run":          r.get::<Option<DateTime<Utc>>, _>("next_run"),
                "team_id":           r.get::<Option<String>, _>("team_id"),
            })
        }))
    }

    async fn update_dag_config(
        &self,
        dag_id: &str,
        schedule_interval: Option<&str>,
        timezone: &str,
        max_active_runs: i32,
        catchup: bool,
        is_dynamic: bool,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE dags
             SET schedule_interval = $1,
                 timezone          = $2,
                 max_active_runs   = $3,
                 catchup           = $4,
                 is_dynamic        = $5
             WHERE id = $6",
        )
        .bind(schedule_interval)
        .bind(timezone)
        .bind(max_active_runs)
        .bind(catchup)
        .bind(is_dynamic)
        .bind(dag_id)
        .execute(&self.pool)
        .await
        .context("update_dag_config")?;
        Ok(())
    }

    async fn update_dag_last_run(&self, dag_id: &str, last_run: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE dags SET last_run = $1 WHERE id = $2")
            .bind(last_run)
            .bind(dag_id)
            .execute(&self.pool)
            .await
            .context("update_dag_last_run")?;
        Ok(())
    }

    async fn update_dag_next_run(
        &self,
        dag_id: &str,
        next_run: Option<DateTime<Utc>>,
    ) -> Result<()> {
        sqlx::query("UPDATE dags SET next_run = $1 WHERE id = $2")
            .bind(next_run)
            .bind(dag_id)
            .execute(&self.pool)
            .await
            .context("update_dag_next_run")?;
        Ok(())
    }

    async fn get_scheduled_dags(
        &self,
    ) -> Result<Vec<(String, String, Option<DateTime<Utc>>, bool, String, i32, bool, Option<String>)>> {
        let rows = sqlx::query(
            "SELECT id, schedule_interval, last_run, is_paused, timezone, max_active_runs, catchup, team_id
             FROM dags
             WHERE schedule_interval IS NOT NULL AND schedule_interval <> ''",
        )
        .fetch_all(&self.pool)
        .await
        .context("get_scheduled_dags")?;

        use sqlx::Row;
        let dags = rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("id"),
                    r.get::<Option<String>, _>("schedule_interval")
                        .unwrap_or_default(),
                    r.get::<Option<DateTime<Utc>>, _>("last_run"),
                    r.get::<bool, _>("is_paused"),
                    r.get::<Option<String>, _>("timezone")
                        .unwrap_or_else(|| "UTC".to_string()),
                    r.get::<i32, _>("max_active_runs"),
                    r.get::<bool, _>("catchup"),
                    r.get::<Option<String>, _>("team_id"),
                )
            })
            .collect();
        Ok(dags)
    }

    async fn pause_dag(&self, dag_id: &str) -> Result<()> {
        sqlx::query("UPDATE dags SET is_paused = TRUE WHERE id = $1")
            .bind(dag_id)
            .execute(&self.pool)
            .await
            .context("pause_dag")?;
        Ok(())
    }

    async fn unpause_dag(&self, dag_id: &str) -> Result<()> {
        sqlx::query("UPDATE dags SET is_paused = FALSE WHERE id = $1")
            .bind(dag_id)
            .execute(&self.pool)
            .await
            .context("unpause_dag")?;
        Ok(())
    }

    async fn get_active_dag_run_count(&self, dag_id: &str) -> Result<i32> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dag_runs WHERE dag_id = $1 AND state IN ('Queued', 'Running')",
        )
        .bind(dag_id)
        .fetch_one(&self.pool)
        .await
        .context("get_active_dag_run_count")?;
        Ok(count as i32)
    }

    async fn get_active_dag_runs_for_team(&self, team_id: &str) -> Result<i32> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(dr.id) 
             FROM dag_runs dr
             JOIN dags d ON dr.dag_id = d.id
             WHERE d.team_id = $1 AND dr.state IN ('Queued', 'Running')"
        )
        .bind(team_id)
        .fetch_one(&self.pool)
        .await
        .context("get_active_dag_runs_for_team")?;
        Ok(count as i32)
    }

    async fn get_active_tasks_for_team(&self, team_id: &str) -> Result<i32> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(ti.id) 
             FROM task_instances ti
             JOIN dags d ON ti.dag_id = d.id
             WHERE d.team_id = $1 AND ti.state IN ('Queued', 'Running')"
        )
        .bind(team_id)
        .fetch_one(&self.pool)
        .await
        .context("get_active_tasks_for_team")?;
        Ok(count as i32)
    }

    // ── Task operations ───────────────────────────────────────────────────────

    async fn save_task(
        &self,
        dag_id: &str,
        task_id: &str,
        name: &str,
        command: &str,
        task_type: &str,
        config: &str,
        max_retries: i32,
        retry_delay_secs: i32,
        pool: &str,
        task_group: Option<&str>,
        execution_timeout: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO tasks (id, dag_id, name, command, task_type, config,
                                max_retries, retry_delay_secs, pool, task_group, execution_timeout)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id, dag_id) DO UPDATE
                SET name             = EXCLUDED.name,
                    command          = EXCLUDED.command,
                    task_type        = EXCLUDED.task_type,
                    config           = EXCLUDED.config,
                    max_retries      = EXCLUDED.max_retries,
                    retry_delay_secs = EXCLUDED.retry_delay_secs,
                    pool             = EXCLUDED.pool,
                    task_group       = EXCLUDED.task_group,
                    execution_timeout= EXCLUDED.execution_timeout",
        )
        .bind(task_id)
        .bind(dag_id)
        .bind(name)
        .bind(command)
        .bind(task_type)
        .bind(config)
        .bind(max_retries)
        .bind(retry_delay_secs)
        .bind(pool)
        .bind(task_group)
        .bind(execution_timeout)
        .execute(&self.pool)
        .await
        .context("save_task")?;
        Ok(())
    }

    async fn get_dag_tasks(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        // PERF-2: fetch_all issues a single query and returns all rows in bulk — no N+1.
        let rows = sqlx::query(
            "SELECT id, name, command, task_type, config, max_retries, retry_delay_secs, pool, task_group, execution_timeout
             FROM tasks WHERE dag_id = $1",
        )
        .bind(dag_id)
        .fetch_all(&self.pool)
        .await
        .context("get_dag_tasks")?;

        use sqlx::Row;
        let tasks = rows
            .iter()
            .map(|r| {
                let config_raw: String = r.get("config");
                let config_val = serde_json::from_str::<serde_json::Value>(&config_raw)
                    .unwrap_or(serde_json::json!({}));
                serde_json::json!({
                    "id":              r.get::<String, _>("id"),
                    "name":            r.get::<String, _>("name"),
                    "command":         r.get::<String, _>("command"),
                    "task_type":       r.get::<String, _>("task_type"),
                    "config":          config_val,
                    "max_retries":     r.get::<i32, _>("max_retries"),
                    "retry_delay_secs":r.get::<i32, _>("retry_delay_secs"),
                    "pool":            r.get::<Option<String>, _>("pool")
                                        .unwrap_or_else(|| "default".to_string()),
                    "task_group":      r.get::<Option<String>, _>("task_group"),
                    "execution_timeout": r.get::<Option<i32>, _>("execution_timeout"),
                })
            })
            .collect();
        Ok(tasks)
    }

    // ── Task instance operations ──────────────────────────────────────────────

    async fn create_task_instance(
        &self,
        id: &str,
        dag_id: &str,
        task_id: &str,
        state: &str,
        execution_date: DateTime<Utc>,
        run_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO task_instances (id, dag_id, task_id, state, execution_date, run_id)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE
                SET state          = EXCLUDED.state,
                    execution_date = EXCLUDED.execution_date,
                    run_id         = EXCLUDED.run_id",
        )
        .bind(id)
        .bind(dag_id)
        .bind(task_id)
        .bind(state)
        .bind(execution_date)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .context("create_task_instance")?;
        Ok(())
    }

    async fn update_task_state(&self, id: &str, state: &str) -> Result<()> {
        let now = Utc::now();
        match state {
            "Running" => {
                sqlx::query(
                    "UPDATE task_instances SET state = $1, start_time = $2 WHERE id = $3",
                )
                .bind(state)
                .bind(now)
                .bind(id)
                .execute(&self.pool)
                .await
                .context("update_task_state(Running)")?;
            }
            "Success" | "Failed" => {
                sqlx::query(
                    "UPDATE task_instances SET state = $1, end_time = $2 WHERE id = $3",
                )
                .bind(state)
                .bind(now)
                .bind(id)
                .execute(&self.pool)
                .await
                .context("update_task_state(terminal)")?;
            }
            _ => {
                sqlx::query("UPDATE task_instances SET state = $1 WHERE id = $2")
                    .bind(state)
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .context("update_task_state")?;
            }
        }
        Ok(())
    }

    async fn get_task_instances(&self, dag_id: &str, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)> {
        let rows = sqlx::query(
            "SELECT id, task_id, state, execution_date, start_time, end_time,
                    stdout, stderr, duration_ms, retry_count, run_id,
                    COUNT(*) OVER() as total_count
             FROM task_instances WHERE dag_id = $1
             ORDER BY execution_date DESC, start_time DESC NULLS LAST
             LIMIT $2 OFFSET $3",
        )
        .bind(dag_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_task_instances")?;

        use sqlx::Row;
        let total = rows.first().map(|r| r.get::<i64, _>("total_count")).unwrap_or(0);
        
        let instances = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":             r.get::<String, _>("id"),
                    "task_id":        r.get::<String, _>("task_id"),
                    "state":          r.get::<String, _>("state"),
                    "execution_date": r.get::<DateTime<Utc>, _>("execution_date"),
                    "start_time":     r.get::<Option<DateTime<Utc>>, _>("start_time"),
                    "end_time":       r.get::<Option<DateTime<Utc>>, _>("end_time"),
                    "stdout":         r.get::<Option<String>, _>("stdout"),
                    "stderr":         r.get::<Option<String>, _>("stderr"),
                    "duration_ms":    r.get::<Option<i64>, _>("duration_ms"),
                    "retry_count":    r.get::<i32, _>("retry_count"),
                    "run_id":         r.get::<Option<String>, _>("run_id"),
                })
            })
            .collect();
        Ok((instances, total))
    }

    async fn get_task_instance(
        &self,
        ti_id: &str,
    ) -> Result<Option<(String, String, DateTime<Utc>)>> {
        let row = sqlx::query(
            "SELECT dag_id, task_id, execution_date FROM task_instances WHERE id = $1",
        )
        .bind(ti_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_task_instance")?;

        use sqlx::Row;
        Ok(row.map(|r| {
            (
                r.get::<String, _>("dag_id"),
                r.get::<String, _>("task_id"),
                r.get::<DateTime<Utc>, _>("execution_date"),
            )
        }))
    }


    // ── Task Events ──────────────────────────────────────────────────────────

    async fn log_task_event(
        &self,
        ti_id: &str,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        event: &str,
        message: Option<&str>,
        worker_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO task_events (ti_id, dag_id, task_id, run_id, event, message, worker_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        )
        .bind(ti_id)
        .bind(dag_id)
        .bind(task_id)
        .bind(run_id)
        .bind(event)
        .bind(message)
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .context("log_task_event")?;
        Ok(())
    }

    async fn get_task_events(&self, ti_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT id, event, message, worker_id, created_at
             FROM task_events WHERE ti_id = $1 ORDER BY created_at ASC"
        )
        .bind(ti_id)
        .fetch_all(&self.pool)
        .await
        .context("get_task_events")?;

        use sqlx::Row;
        Ok(rows.iter().map(|r| {
            serde_json::json!({
                "id": r.get::<i64, _>("id"),
                "event": r.get::<String, _>("event"),
                "message": r.get::<Option<String>, _>("message"),
                "worker_id": r.get::<Option<String>, _>("worker_id"),
                "created_at": r.get::<DateTime<Utc>, _>("created_at"),
            })
        }).collect())
    }

    async fn get_interrupted_tasks(&self) -> Result<Vec<(String, String, String)>> {
        let rows = sqlx::query(
            "SELECT id, dag_id, task_id FROM task_instances WHERE state = 'Running'",
        )
        .fetch_all(&self.pool)
        .await
        .context("get_interrupted_tasks")?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("id"),
                    r.get::<String, _>("dag_id"),
                    r.get::<String, _>("task_id"),
                )
            })
            .collect())
    }

    async fn update_task_logs(&self, ti_id: &str, stdout: &str, stderr: &str) -> Result<()> {
        sqlx::query(
            "UPDATE task_instances SET stdout = $1, stderr = $2 WHERE id = $3",
        )
        .bind(stdout)
        .bind(stderr)
        .bind(ti_id)
        .execute(&self.pool)
        .await
        .context("update_task_logs")?;
        Ok(())
    }

    async fn store_task_result(
        &self,
        task_instance_id: &str,
        result: &crate::executor::ExecutionResult,
    ) -> Result<()> {
        let state = if result.success { "Success" } else { "Failed" };
        let now = Utc::now();
        sqlx::query(
            "UPDATE task_instances
             SET state       = $1,
                 stdout      = $2,
                 stderr      = $3,
                 duration_ms = $4,
                 end_time    = $5
             WHERE id = $6",
        )
        .bind(state)
        .bind(&result.stdout)
        .bind(&result.stderr)
        .bind(result.duration_ms as i64)
        .bind(now)
        .bind(task_instance_id)
        .execute(&self.pool)
        .await
        .context("store_task_result")?;
        Ok(())
    }

    async fn get_task_instance_retry_info(&self, ti_id: &str) -> Result<(i32, String)> {
        let row = sqlx::query(
            "SELECT retry_count, state FROM task_instances WHERE id = $1",
        )
        .bind(ti_id)
        .fetch_one(&self.pool)
        .await
        .context("get_task_instance_retry_info")?;

        use sqlx::Row;
        Ok((
            row.get::<i32, _>("retry_count"),
            row.get::<String, _>("state"),
        ))
    }

    async fn increment_task_retry_count(&self, ti_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE task_instances SET retry_count = retry_count + 1 WHERE id = $1",
        )
        .bind(ti_id)
        .execute(&self.pool)
        .await
        .context("increment_task_retry_count")?;
        Ok(())
    }

    async fn get_task_instance_details(
        &self,
        ti_id: &str,
    ) -> Result<Option<(String, String, String, String, String, String, i32, i32, i32)>> {
        let row = sqlx::query(
            "SELECT ti.dag_id,
                    ti.task_id,
                    t.command,
                    dr.id        AS run_id,
                    t.task_type,
                    t.config,
                    t.max_retries,
                    t.retry_delay_secs,
                    COALESCE(t.execution_timeout, 0) AS execution_timeout_secs
             FROM task_instances ti
             JOIN tasks    t  ON ti.task_id = t.id AND ti.dag_id = t.dag_id
             -- Bug 34 fix: JOIN on run_id (unique per run), not (dag_id, execution_date).
             -- Re-triggered runs share dag_id+execution_date, causing duplicate rows.
             JOIN dag_runs dr ON ti.run_id = dr.id
             WHERE ti.id = $1",
        )
        .bind(ti_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_task_instance_details")?;

        use sqlx::Row;
        Ok(row.map(|r| {
            (
                r.get::<String, _>("dag_id"),
                r.get::<String, _>("task_id"),
                r.get::<String, _>("command"),
                r.get::<String, _>("run_id"),
                r.get::<String, _>("task_type"),
                r.get::<String, _>("config"),
                r.get::<i32, _>("max_retries"),
                r.get::<i32, _>("retry_delay_secs"),
                r.get::<i32, _>("execution_timeout_secs"),
            )
        }))
    }

    async fn assign_task_to_worker(&self, ti_id: &str, worker_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE task_instances
             SET state     = 'Running',
                 worker_id = $1,
                 start_time = $2
             WHERE id = $3",
        )
        .bind(worker_id)
        .bind(Utc::now())
        .bind(ti_id)
        .execute(&self.pool)
        .await
        .context("assign_task_to_worker")?;
        Ok(())
    }

    // ── DAG run operations ────────────────────────────────────────────────────

    async fn create_dag_run(
        &self,
        id: &str,
        dag_id: &str,
        execution_date: DateTime<Utc>,
        triggered_by: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO dag_runs (id, dag_id, state, execution_date, start_time, triggered_by)
             VALUES ($1, $2, 'Queued', $3, $4, $5)",
        )
        .bind(id)
        .bind(dag_id)
        .bind(execution_date)
        .bind(execution_date)
        .bind(triggered_by)
        .execute(&self.pool)
        .await
        .context("create_dag_run")?;
        Ok(())
    }

    async fn update_dag_run_state(&self, id: &str, state: &str) -> Result<()> {
        let now = Utc::now();
        match state {
            "Running" => {
                sqlx::query(
                    "UPDATE dag_runs SET state = $1, start_time = $2 WHERE id = $3",
                )
                .bind(state)
                .bind(now)
                .bind(id)
                .execute(&self.pool)
                .await
                .context("update_dag_run_state(Running)")?;
            }
            "Success" | "Failed" => {
                sqlx::query(
                    "UPDATE dag_runs SET state = $1, end_time = $2 WHERE id = $3",
                )
                .bind(state)
                .bind(now)
                .bind(id)
                .execute(&self.pool)
                .await
                .context("update_dag_run_state(terminal)")?;
            }
            _ => {
                sqlx::query("UPDATE dag_runs SET state = $1 WHERE id = $2")
                    .bind(state)
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .context("update_dag_run_state")?;
            }
        }
        Ok(())
    }

    async fn get_dag_runs(&self, dag_id: &str, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)> {
        let rows = sqlx::query(
            "SELECT id, dag_id, state, execution_date, start_time, end_time, triggered_by, sla_missed,
                    COUNT(*) OVER() as total_count
             FROM dag_runs
             WHERE dag_id = $1
             ORDER BY execution_date DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(dag_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_dag_runs")?;

        use sqlx::Row;
        let total = rows.first().map(|r| r.get::<i64, _>("total_count")).unwrap_or(0);
        
        let runs = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":             r.get::<String, _>("id"),
                    "dag_id":         r.get::<String, _>("dag_id"),
                    "state":          r.get::<String, _>("state"),
                    "execution_date": r.get::<DateTime<Utc>, _>("execution_date"),
                    "start_time":     r.get::<Option<DateTime<Utc>>, _>("start_time"),
                    "end_time":       r.get::<Option<DateTime<Utc>>, _>("end_time"),
                    "triggered_by":   r.get::<String, _>("triggered_by"),
                    "sla_missed":     r.try_get::<bool, _>("sla_missed").unwrap_or(false),
                })
            })
            .collect();
        Ok((runs, total))
    }

    async fn get_all_runs(&self, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)> {
        let rows = sqlx::query(
            "SELECT id, dag_id, state, execution_date, start_time, end_time, triggered_by, sla_missed,
                    COUNT(*) OVER() as total_count
             FROM dag_runs
             ORDER BY execution_date DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_all_runs")?;

        use sqlx::Row;
        let total = rows.first().map(|r| r.get::<i64, _>("total_count")).unwrap_or(0);
        let runs = rows.iter().map(|r| serde_json::json!({
            "id":             r.get::<String, _>("id"),
            "dag_id":         r.get::<String, _>("dag_id"),
            "state":          r.get::<String, _>("state"),
            "execution_date": r.get::<DateTime<Utc>, _>("execution_date"),
            "start_time":     r.get::<Option<DateTime<Utc>>, _>("start_time"),
            "end_time":       r.get::<Option<DateTime<Utc>>, _>("end_time"),
            "triggered_by":   r.get::<String, _>("triggered_by"),
            "sla_missed":     r.try_get::<bool, _>("sla_missed").unwrap_or(false),
        })).collect();
        Ok((runs, total))
    }

    async fn mark_sla_missed(&self, run_id: &str) -> Result<()> {
        sqlx::query("UPDATE dag_runs SET sla_missed = TRUE WHERE id = $1")
            .bind(run_id)
            .execute(&self.pool)
            .await
            .context("mark_sla_missed")?;
        Ok(())
    }

    async fn get_running_dag_runs(&self) -> Result<Vec<(String, String, DateTime<Utc>)>> {
        let rows = sqlx::query(
            "SELECT id, dag_id, start_time FROM dag_runs
             WHERE state = 'Running' AND sla_missed = FALSE AND start_time IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await
        .context("get_running_dag_runs")?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("id"),
                    r.get::<String, _>("dag_id"),
                    r.get::<DateTime<Utc>, _>("start_time"),
                )
            })
            .collect())
    }

    // ── User management ───────────────────────────────────────────────────────

    async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: &str,
        api_key: &str,
    ) -> Result<()> {
        validate_password_strength(password)?;
        let hashed = hash(password, DEFAULT_COST).context("bcrypt hash failed")?;
        sqlx::query(
            "INSERT INTO users (username, password_hash, role, api_key)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(username)
        .bind(&hashed)
        .bind(role)
        .bind(api_key)
        .execute(&self.pool)
        .await
        .context("create_user")?;
        Ok(())
    }

    async fn delete_user(&self, username: &str) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE username = $1")
            .bind(username)
            .execute(&self.pool)
            .await
            .context("delete_user")?;
        Ok(())
    }

    async fn get_all_users(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query("SELECT username, role, api_key FROM users")
            .fetch_all(&self.pool)
            .await
            .context("get_all_users")?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "username": r.get::<String, _>("username"),
                    "role":     r.get::<String, _>("role"),
                    "api_key":  r.get::<String, _>("api_key"),
                })
            })
            .collect())
    }

    async fn validate_user(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<(String, String, bool)>> {
        let row = sqlx::query(
            "SELECT password_hash, api_key, role, COALESCE(password_change_required, FALSE) AS password_change_required FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .context("validate_user")?;

        use sqlx::Row;
        if let Some(r) = row {
            let stored_hash: String = r.get("password_hash");
            let api_key: String = r.get("api_key");
            let role: String = r.get("role");
            let password_change_required: bool = r.get("password_change_required");
            if verify(password, &stored_hash).unwrap_or(false) {
                return Ok(Some((api_key, role, password_change_required)));
            }
        }
        Ok(None)
    }

    async fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<(String, String, Option<String>)>> {
        let row = sqlx::query(
            "SELECT username, role, team_id FROM users WHERE api_key = $1",
        )
        .bind(api_key)
        .fetch_optional(&self.pool)
        .await
        .context("get_user_by_api_key")?;

        use sqlx::Row;
        Ok(row.map(|r| {
            (
                r.get::<String, _>("username"),
                r.get::<String, _>("role"),
                r.get::<Option<String>, _>("team_id"),
            )
        }))
    }

    // ── Secret management ─────────────────────────────────────────────────────

    async fn store_secret(&self, key: &str, encrypted_value: &str, team_id: Option<&str>, actor: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO secrets (key, value, updated_at, team_id, created_by, updated_by, version, created_at)
             VALUES ($1, $2, $3, $4, $5, $5, 1, $3)
             ON CONFLICT (key) DO UPDATE
                SET value      = EXCLUDED.value,
                    updated_at = EXCLUDED.updated_at,
                    updated_by = $5,
                    version    = secrets.version + 1",
        )
        .bind(key)
        .bind(encrypted_value)
        .bind(Utc::now())
        .bind(team_id)
        .bind(actor)
        .execute(&self.pool)
        .await
        .context("store_secret")?;
        Ok(())
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let row =
            sqlx::query_scalar("SELECT value FROM secrets WHERE key = $1 AND deleted_at IS NULL")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .context("get_secret")?;
        Ok(row)
    }

    async fn get_secrets_batch(&self, keys: &[String]) -> Result<std::collections::HashMap<String, Option<String>>> {
        if keys.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        // PERF-9: Single query for all required secrets instead of N individual lookups.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, value FROM secrets WHERE key = ANY($1) AND deleted_at IS NULL"
        )
        .bind(keys)
        .fetch_all(&self.pool)
        .await
        .context("get_secrets_batch")?;

        let mut result: std::collections::HashMap<String, Option<String>> =
            keys.iter().map(|k| (k.clone(), None)).collect();
        for (name, value) in rows {
            result.insert(name, Some(value));
        }
        Ok(result)
    }

    async fn get_all_secrets(&self, team_id: Option<&str>) -> Result<Vec<String>> {
        let keys: Vec<String> = match team_id {
            Some(tid) => {
                sqlx::query_scalar("SELECT key FROM secrets WHERE team_id = $1 AND deleted_at IS NULL")
                    .bind(tid)
                    .fetch_all(&self.pool)
                    .await
                    .context("get_all_secrets")?
            }
            None => {
                sqlx::query_scalar("SELECT key FROM secrets WHERE deleted_at IS NULL")
                    .fetch_all(&self.pool)
                    .await
                    .context("get_all_secrets")?
            }
        };
        Ok(keys)
    }

    async fn delete_secret(&self, key: &str, actor: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE secrets SET deleted_at = NOW(), updated_by = $2, version = version + 1
             WHERE key = $1 AND deleted_at IS NULL"
        )
            .bind(key)
            .bind(actor)
            .execute(&self.pool)
            .await
            .context("delete_secret")?;
        Ok(())
    }

    // ── Worker management ─────────────────────────────────────────────────────

    async fn upsert_worker(
        &self,
        id: &str,
        hostname: &str,
        capacity: i32,
        labels: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO workers (id, hostname, capacity, last_heartbeat, state, labels)
             VALUES ($1, $2, $3, $4, 'Active', $5)
             ON CONFLICT (id) DO UPDATE
                SET hostname       = EXCLUDED.hostname,
                    capacity       = EXCLUDED.capacity,
                    last_heartbeat = EXCLUDED.last_heartbeat,
                    state          = CASE
                                       WHEN workers.state = 'Draining' THEN 'Draining'
                                       ELSE 'Active'
                                     END,
                    labels         = EXCLUDED.labels",
        )
        .bind(id)
        .bind(hostname)
        .bind(capacity)
        .bind(Utc::now())
        .bind(labels)
        .execute(&self.pool)
        .await
        .context("upsert_worker")?;
        Ok(())
    }

    async fn update_worker_heartbeat(&self, id: &str, active_tasks: i32) -> Result<()> {
        sqlx::query(
            "UPDATE workers SET last_heartbeat = $1, active_tasks = $2 WHERE id = $3",
        )
        .bind(Utc::now())
        .bind(active_tasks)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("update_worker_heartbeat")?;
        Ok(())
    }

    async fn mark_stale_workers_offline(&self, timeout_seconds: i64) -> Result<Vec<String>> {
        let cutoff = Utc::now() - chrono::Duration::seconds(timeout_seconds);

        // Collect stale worker IDs first
        let stale_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM workers WHERE last_heartbeat < $1 AND state <> 'Offline'",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .context("mark_stale_workers_offline: select")?;

        if !stale_ids.is_empty() {
            sqlx::query(
                "UPDATE workers SET state = 'Offline'
                 WHERE last_heartbeat < $1 AND state <> 'Offline'",
            )
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("mark_stale_workers_offline: update")?;
        }

        Ok(stale_ids)
    }

    async fn requeue_worker_tasks(&self, worker_id: &str) -> Result<usize> {
        let result = sqlx::query(
            "UPDATE task_instances SET state = 'Queued'
             WHERE worker_id = $1 AND state = 'Running'",
        )
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .context("requeue_worker_tasks")?;
        Ok(result.rows_affected() as usize)
    }

    async fn get_interrupted_tasks_by_worker(
        &self,
        worker_id: &str,
    ) -> Result<Vec<(String, String, String, String, String, String, String, i32, i32, i32)>> {
        let rows = sqlx::query(
            "SELECT ti.id,
                    ti.dag_id,
                    ti.task_id,
                    t.command,
                    dr.id AS run_id,
                    t.task_type,
                    t.config,
                    t.max_retries,
                    t.retry_delay_secs,
                    COALESCE(t.execution_timeout, 0) AS execution_timeout_secs
             FROM task_instances ti
             JOIN tasks    t  ON ti.task_id = t.id AND ti.dag_id = t.dag_id
             -- Bug 32 fix: JOIN on run_id (unique per run), not (dag_id, execution_date).
             -- Re-triggered runs share dag_id+execution_date, causing duplicate rows.
             JOIN dag_runs dr ON ti.run_id = dr.id
             WHERE ti.worker_id = $1 AND ti.state = 'Queued'",
        )
        .bind(worker_id)
        .fetch_all(&self.pool)
        .await
        .context("get_interrupted_tasks_by_worker")?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("id"),
                    r.get::<String, _>("dag_id"),
                    r.get::<String, _>("task_id"),
                    r.get::<String, _>("command"),
                    r.get::<String, _>("run_id"),
                    r.get::<String, _>("task_type"),
                    r.get::<String, _>("config"),
                    r.get::<i32, _>("max_retries"),
                    r.get::<i32, _>("retry_delay_secs"),
                    r.get::<i32, _>("execution_timeout_secs"),
                )
            })
            .collect())
    }

    async fn clear_worker_id_from_queued_tasks(&self, worker_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE task_instances SET worker_id = NULL
             WHERE worker_id = $1 AND state = 'Queued'",
        )
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .context("clear_worker_id_from_queued_tasks")?;
        Ok(())
    }

    // ── DAG versioning ────────────────────────────────────────────────────────

    async fn store_dag_version(&self, dag_id: &str, file_path: &str) -> Result<i64> {
        // Bug 23 fix: merge the SELECT MAX(version)+1 and the INSERT into a single
        // statement so two concurrent callers cannot compute the same version number.
        // Previously this was two round-trips (SELECT then INSERT), giving a TOCTOU
        // race where concurrent writes could produce duplicate version numbers.
        let next_version: i64 = sqlx::query_scalar(
            "INSERT INTO dag_versions (id, dag_id, version, file_path, created_at)
             SELECT
                 $1 || '-' || (COALESCE(MAX(version), 0) + 1)::text,
                 $1,
                 COALESCE(MAX(version), 0) + 1,
                 $2,
                 NOW()
             FROM dag_versions WHERE dag_id = $1
             RETURNING version",
        )
        .bind(dag_id)
        .bind(file_path)
        .fetch_one(&self.pool)
        .await
        .context("store_dag_version: atomic insert")?;

        Ok(next_version)
    }

    async fn get_dag_versions(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT id, dag_id, version, file_path, created_at
             FROM dag_versions
             WHERE dag_id = $1
             ORDER BY version DESC",
        )
        .bind(dag_id)
        .fetch_all(&self.pool)
        .await
        .context("get_dag_versions")?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":         r.get::<String, _>("id"),
                    "dag_id":     r.get::<String, _>("dag_id"),
                    "version":    r.get::<i64, _>("version"),
                    "file_path":  r.get::<String, _>("file_path"),
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                })
            })
            .collect())
    }

    async fn get_latest_version(&self, dag_id: &str) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT id, dag_id, version, file_path, created_at
             FROM dag_versions
             WHERE dag_id = $1
             ORDER BY version DESC
             LIMIT 1",
        )
        .bind(dag_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_latest_version")?;

        use sqlx::Row;
        Ok(row.map(|r| {
            serde_json::json!({
                "id":         r.get::<String, _>("id"),
                "dag_id":     r.get::<String, _>("dag_id"),
                "version":    r.get::<i64, _>("version"),
                "file_path":  r.get::<String, _>("file_path"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        }))
    }

    // ── XCom operations ───────────────────────────────────────────────────────

    async fn xcom_push(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let xcom_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO task_xcom (id, dag_id, task_id, run_id, key, value, timestamp)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (dag_id, task_id, run_id, key) DO UPDATE
                SET value     = EXCLUDED.value,
                    timestamp = EXCLUDED.timestamp",
        )
        .bind(xcom_id)
        .bind(dag_id)
        .bind(task_id)
        .bind(run_id)
        .bind(key)
        .bind(value)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .context("xcom_push")?;
        Ok(())
    }

    async fn xcom_pull(&self, dag_id: &str, task_id: &str, run_id: &str, key: &str) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT value FROM task_xcom WHERE dag_id=$1 AND task_id=$2 AND run_id=$3 AND key=$4")
            .bind(dag_id).bind(task_id).bind(run_id).bind(key).fetch_optional(&self.pool).await.context("xcom_pull")
    }

    async fn xcom_pull_all(&self, dag_id: &str, run_id: &str, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)> {
        let rows = sqlx::query("SELECT dag_id, task_id, run_id, key, value, timestamp, COUNT(*) OVER() as total_count FROM task_xcom WHERE dag_id=$1 AND run_id=$2 ORDER BY timestamp ASC LIMIT $3 OFFSET $4")
            .bind(dag_id).bind(run_id).bind(limit).bind(offset).fetch_all(&self.pool).await.context("xcom_pull_all")?;
        
        use sqlx::Row;
        let total = rows.first().map(|r| r.get::<i64, _>("total_count")).unwrap_or(0);
        
        let data = rows.iter().map(|r| serde_json::json!({
            "dag_id": r.get::<String, _>(0),
            "task_id": r.get::<String, _>(1),
            "run_id": r.get::<String, _>(2),
            "key": r.get::<String, _>(3),
            "value": r.get::<String, _>(4),
            "timestamp": r.get::<String, _>(5)
        })).collect();
        Ok((data, total))
    }

    // ── Task Pool operations ──────────────────────────────────────────────────

    async fn get_all_pools(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query("SELECT p.name, p.slots, p.description, COUNT(ps.id) FROM pools p LEFT JOIN pool_slots ps ON ps.pool_name = p.name GROUP BY p.name, p.slots, p.description")
            .fetch_all(&self.pool).await.context("get_all_pools")?;
        use sqlx::Row;
        Ok(rows.iter().map(|r| serde_json::json!({
            "name": r.get::<String, _>(0),
            "slots": r.get::<i32, _>(1),
            "description": r.get::<String, _>(2),
            "occupied_slots": r.get::<i64, _>(3)
        })).collect())
    }

    async fn get_pool(&self, name: &str) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query("SELECT p.name, p.slots, p.description, COUNT(ps.id) FROM pools p LEFT JOIN pool_slots ps ON ps.pool_name = p.name WHERE p.name=$1 GROUP BY p.name, p.slots, p.description")
            .bind(name).fetch_optional(&self.pool).await.context("get_pool")?;
        use sqlx::Row;
        Ok(row.map(|r| serde_json::json!({
            "name": r.get::<String, _>(0),
            "slots": r.get::<i32, _>(1),
            "description": r.get::<String, _>(2),
            "occupied_slots": r.get::<i64, _>(3)
        })))
    }

    async fn create_pool(&self, name: &str, slots: i32, description: &str) -> Result<()> {
        sqlx::query("INSERT INTO pools (name, slots, description) VALUES ($1, $2, $3)")
            .bind(name).bind(slots).bind(description).execute(&self.pool).await.context("create_pool")?;
        Ok(())
    }

    async fn update_pool(&self, name: &str, slots: i32, description: &str) -> Result<()> {
        sqlx::query("UPDATE pools SET slots=$1, description=$2 WHERE name=$3")
            .bind(slots).bind(description).bind(name).execute(&self.pool).await.context("update_pool")?;
        Ok(())
    }

    async fn delete_pool(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM pools WHERE name=$1").bind(name).execute(&self.pool).await.context("delete_pool")?;
        Ok(())
    }


    async fn get_callbacks(&self, dag_id: &str) -> Result<Option<serde_json::Value>> {
        let row: Option<String> = sqlx::query_scalar("SELECT config FROM dag_callbacks WHERE dag_id=$1").bind(dag_id).fetch_optional(&self.pool).await.context("get_callbacks")?;
        Ok(row.and_then(|s| serde_json::from_str(&s).ok()))
    }

    async fn save_callbacks(&self, dag_id: &str, config_json: &str) -> Result<()> {
        sqlx::query("INSERT INTO dag_callbacks (dag_id, config, updated_at) VALUES ($1, $2, $3) ON CONFLICT(dag_id) DO UPDATE SET config=EXCLUDED.config, updated_at=EXCLUDED.updated_at")
            .bind(dag_id).bind(config_json).bind(Utc::now().to_rfc3339()).execute(&self.pool).await.context("save_callbacks")?;
        Ok(())
    }

    async fn delete_callbacks(&self, dag_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM dag_callbacks WHERE dag_id=$1").bind(dag_id).execute(&self.pool).await.context("delete_callbacks")?;
        Ok(())
    }

    // ── Audit Logging (Postgres stubs — route to SQLite backend in practice) ──

    async fn log_audit_event(
        &self,
        actor: &str,
        action: &str,
        target_type: &str,
        target_id: &str,
        metadata: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_log (timestamp, actor, action, target_type, target_id, metadata)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(chrono::Utc::now())
        .bind(actor)
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(metadata)
        .execute(&self.pool)
        .await
        .context("log_audit_event")?;
        Ok(())
    }

    async fn get_audit_logs(
        &self,
        limit: i64,
        offset: i64,
        actor: Option<&str>,
        action: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        use sqlx::Row;
        // Improvement 45: include COUNT(*) OVER() so callers get total row count
        // for pagination without issuing a second query.
        let rows = match (actor, action) {
            (Some(a), Some(act)) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata,
                            COUNT(*) OVER() AS total_count
                     FROM audit_log WHERE actor=$1 AND action=$2
                     ORDER BY timestamp DESC LIMIT $3 OFFSET $4",
                )
                .bind(a).bind(act).bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
            (Some(a), None) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata,
                            COUNT(*) OVER() AS total_count
                     FROM audit_log WHERE actor=$1
                     ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                )
                .bind(a).bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
            (None, Some(act)) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata,
                            COUNT(*) OVER() AS total_count
                     FROM audit_log WHERE action=$1
                     ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                )
                .bind(act).bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata,
                            COUNT(*) OVER() AS total_count
                     FROM audit_log ORDER BY timestamp DESC LIMIT $1 OFFSET $2",
                )
                .bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
        };

        Ok(rows.iter().map(|r| serde_json::json!({
            "id":          r.get::<i64, _>("id"),
            "timestamp":   r.get::<chrono::DateTime<chrono::Utc>, _>("timestamp"),
            "actor":       r.get::<String, _>("actor"),
            "action":      r.get::<String, _>("action"),
            "target_type": r.get::<String, _>("target_type"),
            "target_id":   r.get::<String, _>("target_id"),
            "metadata":    serde_json::from_str::<serde_json::Value>(&r.get::<String, _>("metadata"))
                               .unwrap_or(serde_json::json!({})),
            // Improvement 45: total matching rows for pagination
            "total_count": r.get::<i64, _>("total_count"),
        })).collect())
    }

    async fn get_gantt_data(&self, dag_id: &str, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>> {
        use sqlx::Row;
        // PERF-4: bounded with LIMIT/OFFSET to prevent full table scans.
        let rows = sqlx::query(
            "SELECT task_id, run_id, state, start_time, end_time, duration_ms
             FROM task_instances
             WHERE dag_id = $1 AND start_time IS NOT NULL
             ORDER BY task_id, start_time
             LIMIT $2 OFFSET $3",
        )
        .bind(dag_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_gantt_data")?;

        let mut map: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
        for r in &rows {
            let task_id: String = r.get("task_id");
            let instance = serde_json::json!({
                "run_id":      r.get::<Option<String>, _>("run_id"),
                "state":       r.get::<String, _>("state"),
                "start_time":  r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("start_time"),
                "end_time":    r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("end_time"),
                "duration_ms": r.get::<Option<i64>, _>("duration_ms"),
            });
            map.entry(task_id).or_default().push(instance);
        }

        Ok(map.into_iter().map(|(task_id, instances)| serde_json::json!({
            "task_id": task_id,
            "instances": instances,
        })).collect())
    }

    // ── Multi-Tenancy (Teams) ─────────────────────────────────────────────────

    async fn get_team(&self, team_id: &str) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT id, name, description, max_concurrent_tasks, max_dags
             FROM teams WHERE id = $1",
        )
        .bind(team_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_team")?;

        use sqlx::Row;
        Ok(row.map(|r| {
            serde_json::json!({
                "id":                   r.get::<String, _>("id"),
                "name":                 r.get::<String, _>("name"),
                "description":          r.get::<Option<String>, _>("description"),
                "max_concurrent_tasks": r.get::<i32, _>("max_concurrent_tasks"),
                "max_dags":             r.get::<i32, _>("max_dags"),
            })
        }))
    }

    async fn get_all_teams(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT id, name, description, max_concurrent_tasks, max_dags FROM teams",
        )
        .fetch_all(&self.pool)
        .await
        .context("get_all_teams")?;

        use sqlx::Row;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":                   r.get::<String, _>("id"),
                    "name":                 r.get::<String, _>("name"),
                    "description":          r.get::<Option<String>, _>("description"),
                    "max_concurrent_tasks": r.get::<i32, _>("max_concurrent_tasks"),
                    "max_dags":             r.get::<i32, _>("max_dags"),
                })
            })
            .collect())
    }

    async fn create_team(
        &self,
        id: &str,
        name: &str,
        description: &str,
        max_concurrent_tasks: i32,
        max_dags: i32,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO teams (id, name, description, max_concurrent_tasks, max_dags)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(max_concurrent_tasks)
        .bind(max_dags)
        .execute(&self.pool)
        .await
        .context("create_team")?;
        Ok(())
    }

    async fn update_team(
        &self,
        id: &str,
        name: &Option<String>,
        description: &Option<String>,
        max_concurrent_tasks: Option<i32>,
        max_dags: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE teams 
             SET name = COALESCE($2, name),
                 description = COALESCE($3, description),
                 max_concurrent_tasks = COALESCE($4, max_concurrent_tasks),
                 max_dags = COALESCE($5, max_dags)
             WHERE id = $1",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(max_concurrent_tasks)
        .bind(max_dags)
        .execute(&self.pool)
        .await
        .context("update_team")?;

        Ok(())
    }

    async fn delete_team(&self, id: &str) -> Result<()> {
        // Must unassign all users and DAGs first to not violate FK, or cascade delete.
        // Depending on product specifics. We'll simply issue DELETE.
        sqlx::query("DELETE FROM teams WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("delete_team")?;
        Ok(())
    }

    async fn assign_user_to_team(&self, username: &str, team_id: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE users SET team_id = $1 WHERE username = $2")
            .bind(team_id)
            .bind(username)
            .execute(&self.pool)
            .await
            .context("assign_user_to_team")?;
        Ok(())
    }

    // ── High Availability (HA) Advisory Locks ─────────────────────────────────
    //
    // Bug 15 fix: replaced pg_try_advisory_lock (session-scoped) with a
    // heartbeat-based leader_election table. With a connection pool, the session
    // holding a pg_advisory_lock can be recycled at any time, silently releasing
    // the lock and causing split-brain (two leaders at once).
    //
    // The leader_election table has a single row (lock_key = 1). The holder must
    // renew their lease every ~10s. An expired lease can be stolen by another node.

    async fn try_acquire_leader_lock(&self) -> Result<bool> {
        let expires = Utc::now() + chrono::Duration::seconds(30);
        // Upsert the single leader row:
        //  - Insert if the table is empty
        //  - Update if the existing lease has expired OR if we already hold it
        //  - Return the node_id that ends up in the row after the upsert
        let row: Option<(String,)> = sqlx::query_as(
            "INSERT INTO leader_election (lock_key, node_id, expires_at)
             VALUES (1, $1, $2)
             ON CONFLICT (lock_key) DO UPDATE
               SET node_id    = EXCLUDED.node_id,
                   expires_at = EXCLUDED.expires_at
             WHERE leader_election.expires_at < NOW()   -- steal expired lease
                OR leader_election.node_id  = $1        -- or renew our own lease
             RETURNING node_id",
        )
        .bind(&self.node_id)
        .bind(expires)
        .fetch_optional(&self.pool)
        .await
        .context("try_acquire_leader_lock")?;

        // We hold the lock iff we got a row back (the upsert succeeded)
        Ok(row.is_some())
    }

    async fn release_leader_lock(&self) -> Result<()> {
        sqlx::query("DELETE FROM leader_election WHERE node_id = $1")
            .bind(&self.node_id)
            .execute(&self.pool)
            .await
            .context("release_leader_lock")?;
        Ok(())
    }

    /// BUG-H5 fix: Uses INSERT-first approach to eliminate the TOCTOU race in
    /// pool slot acquisition. The slot claim is inserted first, then a serialized
    /// capacity check (via FOR UPDATE on the pool row) verifies the pool is not
    /// over capacity. If over capacity, the claim is removed within the same
    /// transaction before committing.
    async fn acquire_pool_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await.context("acquire_pool_slot: begin tx")?;

        // Step 1: INSERT the slot claim first (INSERT-first eliminates TOCTOU).
        // ON CONFLICT handles the case where this task already holds a slot.
        let insert_result = sqlx::query(
            "INSERT INTO pool_slots (id, pool_name, task_instance_id, acquired_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (pool_name, task_instance_id) DO NOTHING"
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(pool_name)
        .bind(task_instance_id)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await
        .context("acquire_pool_slot: insert slot claim")?;

        if insert_result.rows_affected() == 0 {
            // Task already holds a slot in this pool — idempotent success
            tx.commit().await.context("acquire_pool_slot: commit (already held)")?;
            return Ok(true);
        }

        // Step 2: Lock the pool row to serialize the capacity check across
        // concurrent transactions, then verify we haven't exceeded the limit.
        let pool_row = sqlx::query(
            "SELECT slots FROM pools WHERE name = $1 FOR UPDATE"
        )
        .bind(pool_name)
        .fetch_optional(&mut *tx)
        .await
        .context("acquire_pool_slot: lock pool row")?;

        let max_slots: i32 = match pool_row {
            Some(row) => {
                use sqlx::Row;
                row.get("slots")
            }
            None => {
                tx.rollback().await.ok();
                return Err(anyhow::anyhow!("Pool '{}' not found", pool_name));
            }
        };

        let occupied: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pool_slots WHERE pool_name = $1"
        )
        .bind(pool_name)
        .fetch_one(&mut *tx)
        .await
        .context("acquire_pool_slot: count slots")?;

        if occupied > max_slots as i64 {
            // Over capacity — remove our claim and report failure
            sqlx::query(
                "DELETE FROM pool_slots WHERE pool_name = $1 AND task_instance_id = $2"
            )
            .bind(pool_name)
            .bind(task_instance_id)
            .execute(&mut *tx)
            .await
            .context("acquire_pool_slot: remove over-capacity claim")?;

            tx.commit().await.context("acquire_pool_slot: commit (over capacity)")?;
            return Ok(false);
        }

        tx.commit().await.context("acquire_pool_slot: commit")?;
        Ok(true)
    }

    async fn release_pool_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM pool_slots WHERE pool_name = $1 AND task_instance_id = $2"
        )
        .bind(pool_name)
        .bind(task_instance_id)
        .execute(&self.pool)
        .await
        .context("release_pool_slot")?;
        Ok(())
    }

    async fn get_task_instance_details_full(
        &self,
        ti_id: &str,
    ) -> Result<Option<(String, String, String, String, String, String, i32, i32, i32)>> {
        self.get_task_instance_details(ti_id).await
    }

    /// Improvement 42: lightweight DB connectivity check used by GET /health.
    async fn ping(&self) -> bool {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .is_ok()
    }

    // ── Auth Sessions (IAM) ─────────────────────────────────────────

    async fn create_session(&self, session: &crate::auth::UserSession) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_sessions (session_id, username, provider_id, access_token, refresh_token, id_token, expires_at, created_at, ip_address, user_agent)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), $8, $9)
             ON CONFLICT (session_id) DO UPDATE SET expires_at = $7, access_token = $4, refresh_token = $5"
        )
        .bind(&session.session_id)
        .bind(&session.username)
        .bind(&session.provider_id)
        .bind(&session.access_token)
        .bind(&session.refresh_token)
        .bind(&session.id_token)
        .bind(session.expires_at)
        .bind(&session.ip_address)
        .bind(&session.user_agent)
        .execute(&self.pool)
        .await
        .context("create_session")?;
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<crate::auth::UserSession>> {
        let row = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<String>, chrono::DateTime<Utc>, Option<String>, Option<String>)>(
            "SELECT session_id, username, provider_id, access_token, refresh_token, id_token, expires_at, ip_address, user_agent
             FROM user_sessions WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_session")?;

        Ok(row.map(|(sid, username, provider_id, access_token, refresh_token, id_token, expires_at, ip_address, user_agent)| {
            crate::auth::UserSession {
                session_id: sid,
                username,
                provider_id,
                access_token,
                refresh_token,
                id_token,
                expires_at,
                ip_address,
                user_agent,
            }
        }))
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM user_sessions WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .context("delete_session")?;
        Ok(())
    }

    async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM user_sessions WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await
            .context("cleanup_expired_sessions")?;
        Ok(result.rows_affected())
    }

    async fn get_auth_providers(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (String, String, String, String, bool, i32)>(
            "SELECT id, provider_type, name, config, enabled, priority FROM auth_providers ORDER BY priority ASC"
        )
        .fetch_all(&self.pool)
        .await
        .context("get_auth_providers")?;

        Ok(rows.iter().map(|(id, ptype, name, config, enabled, priority)| {
            serde_json::json!({
                "id": id,
                "provider_type": ptype,
                "name": name,
                "config": serde_json::from_str::<serde_json::Value>(config).unwrap_or(serde_json::json!({})),
                "enabled": enabled,
                "priority": priority,
            })
        }).collect())
    }

    async fn get_auth_provider(&self, provider_id: &str) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query_as::<_, (String, String, String, String, bool, i32)>(
            "SELECT id, provider_type, name, config, enabled, priority FROM auth_providers WHERE id = $1"
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .context("get_auth_provider")?;

        Ok(row.map(|(id, ptype, name, config, enabled, priority)| {
            serde_json::json!({
                "id": id,
                "provider_type": ptype,
                "name": name,
                "config": serde_json::from_str::<serde_json::Value>(&config).unwrap_or(serde_json::json!({})),
                "enabled": enabled,
                "priority": priority,
            })
        }))
    }

    async fn upsert_auth_provider(
        &self,
        id: &str,
        provider_type: &str,
        name: &str,
        config: &str,
        enabled: bool,
        priority: i32,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO auth_providers (id, provider_type, name, config, enabled, priority, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
             ON CONFLICT (id) DO UPDATE SET name = $3, config = $4, enabled = $5, priority = $6, updated_at = NOW()"
        )
        .bind(id)
        .bind(provider_type)
        .bind(name)
        .bind(config)
        .bind(enabled)
        .bind(priority)
        .execute(&self.pool)
        .await
        .context("upsert_auth_provider")?;
        Ok(())
    }

    async fn delete_auth_provider(&self, provider_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM auth_providers WHERE id = $1 AND id != 'local'")
            .bind(provider_id)
            .execute(&self.pool)
            .await
            .context("delete_auth_provider")?;
        Ok(())
    }

    async fn update_user_last_login(&self, username: &str) -> Result<()> {
        sqlx::query("UPDATE users SET last_login = NOW() WHERE username = $1")
            .bind(username)
            .execute(&self.pool)
            .await
            .context("update_user_last_login")?;
        Ok(())
    }

    // ── Lineage (Observability) ─────────────────────────────────────

    async fn store_lineage_event(
        &self,
        event_type: &str,
        run_id: &str,
        dag_id: &str,
        task_id: Option<&str>,
        job_namespace: &str,
        job_name: &str,
        inputs: &str,
        outputs: &str,
        facets: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO lineage_events (id, event_type, event_time, run_id, dag_id, task_id, job_namespace, job_name, producer, inputs, outputs, facets)
             VALUES ($1, $2, NOW(), $3, $4, $5, $6, $7, 'vortex', $8::jsonb, $9::jsonb, $10::jsonb)"
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(event_type)
        .bind(run_id)
        .bind(dag_id)
        .bind(task_id)
        .bind(job_namespace)
        .bind(job_name)
        .bind(inputs)
        .bind(outputs)
        .bind(facets)
        .execute(&self.pool)
        .await
        .context("store_lineage_event")?;
        Ok(())
    }

    async fn get_lineage_events(
        &self,
        dag_id: &str,
        run_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let rows = if let Some(rid) = run_id {
            sqlx::query_as::<_, (String, String, chrono::DateTime<Utc>, String, String, Option<String>, String, String, serde_json::Value, serde_json::Value)>(
                "SELECT id, event_type, event_time, run_id, dag_id, task_id, job_name, producer, inputs, outputs
                 FROM lineage_events WHERE dag_id = $1 AND run_id = $2 ORDER BY event_time DESC LIMIT $3"
            )
            .bind(dag_id)
            .bind(rid)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .context("get_lineage_events")?
        } else {
            sqlx::query_as::<_, (String, String, chrono::DateTime<Utc>, String, String, Option<String>, String, String, serde_json::Value, serde_json::Value)>(
                "SELECT id, event_type, event_time, run_id, dag_id, task_id, job_name, producer, inputs, outputs
                 FROM lineage_events WHERE dag_id = $1 ORDER BY event_time DESC LIMIT $2"
            )
            .bind(dag_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .context("get_lineage_events")?
        };

        Ok(rows.iter().map(|(id, event_type, event_time, run_id, dag_id, task_id, job_name, producer, inputs, outputs)| {
            serde_json::json!({
                "id": id,
                "event_type": event_type,
                "event_time": event_time.to_rfc3339(),
                "run_id": run_id,
                "dag_id": dag_id,
                "task_id": task_id,
                "job_name": job_name,
                "producer": producer,
                "inputs": inputs,
                "outputs": outputs,
            })
        }).collect())
    }

    async fn get_lineage_datasets(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (String, String, String, String, serde_json::Value)>(
            "SELECT id, namespace, name, source_type, facets FROM lineage_datasets ORDER BY updated_at DESC LIMIT $1 OFFSET $2"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_lineage_datasets")?;

        Ok(rows.iter().map(|(id, namespace, name, source_type, facets)| {
            serde_json::json!({
                "id": id,
                "namespace": namespace,
                "name": name,
                "source_type": source_type,
                "facets": facets,
            })
        }).collect())
    }

    async fn get_incident_configs(&self, team_id: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let rows = if let Some(tid) = team_id {
            sqlx::query_as::<_, (String, Option<String>, String, String, serde_json::Value, bool)>(
                "SELECT id, team_id, provider, name, config, enabled FROM incident_configs WHERE team_id = $1 OR team_id IS NULL ORDER BY name"
            )
            .bind(tid)
            .fetch_all(&self.pool)
            .await
            .context("get_incident_configs")?
        } else {
            sqlx::query_as::<_, (String, Option<String>, String, String, serde_json::Value, bool)>(
                "SELECT id, team_id, provider, name, config, enabled FROM incident_configs ORDER BY name"
            )
            .fetch_all(&self.pool)
            .await
            .context("get_incident_configs")?
        };

        Ok(rows.iter().map(|(id, team_id, provider, name, config, enabled)| {
            serde_json::json!({
                "id": id,
                "team_id": team_id,
                "provider": provider,
                "name": name,
                "config": config,
                "enabled": enabled,
            })
        }).collect())
    }

    async fn upsert_incident_config(
        &self,
        id: &str,
        team_id: Option<&str>,
        provider: &str,
        name: &str,
        config: &str,
        enabled: bool,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO incident_configs (id, team_id, provider, name, config, enabled, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5::jsonb, $6, NOW(), NOW())
             ON CONFLICT (id) DO UPDATE SET name = $4, config = $5::jsonb, enabled = $6, updated_at = NOW()"
        )
        .bind(id)
        .bind(team_id)
        .bind(provider)
        .bind(name)
        .bind(config)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .context("upsert_incident_config")?;
        Ok(())
    }

    async fn delete_incident_config(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM incident_configs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("delete_incident_config")?;
        Ok(())
    }

    // ── Compliance & Governance ──────────────────────────────────

    async fn insert_audit_log(&self, entry: &crate::compliance::AuditEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_log (event_type, actor, actor_ip, resource_type, resource_id, action, details, team_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        )
        .bind(&entry.event_type)
        .bind(&entry.actor)
        .bind(&entry.actor_ip)
        .bind(&entry.resource_type)
        .bind(&entry.resource_id)
        .bind(&entry.action)
        .bind(&entry.details)
        .bind(&entry.team_id)
        .execute(&self.pool)
        .await
        .context("insert_audit_log")?;
        Ok(())
    }

    async fn get_audit_log(
        &self,
        event_type: Option<&str>,
        actor: Option<&str>,
        resource_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT id, event_type, actor, actor_ip, resource_type, resource_id, action, details, team_id, created_at
                FROM audit_log
                WHERE ($1::text IS NULL OR event_type = $1)
                  AND ($2::text IS NULL OR actor = $2)
                  AND ($3::text IS NULL OR resource_type = $3)
                ORDER BY created_at DESC
                LIMIT $4 OFFSET $5
            ) t"
        )
        .bind(event_type)
        .bind(actor)
        .bind(resource_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("get_audit_log")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn find_matching_approval_gate(&self, resource_type: &str, resource_id: &str) -> Result<Option<serde_json::Value>> {
        // SECURITY (BUG-H7): Escape SQL LIKE metacharacters in user-supplied resource_id
        // to prevent pattern injection (e.g., "%" matching everything).
        let safe_resource_id = escape_like_pattern(resource_id);
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM approval_gates
                WHERE enabled = TRUE AND resource_type = $1 AND $2 LIKE replace(replace(resource_pattern, '*', '%'), '?', '_')
                LIMIT 1
            ) t"
        )
        .bind(resource_type)
        .bind(&safe_resource_id)
        .fetch_optional(&self.pool)
        .await
        .context("find_matching_approval_gate")?;
        Ok(row.map(|r| r.0))
    }

    async fn get_approval_gates(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (SELECT * FROM approval_gates ORDER BY created_at DESC) t"
        )
        .fetch_all(&self.pool)
        .await
        .context("get_approval_gates")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn upsert_approval_gate(
        &self, id: &str, name: &str, resource_type: &str, resource_pattern: &str,
        required_approvers: i32, approver_roles: &[String], enabled: bool,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO approval_gates (id, name, resource_type, resource_pattern, required_approvers, approver_roles, enabled, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
             ON CONFLICT (id) DO UPDATE SET
               name = EXCLUDED.name, resource_type = EXCLUDED.resource_type,
               resource_pattern = EXCLUDED.resource_pattern, required_approvers = EXCLUDED.required_approvers,
               approver_roles = EXCLUDED.approver_roles, enabled = EXCLUDED.enabled, updated_at = NOW()"
        )
        .bind(id)
        .bind(name)
        .bind(resource_type)
        .bind(resource_pattern)
        .bind(required_approvers)
        .bind(approver_roles)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .context("upsert_approval_gate")?;
        Ok(())
    }

    async fn delete_approval_gate(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM approval_gates WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("delete_approval_gate")?;
        Ok(())
    }

    async fn create_approval_request(
        &self, gate_id: &str, requester: &str, resource_type: &str, resource_id: &str,
        description: Option<&str>, diff: &serde_json::Value,
    ) -> Result<String> {
        let row = sqlx::query_as::<_, (String,)>(
            "INSERT INTO approval_requests (gate_id, requester, resource_type, resource_id, change_description, change_diff)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id"
        )
        .bind(gate_id)
        .bind(requester)
        .bind(resource_type)
        .bind(resource_id)
        .bind(description)
        .bind(diff)
        .fetch_one(&self.pool)
        .await
        .context("create_approval_request")?;
        Ok(row.0)
    }

    async fn get_approval_requests(&self, status: Option<&str>, limit: i64) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM approval_requests
                WHERE ($1::text IS NULL OR status = $1)
                ORDER BY created_at DESC LIMIT $2
            ) t"
        )
        .bind(status)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("get_approval_requests")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn add_approval_vote(&self, request_id: &str, approver: &str, comment: Option<&str>) -> Result<String> {
        // BUG-M12 FIX: Wrap vote insertion and status update in a single
        // transaction so a vote can never be recorded without the subsequent
        // status check/update succeeding atomically.
        let mut tx = self.pool.begin().await.context("add_approval_vote: begin tx")?;

        let vote = serde_json::json!({
            "approver": approver,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "comment": comment.unwrap_or("")
        });
        sqlx::query(
            "UPDATE approval_requests SET approvals = approvals || $2::jsonb WHERE id = $1"
        )
        .bind(request_id)
        .bind(&vote)
        .execute(&mut *tx)
        .await
        .context("add_approval_vote")?;

        // Check if we have enough approvals to auto-approve
        let row = sqlx::query_as::<_, (String, i64, i32)>(
            "SELECT ar.status, jsonb_array_length(ar.approvals), ag.required_approvers
             FROM approval_requests ar JOIN approval_gates ag ON ar.gate_id = ag.id
             WHERE ar.id = $1"
        )
        .bind(request_id)
        .fetch_one(&mut *tx)
        .await
        .context("check_approval_count")?;

        let (status, approval_count, required) = row;
        let result = if status == "pending" && approval_count >= required as i64 {
            sqlx::query("UPDATE approval_requests SET status = 'approved', resolved_at = NOW() WHERE id = $1")
                .bind(request_id)
                .execute(&mut *tx)
                .await
                .context("add_approval_vote: approve")?;
            "approved".to_string()
        } else {
            "pending".to_string()
        };

        tx.commit().await.context("add_approval_vote: commit")?;
        Ok(result)
    }

    async fn reject_approval_request(&self, request_id: &str, rejector: &str, reason: Option<&str>) -> Result<()> {
        let rejection = serde_json::json!({
            "rejector": rejector,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "reason": reason.unwrap_or("")
        });
        sqlx::query(
            "UPDATE approval_requests SET status = 'rejected', rejections = rejections || $2::jsonb, resolved_at = NOW() WHERE id = $1"
        )
        .bind(request_id)
        .bind(&rejection)
        .execute(&self.pool)
        .await
        .context("reject_approval_request")?;
        Ok(())
    }

    async fn get_retention_policies(&self, enabled_only: bool) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM retention_policies WHERE ($1 = FALSE OR enabled = TRUE) ORDER BY created_at
            ) t"
        )
        .bind(enabled_only)
        .fetch_all(&self.pool)
        .await
        .context("get_retention_policies")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn upsert_retention_policy(
        &self, id: &str, name: &str, target_table: &str, retention_days: i32,
        batch_size: i32, enabled: bool,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO retention_policies (id, name, target_table, retention_days, delete_batch_size, enabled)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET
               name = EXCLUDED.name, target_table = EXCLUDED.target_table,
               retention_days = EXCLUDED.retention_days, delete_batch_size = EXCLUDED.delete_batch_size,
               enabled = EXCLUDED.enabled"
        )
        .bind(id)
        .bind(name)
        .bind(target_table)
        .bind(retention_days)
        .bind(batch_size)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .context("upsert_retention_policy")?;
        Ok(())
    }

    async fn execute_retention_delete(&self, table: &str, retention_days: i64, batch_size: i64) -> Result<i64> {
        // Only allow known tables to prevent SQL injection
        let allowed = ["dag_runs", "task_instances", "audit_log", "lineage_events"];
        if !allowed.contains(&table) {
            anyhow::bail!("Retention delete not allowed on table: {}", table);
        }
        // PERF-3: use stable `id` column (not `ctid` which changes after VACUUM).
        // Loop until no rows remain to delete, processing in batches to bound memory usage.
        let query = format!(
            "DELETE FROM {} WHERE id IN (SELECT id FROM {} WHERE created_at < NOW() - INTERVAL '{} days' LIMIT {})",
            table, table, retention_days, batch_size
        );
        let mut total_deleted: i64 = 0;
        loop {
            let result = sqlx::query(&query)
                .execute(&self.pool)
                .await
                .context("execute_retention_delete")?;
            let deleted = result.rows_affected() as i64;
            total_deleted += deleted;
            if deleted == 0 {
                break;
            }
        }
        Ok(total_deleted)
    }

    async fn update_retention_last_run(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE retention_policies SET last_run_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("update_retention_last_run")?;
        Ok(())
    }

    async fn get_compliance_controls(&self, framework: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM compliance_controls
                WHERE ($1::text IS NULL OR framework = $1)
                ORDER BY framework, control_id
            ) t"
        )
        .bind(framework)
        .fetch_all(&self.pool)
        .await
        .context("get_compliance_controls")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn upsert_compliance_control(
        &self, framework: &str, control_id: &str, description: &str,
        status: &str, evidence: &serde_json::Value, assessor: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO compliance_controls (framework, control_id, description, status, evidence, assessed_by, assessed_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
             ON CONFLICT (framework, control_id) DO UPDATE SET
               description = CASE WHEN EXCLUDED.description = '' THEN compliance_controls.description ELSE EXCLUDED.description END,
               status = EXCLUDED.status, evidence = EXCLUDED.evidence,
               assessed_by = EXCLUDED.assessed_by, assessed_at = NOW(), updated_at = NOW()"
        )
        .bind(framework)
        .bind(control_id)
        .bind(description)
        .bind(status)
        .bind(evidence)
        .bind(assessor)
        .execute(&self.pool)
        .await
        .context("upsert_compliance_control")?;
        Ok(())
    }

    // ── Fine-Grained RBAC & Token Scoping ───────────────────────

    async fn check_user_permission(&self, user_id: &str, permission: &str, team_id: Option<&str>) -> Result<bool> {
        let row = sqlx::query_as::<_, (bool,)>(
            "SELECT EXISTS(
                SELECT 1 FROM rbac_user_roles ur
                JOIN rbac_role_permissions rp ON ur.role_id = rp.role_id
                JOIN rbac_permissions p ON rp.permission_id = p.id
                WHERE ur.user_id = $1 AND p.name = $2
                  AND (ur.team_id IS NULL OR ur.team_id = $3)
            )"
        )
        .bind(user_id)
        .bind(permission)
        .bind(team_id)
        .fetch_one(&self.pool)
        .await
        .context("check_user_permission")?;
        Ok(row.0)
    }

    async fn get_user_effective_permissions(&self, user_id: &str, team_id: Option<&str>) -> Result<Vec<String>> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT p.name FROM rbac_user_roles ur
             JOIN rbac_role_permissions rp ON ur.role_id = rp.role_id
             JOIN rbac_permissions p ON rp.permission_id = p.id
             WHERE ur.user_id = $1 AND (ur.team_id IS NULL OR ur.team_id = $2)
             ORDER BY p.name"
        )
        .bind(user_id)
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .context("get_user_effective_permissions")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_rbac_roles(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (SELECT * FROM rbac_roles ORDER BY name) t"
        )
        .fetch_all(&self.pool)
        .await
        .context("get_rbac_roles")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_rbac_role_permissions(&self, role_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT p.* FROM rbac_role_permissions rp
                JOIN rbac_permissions p ON rp.permission_id = p.id
                WHERE rp.role_id = $1 ORDER BY p.name
            ) t"
        )
        .bind(role_id)
        .fetch_all(&self.pool)
        .await
        .context("get_rbac_role_permissions")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn assign_user_role(&self, user_id: &str, role_id: &str, team_id: Option<&str>, granted_by: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO rbac_user_roles (user_id, role_id, team_id, granted_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id, role_id, COALESCE(team_id, '__global__')) DO NOTHING"
        )
        .bind(user_id)
        .bind(role_id)
        .bind(team_id)
        .bind(granted_by)
        .execute(&self.pool)
        .await
        .context("assign_user_role")?;
        Ok(())
    }

    async fn revoke_user_role(&self, user_id: &str, role_id: &str, team_id: Option<&str>) -> Result<()> {
        sqlx::query(
            "DELETE FROM rbac_user_roles WHERE user_id = $1 AND role_id = $2 AND (($3::text IS NULL AND team_id IS NULL) OR team_id = $3)"
        )
        .bind(user_id)
        .bind(role_id)
        .bind(team_id)
        .execute(&self.pool)
        .await
        .context("revoke_user_role")?;
        Ok(())
    }

    async fn get_user_roles(&self, user_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT r.*, ur.team_id, ur.granted_by, ur.granted_at
                FROM rbac_user_roles ur JOIN rbac_roles r ON ur.role_id = r.id
                WHERE ur.user_id = $1
                ORDER BY r.name
            ) t"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("get_user_roles")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn create_api_token(&self, name: &str, token_hash: &str, user_id: &str, scopes: &[String], team_id: Option<&str>, expires_at: Option<&str>) -> Result<String> {
        let row = sqlx::query_as::<_, (String,)>(
            "INSERT INTO api_tokens (name, token_hash, user_id, scopes, team_id, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6::timestamptz)
             RETURNING id"
        )
        .bind(name)
        .bind(token_hash)
        .bind(user_id)
        .bind(scopes)
        .bind(team_id)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await
        .context("create_api_token")?;
        Ok(row.0)
    }

    async fn get_api_tokens(&self, user_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT id, name, user_id, scopes, team_id, expires_at, last_used_at, created_at, revoked
                FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC
            ) t"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("get_api_tokens")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn revoke_api_token(&self, token_id: &str) -> Result<()> {
        sqlx::query("UPDATE api_tokens SET revoked = TRUE WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await
            .context("revoke_api_token")?;
        Ok(())
    }

    async fn find_api_token_by_hash(&self, _token_prefix: &str) -> Result<Option<serde_json::Value>> {
        // For token lookup, we need to check all non-revoked, non-expired tokens
        // In production, store a prefix index for faster lookup
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM api_tokens
                WHERE revoked = FALSE AND (expires_at IS NULL OR expires_at > NOW())
                ORDER BY created_at DESC LIMIT 100
            ) t"
        )
        .fetch_all(&self.pool)
        .await
        .context("find_api_token_by_hash")?;
        // Caller will verify bcrypt hash against each candidate
        // Return first match or None; actual matching done in rbac module
        Ok(row.into_iter().map(|r| r.0).next())
    }

    async fn update_token_last_used(&self, token_id: &str) -> Result<()> {
        sqlx::query("UPDATE api_tokens SET last_used_at = NOW() WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await
            .context("update_token_last_used")?;
        Ok(())
    }

    async fn get_ip_allowlist(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (SELECT * FROM ip_allowlist ORDER BY created_at) t"
        )
        .fetch_all(&self.pool)
        .await
        .context("get_ip_allowlist")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn upsert_ip_allowlist_rule(&self, id: &str, cidr: &str, description: &str, enabled: bool) -> Result<()> {
        sqlx::query(
            "INSERT INTO ip_allowlist (id, cidr, description, enabled)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (id) DO UPDATE SET cidr = EXCLUDED.cidr, description = EXCLUDED.description, enabled = EXCLUDED.enabled"
        )
        .bind(id)
        .bind(cidr)
        .bind(description)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .context("upsert_ip_allowlist_rule")?;
        Ok(())
    }

    async fn delete_ip_allowlist_rule(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM ip_allowlist WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("delete_ip_allowlist_rule")?;
        Ok(())
    }

    // ── Advanced Scheduling & Data-Aware Orchestration ──────────

    async fn upsert_dataset(&self, id: &str, uri: &str, name: &str, description: Option<&str>, producer_dag_id: Option<&str>, metadata: &serde_json::Value) -> Result<()> {
        sqlx::query(
            "INSERT INTO datasets (id, uri, name, description, producer_dag_id, metadata, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW())
             ON CONFLICT (id) DO UPDATE SET uri = EXCLUDED.uri, name = EXCLUDED.name,
               description = EXCLUDED.description, producer_dag_id = EXCLUDED.producer_dag_id,
               metadata = EXCLUDED.metadata, updated_at = NOW()"
        )
        .bind(id).bind(uri).bind(name).bind(description).bind(producer_dag_id).bind(metadata)
        .execute(&self.pool).await.context("upsert_dataset")?;
        Ok(())
    }

    async fn get_datasets(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (SELECT * FROM datasets ORDER BY name LIMIT $1 OFFSET $2) t"
        ).bind(limit).bind(offset).fetch_all(&self.pool).await.context("get_datasets")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn insert_dataset_event(&self, event: &crate::advanced_scheduler::DatasetEvent) -> Result<()> {
        sqlx::query(
            "INSERT INTO dataset_events (dataset_id, source_dag_id, source_task_id, source_run_id, event_type, metadata)
             VALUES ($1, $2, $3, $4, $5, $6)"
        )
        .bind(&event.dataset_id).bind(&event.source_dag_id).bind(&event.source_task_id)
        .bind(&event.source_run_id).bind(&event.event_type).bind(&event.metadata)
        .execute(&self.pool).await.context("insert_dataset_event")?;
        Ok(())
    }

    async fn get_dataset_events(&self, dataset_id: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM dataset_events WHERE dataset_id = $1 ORDER BY created_at DESC LIMIT $2
            ) t"
        ).bind(dataset_id).bind(limit).fetch_all(&self.pool).await.context("get_dataset_events")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn upsert_dataset_trigger(&self, id: &str, dag_id: &str, dataset_ids: &[String], condition: &str, min_interval: Option<i32>, enabled: bool) -> Result<()> {
        sqlx::query(
            "INSERT INTO dataset_triggers (id, dag_id, dataset_ids, condition, min_interval_seconds, enabled)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO UPDATE SET dag_id = EXCLUDED.dag_id, dataset_ids = EXCLUDED.dataset_ids,
               condition = EXCLUDED.condition, min_interval_seconds = EXCLUDED.min_interval_seconds, enabled = EXCLUDED.enabled"
        )
        .bind(id).bind(dag_id).bind(dataset_ids).bind(condition).bind(min_interval).bind(enabled)
        .execute(&self.pool).await.context("upsert_dataset_trigger")?;
        Ok(())
    }

    async fn get_dataset_triggers_for_dataset(&self, dataset_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM dataset_triggers WHERE enabled = TRUE AND $1 = ANY(dataset_ids)
            ) t"
        ).bind(dataset_id).fetch_all(&self.pool).await.context("get_dataset_triggers_for_dataset")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn check_all_datasets_updated(&self, dataset_ids: &[String], _trigger_id: &str) -> Result<bool> {
        // Check that each dataset has at least one recent event
        for ds_id in dataset_ids {
            let row = sqlx::query_as::<_, (bool,)>(
                "SELECT EXISTS(SELECT 1 FROM dataset_events WHERE dataset_id = $1 AND created_at > NOW() - INTERVAL '24 hours')"
            ).bind(ds_id).fetch_one(&self.pool).await.context("check_dataset_updated")?;
            if !row.0 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn upsert_cross_dag_dependency(&self, id: &str, downstream: &str, upstream: &str, upstream_task: Option<&str>, condition: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO cross_dag_dependencies (id, downstream_dag_id, upstream_dag_id, upstream_task_id, condition)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (downstream_dag_id, upstream_dag_id, COALESCE(upstream_task_id, '__all__'))
             DO UPDATE SET condition = EXCLUDED.condition"
        )
        .bind(id).bind(downstream).bind(upstream).bind(upstream_task).bind(condition)
        .execute(&self.pool).await.context("upsert_cross_dag_dependency")?;
        Ok(())
    }

    async fn get_cross_dag_dependencies(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT row_to_json(t) FROM (
                SELECT * FROM cross_dag_dependencies WHERE downstream_dag_id = $1 AND enabled = TRUE
            ) t"
        ).bind(dag_id).fetch_all(&self.pool).await.context("get_cross_dag_dependencies")?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn check_upstream_completed(&self, upstream_dag: &str, upstream_task: Option<&str>, condition: &str) -> Result<bool> {
        let status_check = match condition {
            "success" => "'success'",
            "complete" => "'success', 'failed'",
            _ => "'success', 'failed', 'running'",
        };
        let query = if upstream_task.is_some() {
            format!(
                "SELECT EXISTS(SELECT 1 FROM task_instances WHERE dag_id = $1 AND task_id = $2 AND status IN ({}) AND updated_at > NOW() - INTERVAL '24 hours')",
                status_check
            )
        } else {
            format!(
                "SELECT EXISTS(SELECT 1 FROM dag_runs WHERE dag_id = $1 AND status IN ({}) AND updated_at > NOW() - INTERVAL '24 hours')",
                status_check
            )
        };
        let row = sqlx::query_as::<_, (bool,)>(&query)
            .bind(upstream_dag)
            .bind(upstream_task.unwrap_or(""))
            .fetch_one(&self.pool)
            .await
            .context("check_upstream_completed")?;
        Ok(row.0)
    }

    async fn delete_cross_dag_dependency(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM cross_dag_dependencies WHERE id = $1")
            .bind(id).execute(&self.pool).await.context("delete_cross_dag_dependency")?;
        Ok(())
    }
}
