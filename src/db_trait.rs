// db_trait.rs — Database abstraction trait for VORTEX
//
// This trait defines the full interface that both the SQLite and PostgreSQL
// backends must implement. Add new methods here as VORTEX grows.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Unified async database interface for VORTEX.
///
/// All database backends (SQLite via spawn_blocking, PostgreSQL via sqlx)
/// implement this trait so the rest of the application is backend-agnostic.
#[async_trait]
pub trait DatabaseBackend: Send + Sync {
    // ── DAG operations ────────────────────────────────────────────────────────

    /// Insert or update a DAG record (upsert).
    async fn save_dag(&self, dag_id: &str, schedule_interval: Option<&str>) -> Result<()>;

    /// Register a full DAG (upsert DAG + sync tasks).
    async fn register_dag(&self, dag: &crate::scheduler::Dag) -> Result<()>;

    /// Return all DAGs as JSON objects.
    async fn get_all_dags(&self) -> Result<Vec<serde_json::Value>>;

    /// Return a single DAG by its ID.
    async fn get_dag_by_id(&self, dag_id: &str) -> Result<Option<serde_json::Value>>;

    /// Update schedule/timezone/concurrency config for a DAG.
    async fn update_dag_config(
        &self,
        dag_id: &str,
        schedule_interval: Option<&str>,
        timezone: &str,
        max_active_runs: i32,
        catchup: bool,
        is_dynamic: bool,
    ) -> Result<()>;

    /// Persist the last-run timestamp for a DAG.
    async fn update_dag_last_run(&self, dag_id: &str, last_run: DateTime<Utc>) -> Result<()>;

    /// Persist (or clear) the next scheduled run timestamp for a DAG.
    async fn update_dag_next_run(
        &self,
        dag_id: &str,
        next_run: Option<DateTime<Utc>>,
    ) -> Result<()>;

    /// Return all DAGs that have a non-empty schedule_interval.
    ///
    /// Tuple fields: (dag_id, schedule_interval, last_run, is_paused, timezone,
    ///                max_active_runs, catchup)
    async fn get_scheduled_dags(
        &self,
    ) -> Result<Vec<(String, String, Option<DateTime<Utc>>, bool, String, i32, bool)>>;

    /// Pause a DAG (is_paused = true).
    async fn pause_dag(&self, dag_id: &str) -> Result<()>;

    /// Unpause a DAG (is_paused = false).
    async fn unpause_dag(&self, dag_id: &str) -> Result<()>;

    /// Count DAG runs that are currently Queued or Running.
    async fn get_active_dag_run_count(&self, dag_id: &str) -> Result<i32>;

    /// Count DAG runs that are currently Queued or Running for a specific team.
    async fn get_active_dag_runs_for_team(&self, team_id: &str) -> Result<i32>;

    /// Count tasks that are currently Queued or Running for a specific team.
    async fn get_active_tasks_for_team(&self, team_id: &str) -> Result<i32>;

    // ── Task operations ───────────────────────────────────────────────────────

