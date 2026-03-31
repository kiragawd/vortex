#![allow(dead_code)]
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

    /// Return a paginated list of all DAGs as JSON objects, along with the total count.
    async fn get_all_dags(&self, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)>;

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
    ///                max_active_runs, catchup, team_id)
    async fn get_scheduled_dags(
        &self,
    ) -> Result<Vec<(String, String, Option<DateTime<Utc>>, bool, String, i32, bool, Option<String>)>>;

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

    /// Return a paginated list of task instances for a specific DAG, and the total count.
    async fn get_task_instances(&self, dag_id: &str, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)>;

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

    // ── Task Events ──────────────────────────────────────────────────────────

    /// Log a state transition or significant event for a task instance.
    async fn log_task_event(
        &self,
        ti_id: &str,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        event: &str,
        message: Option<&str>,
        worker_id: Option<&str>,
    ) -> Result<()>;

    /// Retrieve the event log for a specific task instance.
    async fn get_task_events(&self, ti_id: &str) -> Result<Vec<serde_json::Value>>;

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

    /// Return the most-recent DAG runs for a DAG, newest first.
    async fn get_dag_runs(&self, dag_id: &str, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)>;

    /// Return recent DAG runs across ALL DAGs, newest first.
    async fn get_all_runs(&self, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)>;

    /// Set the SLA breached flag for a DAG run.
    async fn mark_sla_missed(&self, run_id: &str) -> Result<()>;

    /// Return all DAG runs currently in 'Running' state.
    /// Each entry: (run_id, dag_id, start_time)
    async fn get_running_dag_runs(&self) -> Result<Vec<(String, String, DateTime<Utc>)>>;

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
    /// Tuple: (ti_id, dag_id, task_id, command, run_id, task_type, config, max_retries, retry_delay_secs)
    async fn get_interrupted_tasks_by_worker(
        &self,
        worker_id: &str,
    ) -> Result<Vec<(String, String, String, String, String, String, String, i32, i32)>>;

    /// Clear the worker_id field from Queued tasks previously owned by a worker.
    async fn clear_worker_id_from_queued_tasks(&self, worker_id: &str) -> Result<()>;

    /// Return full task execution details needed to re-run a task instance, including extra swarm fields.
    ///
    /// Tuple: (dag_id, task_id, command, run_id, task_type, config,
    ///         max_retries, retry_delay_secs)
    async fn get_task_instance_details_full(
        &self,
        ti_id: &str,
    ) -> Result<Option<(String, String, String, String, String, String, i32, i32)>>;

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
    async fn xcom_pull_all(&self, dag_id: &str, run_id: &str, limit: i64, offset: i64) -> Result<(Vec<serde_json::Value>, i64)>;

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

    // ── High Availability (HA) Advisory Locks ─────────────────────────────────

    /// Try to acquire the global advisory lock for the leader controller.
    /// Returns true if lock was successfully acquired, false if it is held by another process.
    async fn try_acquire_leader_lock(&self) -> Result<bool>;

    /// Release the global advisory lock for the leader controller.
    async fn release_leader_lock(&self) -> Result<()>;

    async fn acquire_pool_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<bool>;
    async fn release_pool_slot(&self, pool_name: &str, task_instance_id: &str) -> Result<()>;

    // ── Health ────────────────────────────────────────────────────────────────

    /// Improvement 42: Lightweight connectivity check — returns true if the
    /// database is reachable.
    async fn ping(&self) -> bool;

    // ── Auth Sessions (IAM) ─────────────────────────────────────────

    /// Create a user session (for SSO flows).
    async fn create_session(&self, session: &crate::auth::UserSession) -> Result<()>;

    /// Retrieve a session by session ID.
    async fn get_session(&self, session_id: &str) -> Result<Option<crate::auth::UserSession>>;

    /// Delete a session (logout).
    async fn delete_session(&self, session_id: &str) -> Result<()>;

    /// Delete all expired sessions. Returns the number of deleted sessions.
    async fn cleanup_expired_sessions(&self) -> Result<u64>;

    /// List all configured auth providers.
    async fn get_auth_providers(&self) -> Result<Vec<serde_json::Value>>;

    /// Get a specific auth provider config.
    async fn get_auth_provider(&self, provider_id: &str) -> Result<Option<serde_json::Value>>;

    /// Create or update an auth provider.
    async fn upsert_auth_provider(
        &self,
        id: &str,
        provider_type: &str,
        name: &str,
        config: &str,
        enabled: bool,
        priority: i32,
    ) -> Result<()>;

    /// Delete an auth provider.
    async fn delete_auth_provider(&self, provider_id: &str) -> Result<()>;

    /// Update user's last login timestamp.
    async fn update_user_last_login(&self, username: &str) -> Result<()>;

    // ── Lineage (Observability) ─────────────────────────────────────

    /// Store an OpenLineage event.
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
    ) -> Result<()>;

    /// Get lineage events for a DAG (optionally filtered by run_id).
    async fn get_lineage_events(
        &self,
        dag_id: &str,
        run_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>>;

    /// Get lineage datasets.
    async fn get_lineage_datasets(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>>;

    /// Get incident provider configurations.
    async fn get_incident_configs(&self, team_id: Option<&str>) -> Result<Vec<serde_json::Value>>;

    /// Create or update an incident provider configuration.
    async fn upsert_incident_config(
        &self,
        id: &str,
        team_id: Option<&str>,
        provider: &str,
        name: &str,
        config: &str,
        enabled: bool,
    ) -> Result<()>;

    /// Delete an incident provider configuration.
    async fn delete_incident_config(&self, id: &str) -> Result<()>;

    // ── Compliance & Governance ──────────────────────────────────

    /// Insert an audit log entry.
    async fn insert_audit_log(&self, entry: &crate::compliance::AuditEntry) -> Result<()>;

    /// Query audit log with filters.
    async fn get_audit_log(
        &self,
        event_type: Option<&str>,
        actor: Option<&str>,
        resource_type: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<serde_json::Value>>;

    /// Find an approval gate matching a resource type and id (glob pattern match).
    async fn find_matching_approval_gate(&self, resource_type: &str, resource_id: &str) -> Result<Option<serde_json::Value>>;

    /// CRUD for approval gates.
    async fn get_approval_gates(&self) -> Result<Vec<serde_json::Value>>;
    async fn upsert_approval_gate(
        &self, id: &str, name: &str, resource_type: &str, resource_pattern: &str,
        required_approvers: i32, approver_roles: &[String], enabled: bool,
    ) -> Result<()>;
    async fn delete_approval_gate(&self, id: &str) -> Result<()>;

    /// Approval requests.
    async fn create_approval_request(
        &self, gate_id: &str, requester: &str, resource_type: &str, resource_id: &str,
        description: Option<&str>, diff: &serde_json::Value,
    ) -> Result<String>;
    async fn get_approval_requests(&self, status: Option<&str>, limit: i64) -> Result<Vec<serde_json::Value>>;
    async fn add_approval_vote(&self, request_id: &str, approver: &str, comment: Option<&str>) -> Result<String>;
    async fn reject_approval_request(&self, request_id: &str, rejector: &str, reason: Option<&str>) -> Result<()>;

    /// Retention policies.
    async fn get_retention_policies(&self, enabled_only: bool) -> Result<Vec<serde_json::Value>>;
    async fn upsert_retention_policy(
        &self, id: &str, name: &str, target_table: &str, retention_days: i32,
        batch_size: i32, enabled: bool,
    ) -> Result<()>;
    async fn execute_retention_delete(&self, table: &str, retention_days: i64, batch_size: i64) -> Result<i64>;
    async fn update_retention_last_run(&self, id: &str) -> Result<()>;

    /// Compliance controls.
    async fn get_compliance_controls(&self, framework: Option<&str>) -> Result<Vec<serde_json::Value>>;
    async fn upsert_compliance_control(
        &self, framework: &str, control_id: &str, description: &str,
        status: &str, evidence: &serde_json::Value, assessor: &str,
    ) -> Result<()>;

    // ── Fine-Grained RBAC & Token Scoping ───────────────────────

    /// Check if a user has a permission (via their roles).
    async fn check_user_permission(&self, user_id: &str, permission: &str, team_id: Option<&str>) -> Result<bool>;

    /// Get all effective permissions for a user.
    async fn get_user_effective_permissions(&self, user_id: &str, team_id: Option<&str>) -> Result<Vec<String>>;

    /// RBAC role CRUD.
    async fn get_rbac_roles(&self) -> Result<Vec<serde_json::Value>>;
    async fn get_rbac_role_permissions(&self, role_id: &str) -> Result<Vec<serde_json::Value>>;
    async fn assign_user_role(&self, user_id: &str, role_id: &str, team_id: Option<&str>, granted_by: &str) -> Result<()>;
    async fn revoke_user_role(&self, user_id: &str, role_id: &str, team_id: Option<&str>) -> Result<()>;
    async fn get_user_roles(&self, user_id: &str) -> Result<Vec<serde_json::Value>>;

    /// Scoped API tokens.
    async fn create_api_token(&self, name: &str, token_hash: &str, user_id: &str, scopes: &[String], team_id: Option<&str>, expires_at: Option<&str>) -> Result<String>;
    async fn get_api_tokens(&self, user_id: &str) -> Result<Vec<serde_json::Value>>;
    async fn revoke_api_token(&self, token_id: &str) -> Result<()>;
    async fn find_api_token_by_hash(&self, token_prefix: &str) -> Result<Option<serde_json::Value>>;
    async fn update_token_last_used(&self, token_id: &str) -> Result<()>;

    /// IP allowlist.
    async fn get_ip_allowlist(&self) -> Result<Vec<serde_json::Value>>;
    async fn upsert_ip_allowlist_rule(&self, id: &str, cidr: &str, description: &str, enabled: bool) -> Result<()>;
    async fn delete_ip_allowlist_rule(&self, id: &str) -> Result<()>;

    // ── Advanced Scheduling & Data-Aware Orchestration ──────────

    /// Dataset CRUD.
    async fn upsert_dataset(&self, id: &str, uri: &str, name: &str, description: Option<&str>, producer_dag_id: Option<&str>, metadata: &serde_json::Value) -> Result<()>;
    async fn get_datasets(&self, limit: i64, offset: i64) -> Result<Vec<serde_json::Value>>;

    /// Dataset events.
    async fn insert_dataset_event(&self, event: &crate::advanced_scheduler::DatasetEvent) -> Result<()>;
    async fn get_dataset_events(&self, dataset_id: &str, limit: i64) -> Result<Vec<serde_json::Value>>;

    /// Dataset triggers.
    async fn upsert_dataset_trigger(&self, id: &str, dag_id: &str, dataset_ids: &[String], condition: &str, min_interval: Option<i32>, enabled: bool) -> Result<()>;
    async fn get_dataset_triggers_for_dataset(&self, dataset_id: &str) -> Result<Vec<serde_json::Value>>;
    async fn check_all_datasets_updated(&self, dataset_ids: &[String], trigger_id: &str) -> Result<bool>;

    /// Cross-DAG dependencies.
    async fn upsert_cross_dag_dependency(&self, id: &str, downstream: &str, upstream: &str, upstream_task: Option<&str>, condition: &str) -> Result<()>;
    async fn get_cross_dag_dependencies(&self, dag_id: &str) -> Result<Vec<serde_json::Value>>;
    async fn check_upstream_completed(&self, upstream_dag: &str, upstream_task: Option<&str>, condition: &str) -> Result<bool>;
    async fn delete_cross_dag_dependency(&self, id: &str) -> Result<()>;
}
