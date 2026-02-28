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

// ─── Connection pool ─────────────────────────────────────────────────────────

pub struct PostgresDb {
    pool: PgPool,
}

impl PostgresDb {
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

        let db = Self { pool };
        db.init().await?;
        Ok(db)
    }

    /// Create all tables via migrations.
    async fn init(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .context("Failed to run PostgreSQL migrations")?;

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
        let admin_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE username = 'admin')",
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to check admin existence")?;

        if !admin_exists {
            let hashed = hash("admin", DEFAULT_COST).context("bcrypt hash failed")?;
            sqlx::query(
                "INSERT INTO users (username, password_hash, role, api_key)
                 VALUES ('admin', $1, 'Admin', 'vortex_admin_key')
                 ON CONFLICT (username) DO NOTHING",
            )
            .bind(&hashed)
            .execute(&self.pool)
            .await
            .context("Failed to seed admin user")?;
        }

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

    async fn register_dag(&self, dag: &crate::scheduler::Dag) -> Result<()> {
        self.save_dag(&dag.id, dag.schedule_interval.as_deref()).await?;
        self.update_dag_config(
            &dag.id,
            dag.schedule_interval.as_deref(),
            &dag.timezone,
            dag.max_active_runs,
            dag.catchup,
            dag.is_dynamic,
        )
        .await?;

        // Remove tasks that are no longer in the DAG definition
        let task_ids: Vec<String> = dag.tasks.keys().cloned().collect();
        if task_ids.is_empty() {
            sqlx::query("DELETE FROM tasks WHERE dag_id = $1")
                .bind(&dag.id)
                .execute(&self.pool)
                .await
                .context("register_dag: delete stale tasks")?;
        } else {
            // Build a NOT IN ($2, $3, ...) clause dynamically
            let placeholders: String = task_ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("${}", i + 2))
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
            q.execute(&self.pool)
                .await
                .context("register_dag: delete stale tasks")?;
        }

        for task in dag.tasks.values() {
            self.save_task(
                &dag.id,
                &task.id,
                &task.name,
                &task.command,
                &task.task_type,
                &task.config.to_string(),
                task.max_retries,
                task.retry_delay_secs,
                &task.pool,
                task.task_group.as_deref(),
                task.execution_timeout,
            )
            .await?;
        }
        Ok(())
    }

    async fn get_all_dags(&self) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT id, created_at, schedule_interval, last_run, is_paused, timezone,
                    max_active_runs, catchup, next_run, is_dynamic
             FROM dags",
        )
        .fetch_all(&self.pool)
        .await
        .context("get_all_dags")?;

        use sqlx::Row;
        let dags = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":                r.get::<String, _>("id"),
                    "created_at":        r.get::<DateTime<Utc>, _>("created_at"),
                    "schedule_interval": r.get::<Option<String>, _>("schedule_interval"),
                    "last_run":          r.get::<Option<DateTime<Utc>>, _>("last_run"),
                    "is_paused":         r.get::<bool, _>("is_paused"),
                    "timezone":          r.get::<String, _>("timezone"),
                    "max_active_runs":   r.get::<i32, _>("max_active_runs"),
                    "catchup":           r.get::<bool, _>("catchup"),
                    "is_dynamic":        r.get::<bool, _>("is_dynamic"),
                    "next_run":          r.get::<Option<DateTime<Utc>>, _>("next_run"),
                })
            })
            .collect();
        Ok(dags)
    }

    async fn get_dag_by_id(&self, dag_id: &str) -> Result<Option<serde_json::Value>> {
        let row = sqlx::query(
            "SELECT id, created_at, schedule_interval, last_run, is_paused, timezone,
                    max_active_runs, catchup, next_run, is_dynamic
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
                "timezone":          r.get::<String, _>("timezone"),
                "max_active_runs":   r.get::<i32, _>("max_active_runs"),
                "catchup":           r.get::<bool, _>("catchup"),
                "is_dynamic":        r.get::<bool, _>("is_dynamic"),
                "next_run":          r.get::<Option<DateTime<Utc>>, _>("next_run"),
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
    ) -> Result<Vec<(String, String, Option<DateTime<Utc>>, bool, String, i32, bool)>> {
        let rows = sqlx::query(
            "SELECT id, schedule_interval, last_run, is_paused, timezone, max_active_runs, catchup
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
                    r.get::<String, _>("schedule_interval"),
                    r.get::<Option<DateTime<Utc>>, _>("last_run"),
                    r.get::<bool, _>("is_paused"),
                    r.get::<Option<String>, _>("timezone")
                        .unwrap_or_else(|| "UTC".to_string()),
                    r.get::<i32, _>("max_active_runs"),
                    r.get::<bool, _>("catchup"),
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

    async fn get_task_instances(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT id, task_id, state, execution_date, start_time, end_time,
                    stdout, stderr, duration_ms, retry_count, run_id
             FROM task_instances WHERE dag_id = $1",
        )
        .bind(dag_id)
        .fetch_all(&self.pool)
        .await
        .context("get_task_instances")?;

        use sqlx::Row;
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
        Ok(instances)
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
    ) -> Result<Option<(String, String, String, String, String, String, i32, i32)>> {
        let row = sqlx::query(
            "SELECT ti.dag_id,
                    ti.task_id,
                    t.command,
                    dr.id        AS run_id,
                    t.task_type,
                    t.config,
                    t.max_retries,
                    t.retry_delay_secs
             FROM task_instances ti
             JOIN tasks    t  ON ti.task_id = t.id AND ti.dag_id = t.dag_id
             JOIN dag_runs dr ON ti.dag_id  = dr.dag_id
                              AND ti.execution_date = dr.execution_date
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

    async fn get_dag_runs(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT id, dag_id, state, execution_date, start_time, end_time, triggered_by
             FROM dag_runs
             WHERE dag_id = $1
             ORDER BY execution_date DESC
             LIMIT 100",
        )
        .bind(dag_id)
        .fetch_all(&self.pool)
        .await
        .context("get_dag_runs")?;

        use sqlx::Row;
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
                })
            })
            .collect();
        Ok(runs)
    }

    // ── User management ───────────────────────────────────────────────────────

    async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: &str,
        api_key: &str,
    ) -> Result<()> {
        if password.len() < 8 {
            return Err(anyhow::anyhow!("Password must be at least 8 characters"));
        }
        if password.chars().all(|c| c.is_lowercase())
            || password.chars().all(|c| c.is_uppercase())
        {
            return Err(anyhow::anyhow!(
                "Password must contain mixed case or numbers"
            ));
        }
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
    ) -> Result<Option<(String, String)>> {
        let row = sqlx::query(
            "SELECT password_hash, api_key, role FROM users WHERE username = $1",
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
            if verify(password, &stored_hash).unwrap_or(false) {
                return Ok(Some((api_key, role)));
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

    async fn store_secret(&self, key: &str, encrypted_value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO secrets (key, value, updated_at) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE
                SET value      = EXCLUDED.value,
                    updated_at = EXCLUDED.updated_at",
        )
        .bind(key)
        .bind(encrypted_value)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .context("store_secret")?;
        Ok(())
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>> {
        let row =
            sqlx::query_scalar("SELECT value FROM secrets WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .context("get_secret")?;
        Ok(row)
    }

    async fn get_all_secrets(&self) -> Result<Vec<String>> {
        let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM secrets")
            .fetch_all(&self.pool)
            .await
            .context("get_all_secrets")?;
        Ok(keys)
    }

    async fn delete_secret(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM secrets WHERE key = $1")
            .bind(key)
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
    ) -> Result<Vec<(String, String, String, String, String)>> {
        let rows = sqlx::query(
            "SELECT ti.id,
                    ti.dag_id,
                    ti.task_id,
                    t.command,
                    dr.id AS run_id
             FROM task_instances ti
             JOIN tasks    t  ON ti.task_id = t.id AND ti.dag_id = t.dag_id
             JOIN dag_runs dr ON ti.dag_id  = dr.dag_id
                              AND ti.execution_date = dr.execution_date
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
        // Compute the next version number atomically
        let next_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM dag_versions WHERE dag_id = $1",
        )
        .bind(dag_id)
        .fetch_one(&self.pool)
        .await
        .context("store_dag_version: compute version")?;

        let id = format!("{}-{}", dag_id, next_version);
        sqlx::query(
            "INSERT INTO dag_versions (id, dag_id, version, file_path, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id)
        .bind(dag_id)
        .bind(next_version)
        .bind(file_path)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .context("store_dag_version: insert")?;

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
                    "created_at": r.get::<String, _>("created_at"),
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
                "created_at": r.get::<String, _>("created_at"),
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

    async fn xcom_pull_all(&self, dag_id: &str, run_id: &str) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query("SELECT dag_id, task_id, run_id, key, value, timestamp FROM task_xcom WHERE dag_id=$1 AND run_id=$2")
            .bind(dag_id).bind(run_id).fetch_all(&self.pool).await.context("xcom_pull_all")?;
        use sqlx::Row;
        Ok(rows.iter().map(|r| serde_json::json!({
            "dag_id": r.get::<String, _>(0),
            "task_id": r.get::<String, _>(1),
            "run_id": r.get::<String, _>(2),
            "key": r.get::<String, _>(3),
            "value": r.get::<String, _>(4),
            "timestamp": r.get::<String, _>(5)
        })).collect())
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

    // ── Callback / Webhook operations ─────────────────────────────────────────

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
        let rows = match (actor, action) {
            (Some(a), Some(act)) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata
                     FROM audit_log WHERE actor=$1 AND action=$2
                     ORDER BY timestamp DESC LIMIT $3 OFFSET $4",
                )
                .bind(a).bind(act).bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
            (Some(a), None) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata
                     FROM audit_log WHERE actor=$1
                     ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                )
                .bind(a).bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
            (None, Some(act)) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata
                     FROM audit_log WHERE action=$1
                     ORDER BY timestamp DESC LIMIT $2 OFFSET $3",
                )
                .bind(act).bind(limit).bind(offset)
                .fetch_all(&self.pool).await.context("get_audit_logs")?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT id, timestamp, actor, action, target_type, target_id, metadata
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
        })).collect())
    }

    async fn get_gantt_data(&self, dag_id: &str) -> Result<Vec<serde_json::Value>> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT task_id, run_id, state, start_time, end_time, duration_ms
             FROM task_instances
             WHERE dag_id = $1 AND start_time IS NOT NULL
             ORDER BY task_id, start_time",
        )
        .bind(dag_id)
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
}