    /// Insert or replace a task definition for a DAG.
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
    ) -> Result<()>;

    /// Return all tasks belonging to a DAG as JSON objects.
    async fn get_dag_tasks(&self, dag_id: &str) -> Result<Vec<serde_json::Value>>;

    // ── Task instance operations ──────────────────────────────────────────────

    /// Create a new task instance record.
    async fn create_task_instance(
        &self,
        id: &str,
        dag_id: &str,
        task_id: &str,
        state: &str,
        execution_date: DateTime<Utc>,
        run_id: &str,
    ) -> Result<()>;

    /// Update the state of a task instance (also sets start/end timestamps).
    async fn update_task_state(&self, id: &str, state: &str) -> Result<()>;

    /// Return all task instances for a DAG as JSON objects.
    async fn get_task_instances(&self, dag_id: &str) -> Result<Vec<serde_json::Value>>;

    /// Return (dag_id, task_id, execution_date) for a single task instance.
    async fn get_task_instance(
        &self,
        ti_id: &str,
    ) -> Result<Option<(String, String, DateTime<Utc>)>>;

    /// Return all task instances currently in the 'Running' state (crash recovery).
    async fn get_interrupted_tasks(&self) -> Result<Vec<(String, String, String)>>;

    /// Append stdout/stderr logs to a task instance.
    async fn update_task_logs(&self, ti_id: &str, stdout: &str, stderr: &str) -> Result<()>;

    /// Persist an `ExecutionResult` to the task instance record.
    async fn store_task_result(
        &self,
        task_instance_id: &str,
        result: &crate::executor::ExecutionResult,
    ) -> Result<()>;

    /// Return (retry_count, state) for a task instance (used by retry logic).
    async fn get_task_instance_retry_info(&self, ti_id: &str) -> Result<(i32, String)>;

    /// Increment the retry counter for a task instance by 1.
    async fn increment_task_retry_count(&self, ti_id: &str) -> Result<()>;

    /// Return full task execution details needed to re-run a task instance.
    ///
    /// Tuple: (dag_id, task_id, command, run_id, task_type, config,
    ///         max_retries, retry_delay_secs)
    async fn get_task_instance_details(
        &self,
        ti_id: &str,
    ) -> Result<Option<(String, String, String, String, String, String, i32, i32)>>;

    /// Mark a task instance as Running and bind it to a worker.
    async fn assign_task_to_worker(&self, ti_id: &str, worker_id: &str) -> Result<()>;

    // ── DAG run operations ────────────────────────────────────────────────────

    /// Create a new DAG run record (state = 'Queued').
    async fn create_dag_run(
        &self,
        id: &str,
        dag_id: &str,
        execution_date: DateTime<Utc>,
        triggered_by: &str,
    ) -> Result<()>;

    /// Transition a DAG run to a new state (also sets start/end timestamps).
    async fn update_dag_run_state(&self, id: &str, state: &str) -> Result<()>;

    /// Return the most-recent 100 DAG runs for a DAG, newest first.
    async fn get_dag_runs(&self, dag_id: &str) -> Result<Vec<serde_json::Value>>;

    // ── User management ───────────────────────────────────────────────────────

    /// Create a new user with a bcrypt-hashed password.
    async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: &str,
        api_key: &str,
    ) -> Result<()>;

    /// Delete a user by username.
    async fn delete_user(&self, username: &str) -> Result<()>;

    /// Return all users (username, role, api_key) as JSON objects.
    async fn get_all_users(&self) -> Result<Vec<serde_json::Value>>;

    /// Verify credentials; return (api_key, role) on success.
    async fn validate_user(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<(String, String)>>;

    /// Look up a user by API key; return (username, role, team_id) on match.
    async fn get_user_by_api_key(&self, api_key: &str) -> Result<Option<(String, String, Option<String>)>>;

    // ── Secret management ─────────────────────────────────────────────────────

    /// Store (upsert) an encrypted secret value.
    async fn store_secret(&self, key: &str, encrypted_value: &str) -> Result<()>;

    /// Retrieve an encrypted secret value by key.
    async fn get_secret(&self, key: &str) -> Result<Option<String>>;

    /// Return all secret keys (not their values).
    async fn get_all_secrets(&self) -> Result<Vec<String>>;

    /// Delete a secret by key.
    async fn delete_secret(&self, key: &str) -> Result<()>;

    // ── Worker management ─────────────────────────────────────────────────────

    /// Register or refresh a worker (upsert).
    async fn upsert_worker(
        &self,
        id: &str,
        hostname: &str,
        capacity: i32,
        labels: &str,
    ) -> Result<()>;

    /// Update worker heartbeat timestamp and active task count.
    async fn update_worker_heartbeat(&self, id: &str, active_tasks: i32) -> Result<()>;

    /// Mark workers whose heartbeat is older than `timeout_seconds` as Offline.
    /// Returns the IDs of workers that were just marked offline.
    async fn mark_stale_workers_offline(&self, timeout_seconds: i64) -> Result<Vec<String>>;

    /// Transition Running task instances owned by a worker back to Queued.
    /// Returns the count of re-queued tasks.
    async fn requeue_worker_tasks(&self, worker_id: &str) -> Result<usize>;

    /// Return Queued task instances still attributed to a dead worker.
    ///
    /// Tuple: (ti_id, dag_id, task_id, command, run_id)
    async fn get_interrupted_tasks_by_worker(
        &self,
        worker_id: &str,
    ) -> Result<Vec<(String, String, String, String, String)>>;

    /// Clear the worker_id field from Queued tasks previously owned by a worker.
    async fn clear_worker_id_from_queued_tasks(&self, worker_id: &str) -> Result<()>;

    // ── DAG versioning ────────────────────────────────────────────────────────

    /// Store a new version snapshot for a DAG file.
    /// Returns the new version number.
    async fn store_dag_version(&self, dag_id: &str, file_path: &str) -> Result<i64>;

    /// Return all stored versions for a DAG, newest first.
    async fn get_dag_versions(&self, dag_id: &str) -> Result<Vec<serde_json::Value>>;

    /// Return the latest version record for a DAG, if any.
    async fn get_latest_version(&self, dag_id: &str) -> Result<Option<serde_json::Value>>;

    // ── XCom operations ───────────────────────────────────────────────────────

    async fn xcom_push(&self, dag_id: &str, task_id: &str, run_id: &str, key: &str, value: &str) -> Result<()>;
    async fn xcom_pull(&self, dag_id: &str, task_id: &str, run_id: &str, key: &str) -> Result<Option<String>>;
    async fn xcom_pull_all(&self, dag_id: &str, run_id: &str) -> Result<Vec<serde_json::Value>>;

    // ── Task Pool operations ──────────────────────────────────────────────────

    async fn get_all_pools(&self) -> Result<Vec<serde_json::Value>>;
    async fn get_pool(&self, name: &str) -> Result<Option<serde_json::Value>>;
    async fn create_pool(&self, name: &str, slots: i32, description: &str) -> Result<()>;
    async fn update_pool(&self, name: &str, slots: i32, description: &str) -> Result<()>;
    async fn delete_pool(&self, name: &str) -> Result<()>;

    // ── Callback / Webhook operations ─────────────────────────────────────────

    async fn get_callbacks(&self, dag_id: &str) -> Result<Option<serde_json::Value>>;
    async fn save_callbacks(&self, dag_id: &str, config_json: &str) -> Result<()>;
    async fn delete_callbacks(&self, dag_id: &str) -> Result<()>;

    // ── Audit Logging ─────────────────────────────────────────────────────────

    /// Record an audit event (actor performed action on target).
    async fn log_audit_event(
        &self,
        actor: &str,
        action: &str,
        target_type: &str,
        target_id: &str,
        metadata: &str,
    ) -> Result<()>;

    /// Return paginated audit log entries, optionally filtered by actor/action.
    async fn get_audit_logs(
        &self,
        limit: i64,
        offset: i64,
        actor: Option<&str>,
        action: Option<&str>,
    ) -> Result<Vec<serde_json::Value>>;

    // ── Analysis / Gantt ──────────────────────────────────────────────────────

    /// Return task instance timeline data for a DAG, grouped by task_id.
    async fn get_gantt_data(&self, dag_id: &str) -> Result<Vec<serde_json::Value>>;

    // ── Multi-Tenancy (Teams) ─────────────────────────────────────────────────

    async fn get_all_teams(&self) -> Result<Vec<serde_json::Value>>;
    
    async fn get_team(&self, team_id: &str) -> Result<Option<serde_json::Value>>;
    
    async fn create_team(
        &self,
        id: &str,
        name: &str,
        description: &str,
        max_concurrent_tasks: i32,
        max_dags: i32,
    ) -> Result<()>;
    
    async fn update_team(
        &self,
        id: &str,
        name: &Option<String>,
        description: &Option<String>,
        max_concurrent_tasks: Option<i32>,
        max_dags: Option<i32>,
    ) -> Result<()>;
    
    async fn delete_team(&self, id: &str) -> Result<()>;
    
    async fn assign_user_to_team(&self, username: &str, team_id: Option<&str>) -> Result<()>;

}
