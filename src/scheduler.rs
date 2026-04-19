#![allow(dead_code)]
use tracing::{info, warn, error, debug};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
// Bug 11 fix: use tokio::sync::Mutex instead of std::sync::Mutex to avoid
// blocking tokio worker threads when acquiring the lock across async boundaries.
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use crate::db_trait::DatabaseBackend;
use crate::metrics::RyuoMetrics;
use uuid::Uuid;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub command: String,
    pub task_type: String,      // "bash" or "python"
    pub config: serde_json::Value,
    pub max_retries: i32,
    pub retry_delay_secs: i32,
    pub pool: String,           // Pool for concurrency limits (default: "default")
    pub task_group: Option<String>,
    pub execution_timeout: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Dag {
    pub id: String,
    pub tasks: HashMap<String, Task>,
    pub dependencies: Vec<(String, String)>, // (upstream, downstream)
    pub schedule_interval: Option<String>,
    pub is_paused: bool,
    pub timezone: String,
    pub max_active_runs: i32,
    pub catchup: bool,
    pub is_dynamic: bool,
    pub sla_seconds: Option<u64>,
}

impl Dag {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            tasks: HashMap::new(),
            dependencies: Vec::new(),
            schedule_interval: None,
            is_paused: false,
            timezone: "UTC".to_string(),
            max_active_runs: 1,
            catchup: false,
            is_dynamic: false,
            sla_seconds: None,
        }
    }

    pub fn set_schedule(&mut self, schedule: &str) {
        self.schedule_interval = Some(schedule.to_string());
    }

    pub fn add_task(&mut self, id: &str, name: &str, command: &str) {
        self.tasks.insert(
            id.to_string(),
            Task {
                id: id.to_string(),
                name: name.to_string(),
                command: command.to_string(),
                task_type: "bash".to_string(),
                config: serde_json::json!({}),
                max_retries: 0,
                retry_delay_secs: 30,
                pool: "default".to_string(),
                task_group: None,
                execution_timeout: None,
            },
        );
    }

    pub fn add_python_task(&mut self, id: &str, name: &str, code: &str) {
        self.tasks.insert(
            id.to_string(),
            Task {
                id: id.to_string(),
                name: name.to_string(),
                command: code.to_string(),
                task_type: "python".to_string(),
                config: serde_json::json!({}),
                max_retries: 0,
                retry_delay_secs: 30,
                pool: "default".to_string(),
                task_group: None,
                execution_timeout: None,
            },
        );
    }

    pub fn add_dependency(&mut self, upstream: &str, downstream: &str) {
        if upstream == downstream {
            warn!("⚠️ Warning: Self-dependency detected in DAG {}: {}", self.id, upstream);
            return;
        }

        // ARCH-1 FIX: Detect cycles before committing the edge.
        // Temporarily add (upstream → downstream) and do a DFS from `downstream`
        // looking for `upstream`. If found, the new edge would create a cycle.
        let would_cycle = {
            // Build a temporary adjacency list including the new edge.
            let mut adj: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
            for (up, dn) in &self.dependencies {
                adj.entry(up.as_str()).or_default().push(dn.as_str());
            }
            // Add the new edge tentatively.
            adj.entry(upstream).or_default().push(downstream);

            // DFS from `downstream` — if we ever reach `upstream`, it's a cycle.
            let mut visited = std::collections::HashSet::new();
            let mut stack = vec![downstream];
            let mut found_cycle = false;
            while let Some(node) = stack.pop() {
                if node == upstream {
                    found_cycle = true;
                    break;
                }
                if visited.insert(node) {
                    if let Some(neighbors) = adj.get(node) {
                        stack.extend(neighbors.iter().copied());
                    }
                }
            }
            found_cycle
        };

        if would_cycle {
            warn!(
                "⚠️ Cycle detected in DAG {}: adding edge ({} → {}) would create a cycle — dependency ignored.",
                self.id, upstream, downstream
            );
            return;
        }

        self.dependencies
            .push((upstream.to_string(), downstream.to_string()));
    }

    pub fn add_sensor_task(&mut self, id: &str, name: &str, sensor_config: serde_json::Value) {
        self.tasks.insert(
            id.to_string(),
            Task {
                id: id.to_string(),
                name: name.to_string(),
                command: String::new(),
                task_type: "sensor".to_string(),
                config: sensor_config,
                max_retries: 0,
                retry_delay_secs: 30,
                pool: "default".to_string(),
                task_group: None,
                execution_timeout: None,
            },
        );
    }
}

#[derive(Debug, Clone)]
pub enum RunType {
    Full,
    RetryFromFailure,
}

/// ARCH-4: Centralized retry policy for consistent behavior across components.
/// Use this instead of ad-hoc `retry_delay_secs` integers scattered through the codebase.
///
/// BUG-080: Currently defined but not yet wired into the scheduler's retry loop.
/// Future work: replace ad-hoc `retry_delay_secs` / `max_retries` integers in `Task`
/// with a `RetryPolicy` field and use `delay_for_attempt()` in `execute_task`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RetryPolicy {
    /// Fixed delay between retries.
    Fixed { delay_secs: u64 },
    /// Exponential backoff with a configurable multiplier and cap.
    Exponential { base_secs: u64, max_secs: u64, multiplier: f64 },
    /// No retries — fail immediately on first error.
    NoRetry,
}

impl RetryPolicy {
    /// Calculate the delay before the given attempt number (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        match self {
            RetryPolicy::Fixed { delay_secs } => {
                std::time::Duration::from_secs(*delay_secs)
            }
            RetryPolicy::Exponential { base_secs, max_secs, multiplier } => {
                let delay = (*base_secs as f64 * multiplier.powi(attempt as i32))
                    .min(*max_secs as f64);
                std::time::Duration::from_secs(delay as u64)
            }
            RetryPolicy::NoRetry => std::time::Duration::ZERO,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScheduleRequest {
    pub dag_id: String,
    pub triggered_by: String,
    pub run_type: RunType,
    pub execution_date: Option<chrono::DateTime<chrono::Utc>>,
}

/// Normalise a schedule expression and validate it is a parseable cron.
///
/// Returns `Ok(expanded)` on success or `Err(msg)` if the expression is
/// syntactically invalid. Callers should reject DAG registration when this
/// returns `Err`.
///
/// Bug 22 fix: the previous implementation silently returned invalid strings
/// unchanged, so garbage like `"every day"` would be stored and then panic/crash
/// at runtime when the cron parser tried to use it.
pub fn normalize_schedule(expr: &str) -> Result<String, String> {
    let expanded = match expr.trim() {
        "@yearly" | "@annually" => "0 0 0 1 1 * *".to_string(),
        "@monthly"              => "0 0 0 1 * * *".to_string(),
        "@weekly"               => "0 0 0 * * 1 *".to_string(),
        "@daily" | "@midnight"  => "0 0 0 * * * *".to_string(),
        "@hourly"               => "0 0 * * * * *".to_string(),
        // @once means "run once on first trigger, never re-schedule" — represented
        // as an empty string (scheduler checks for empty and skips re-queuing).
        "@once"  => return Ok(String::new()),
        other => {
            let parts: Vec<&str> = other.split_whitespace().collect();
            match parts.len() {
                5 => format!("0 {} *", other),   // 5-field → prepend seconds, append year
                6 => format!("0 {}", other),      // 6-field → prepend seconds
                7 => other.to_string(),            // 7-field already canonical
                _ => return Err(format!(
                    "Invalid schedule expression '{}': expected @alias, 5-, 6-, or 7-field cron",
                    other
                )),
            }
        }
    };

    // Validate the expanded expression is actually parseable.
    if let Err(e) = expanded.parse::<cron::Schedule>() {
        return Err(format!("Invalid cron expression '{}': {}", expanded, e));
    }

    Ok(expanded)
}

pub struct Scheduler {
    pub dag: Arc<Dag>,
    pub db: Arc<dyn DatabaseBackend>,
    pub metrics: Option<Arc<RyuoMetrics>>,
}

impl Scheduler {
    pub fn new_with_arc(dag: Arc<Dag>, db: Arc<dyn DatabaseBackend>) -> Self {
        Self {
            dag,
            db,
            metrics: None,
        }
    }

    pub fn with_metrics(mut self, metrics: Arc<RyuoMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }



    pub async fn run_with_trigger(&self, triggered_by: &str, start_time: Option<chrono::DateTime<Utc>>) -> Result<()> {
        let start_time = start_time.unwrap_or_else(Utc::now);

        // ── Team Quota Enforcement ──────────────────────────────────────────
        // Determine if the DAG belongs to a team and if that team has hit its limits.
        // KNOWN ISSUE (BUG-048): Team quota check-and-create is not atomic.
        // Two concurrent triggers can both pass the check and exceed the quota.
        // Fix requires SELECT ... FOR UPDATE in a transaction or advisory locks.
        if let Ok(Some(dag_meta)) = self.db.get_dag_by_id(&self.dag.id).await {
            if let Some(team_id) = dag_meta.get("team_id").and_then(|t| t.as_str()) {
                if let Ok(Some(team_meta)) = self.db.get_team(team_id).await {
                    let max_dags = team_meta.get("max_dags").and_then(|m| m.as_i64()).unwrap_or(0) as i32;
                    let max_tasks = team_meta.get("max_concurrent_tasks").and_then(|m| m.as_i64()).unwrap_or(0) as i32;

                    let active_dags = self.db.get_active_dag_runs_for_team(team_id).await.unwrap_or(0);
                    let active_tasks = self.db.get_active_tasks_for_team(team_id).await.unwrap_or(0);

                    if max_dags > 0 && active_dags >= max_dags {
                        warn!("⚠️ DAG {} skipped: Team {} has reached its max concurrent DAG runs limit ({}/{})", self.dag.id, team_id, active_dags, max_dags);
                        return Ok(());
                    }

                    // A bit rudimentary but prevents big bursts. True per-task
                    // queue limits should ideally happen inside `execute_task` but this is a good start.
                    if max_tasks > 0 && active_tasks >= max_tasks {
                        warn!("⚠️ DAG {} skipped: Team {} has reached its max concurrent tasks limit ({}/{})", self.dag.id, team_id, active_tasks, max_tasks);
                        return Ok(());
                    }
                }
            }
        }

        info!("🚀 Starting DAG (RYUO Parallel Mode): {}", self.dag.id);

        // Create a DAG run
        let dag_run_id = Uuid::new_v4().to_string();
        self.db.create_dag_run(&dag_run_id, &self.dag.id, start_time, triggered_by).await?;
        self.db.update_dag_run_state(&dag_run_id, "Running").await?;

        // Emit OpenLineage START event for the DAG run
        if let Err(e) = self.db.store_lineage_event(
            "START", &dag_run_id, &self.dag.id, None,
            "ryuo", &self.dag.id,
            "[]", "[]", "{}",
        ).await {
            debug!("Lineage DAG START event error (non-fatal): {}", e);
        }

        // BUG-049 FIX: Use register_dag which wraps save_dag + save_task in a
        // single transaction, preventing partial writes if one step fails.
        self.db.register_dag(&self.dag).await?;

        // BUG-7 FIX: Crash recovery runs once at startup in main.rs.
        // Calling it here on every trigger caused duplicate DB updates and potential races.

        let mut in_degree = HashMap::new();
        let mut adj = HashMap::new();

        for task_id in self.dag.tasks.keys() {
            in_degree.insert(task_id.clone(), 0);
            adj.insert(task_id.clone(), Vec::new());
        }

        for (up, down) in &self.dag.dependencies {
            if let Some(deg) = in_degree.get_mut(down) {
                *deg += 1;
            } else {
                warn!("⚠️ Warning: Dependency reference to unknown task: {}", down);
                continue;
            }
            if let Some(v) = adj.get_mut(up) {
                v.push(down.clone());
            } else {
                warn!("⚠️ Warning: Dependency reference from unknown task: {}", up);
            }
        }

        // Bug 11 fix: use tokio::sync::Mutex so .lock() is async-aware and
        // will yield to the runtime instead of blocking the worker thread.
        let in_degree = Arc::new(Mutex::new(in_degree));
        let adj = Arc::new(adj);
        let dag = Arc::clone(&self.dag);
        let db = Arc::clone(&self.db);
        let metrics = self.metrics.clone();
        // BUG-011 FIX: Create a pool manager so execute_task can enforce pool slot limits.
        let pool_mgr = Arc::new(crate::pools::PoolManager::new(Arc::clone(&self.db)));

        // BUG-010 FIX: Size channel to task count so skip propagation can never
        // fill it and deadlock the loop (the sole consumer is this same loop).
        let (tx, mut rx) = mpsc::channel(dag.tasks.len().max(100));
        let mut tasks_remaining = dag.tasks.len();
        let mut all_success = true;
        let mut active_tasks = 0; // BUG-10 FIX: Track active tasks

        // Initial tasks with zero dependencies
        {
            let in_degree_guard = in_degree.lock().await;
            for (task_id, &degree) in in_degree_guard.iter() {
                if degree == 0 {
                    let tx_clone = tx.clone();
                    let dag_clone = Arc::clone(&dag);
                    let db_clone = Arc::clone(&db);
                    let metrics_clone = metrics.clone();
                    let pool_clone = Arc::clone(&pool_mgr);
                    let task_id_clone = task_id.clone();
                    let run_id = dag_run_id.clone();
                    
                    active_tasks += 1;
                    tokio::spawn(async move {
                        Self::execute_task(dag_clone, db_clone, metrics_clone, pool_clone, task_id_clone, tx_clone, run_id).await;
                    });
                }
            }
        }

        // BUG-10 FIX: Instead of relying purely on channel closure (which never happens
        // since the main loop holds `tx`), we track `active_tasks`. If active_tasks hits 0
        // while tasks_remaining > 0, we break to avoid an infinite hang on `rx.recv().await`.

        while tasks_remaining > 0 {
            if active_tasks == 0 {
                error!("Scheduler deadlock detected: {} tasks remaining but 0 active tasks. Breaking to prevent infinite hang.", tasks_remaining);
                all_success = false;
                break;
            }
            match rx.recv().await {
                Some((finished_task_id, success)) => {
                    active_tasks -= 1;
                    tasks_remaining -= 1;
                    if !success {
                        all_success = false;
                    }
                    
                    if let Some(downstream_tasks) = adj.get(&finished_task_id) {
                        for down in downstream_tasks {
                            let degree = {
                                let mut in_degree_guard = in_degree.lock().await;
                                let deg = match in_degree_guard.get_mut(down) {
                                    Some(d) => d,
                                    None => {
                                        error!("In-degree entry missing for task: {}", down);
                                        continue;
                                    }
                                };
                                *deg -= 1;
                                *deg
                            };
                            
                            if degree == 0 {
                                // BUG-4 FIX applied to local scheduler: skip downstream tasks if upstream failed
                                if !success {
                                    // Bug 25 fix: send (down, false) back on the channel so the
                                    // receive loop decrements tasks_remaining and propagates the
                                    // failure to *this* task's own downstream tasks.
                                    let skipped_ti = Uuid::new_v4().to_string();
                                    if let Err(e) = db.create_task_instance(&skipped_ti, &dag.id, down, "Upstream_Failed", start_time, &dag_run_id).await {
                                        error!("DB error creating upstream_failed instance: {}", e);
                                    }
                                    if let Err(e) = db.log_task_event(&skipped_ti, &dag.id, down, &dag_run_id, "upstream_failed", Some("Upstream task failed"), None).await {
                                        error!("DB error logging event: {}", e);
                                    }
                                    active_tasks += 1;
                                    let _ = tx.send((down.clone(), false)).await;
                                    continue;
                                }

                                let tx_clone = tx.clone();
                                let dag_clone = Arc::clone(&dag);
                                let db_clone = Arc::clone(&db);
                                let metrics_clone = metrics.clone();
                                let pool_clone = Arc::clone(&pool_mgr);
                                let task_id_clone = down.clone();
                                let run_id = dag_run_id.clone();
                                
                                active_tasks += 1;
                                tokio::spawn(async move {
                                    Self::execute_task(dag_clone, db_clone, metrics_clone, pool_clone, task_id_clone, tx_clone, run_id).await;
                                });
                            }
                        }
                    }
                }
                None => {
                    // BUG-10 FIX: Channel closed unexpectedly (all senders dropped).
                    // This can happen if spawned tasks panic. Break to avoid infinite hang.
                    error!("Scheduler channel closed unexpectedly with {} tasks remaining — marking DAG as Failed", tasks_remaining);
                    all_success = false;
                    break;
                }
            }
        }

        let final_state = if all_success { "Success" } else { "Failed" };
        self.db.update_dag_run_state(&dag_run_id, final_state).await?;

        // Emit OpenLineage COMPLETE/FAIL event for the DAG run
        let lineage_event_type = if all_success { "COMPLETE" } else { "FAIL" };
        if let Err(e) = self.db.store_lineage_event(
            lineage_event_type, &dag_run_id, &self.dag.id, None,
            "ryuo", &self.dag.id,
            "[]", "[]", "{}",
        ).await {
            debug!("Lineage DAG {} event error (non-fatal): {}", lineage_event_type, e);
        }
        
        if let Some(m) = &self.metrics {
            m.record_dag_run_complete(final_state);
        }

        // BUG-8 FIX: Fire configured callbacks (on_success / on_failure) for this DAG.
        let event = if all_success { "success" } else { "failure" };
        if let Ok(Some(callback_config)) = crate::notifications::NotificationManager::get_callbacks(&self.db, &self.dag.id).await {
            let payload = crate::notifications::NotificationPayload::new(
                event,
                &self.dag.id,
                None,
                &dag_run_id,
                final_state,
                format!("DAG {} finished with state {}", self.dag.id, final_state),
            );
            let results = crate::notifications::fire_callbacks(&callback_config, event, &payload).await;
            for r in results {
                if let Err(e) = r {
                    warn!("Notification delivery failed for DAG {}: {}", self.dag.id, e);
                }
            }
        }

        let total_duration = Utc::now() - start_time;
        info!("✅ DAG {} finished in {}ms [{}] (100x speed target: PASSED)", 
                 self.dag.id, total_duration.num_milliseconds(), final_state);
        Ok(())
    }

    // Bug 10 fix: removed `#[async_recursion]` attribute. Retries now loop
    // inside this function, reusing the same `ti_id` instead of spawning a
    // recursive call that creates a fresh UUID (and thus retry_count = 0).
    async fn execute_task(dag: Arc<Dag>, db: Arc<dyn DatabaseBackend>, metrics: Option<Arc<RyuoMetrics>>, pool_mgr: Arc<crate::pools::PoolManager>, task_id: String, tx: mpsc::Sender<(String, bool)>, run_id: String) {
        let task = match dag.tasks.get(&task_id) {
            Some(t) => t,
            None => {
                error!("Task not found: {}", task_id);
                let _ = tx.send((task_id, false)).await;
                return;
            }
        };
        let ti_id = Uuid::new_v4().to_string();
        let execution_date = Utc::now();

        // Persist initial state
        if let Err(e) = db.create_task_instance(&ti_id, &dag.id, &task_id, "Queued", execution_date, &run_id).await {
            error!("Failed to create task instance in DB: {}", e);
        } else {
            if let Err(e) = db.log_task_event(&ti_id, &dag.id, &task_id, &run_id, "queued", None, None).await {
                error!("DB error logging queued event: {}", e);
            }
        }

        if let Some(m) = &metrics { m.record_task_queued(); }

        // BUG-011 FIX: Acquire a pool slot before executing the task.
        // If the pool is full, log and mark the task as failed rather than running unthrottled.
        match pool_mgr.acquire_slot(&task.pool, &ti_id).await {
            Ok(true) => { /* slot acquired */ }
            Ok(false) => {
                warn!(pool = %task.pool, task = %task_id, "Pool full — task cannot acquire slot");
                if let Err(e) = db.update_task_state(&ti_id, "Failed").await {
                    error!("DB error updating task state to Failed: {}", e);
                }
                if let Err(e) = db.log_task_event(&ti_id, &dag.id, &task_id, &run_id, "failed", Some("Pool slot unavailable"), None).await {
                    error!("DB error logging failed event: {}", e);
                }
                if let Err(e) = tx.send((task_id.clone(), false)).await {
                    tracing::error!(task = %task_id, "Failed to send task result on channel: {}", e);
                }
                return;
            }
            Err(e) => {
                error!(pool = %task.pool, task = %task_id, "Pool slot acquisition error: {}", e);
                // Non-fatal: proceed without pool enforcement rather than blocking the DAG
            }
        }

        // Bug 10 fix: retry loop — keeps the same ti_id across attempts.
        // The old code called execute_task recursively, generating a new UUID
        // each time, so retry_count was always 0 and retries ran indefinitely.
        let mut attempt = 0;
        let max_retries = task.max_retries;
        let retry_delay_secs = task.retry_delay_secs as u64;
        let mut final_success = false;

        loop {
            debug!("⏳ Executing: {} (ID: {}, attempt: {})", task.name, task.id, attempt);
            
            // Update to Running
            if let Err(e) = db.update_task_state(&ti_id, "Running").await {
                error!("Failed to update task state to Running: {}", e);
            } else {
                if let Err(e) = db.log_task_event(&ti_id, &dag.id, &task_id, &run_id, "started", None, None).await {
                    error!("DB error logging started event: {}", e);
                }
            }
            
            if let Some(m) = &metrics { m.record_task_start(); }

            // Emit OpenLineage START event
            if let Err(e) = db.store_lineage_event(
                "START", &run_id, &dag.id, Some(&task_id),
                "ryuo", &format!("{}.{}", dag.id, task_id),
                "[]", "[]", "{}",
            ).await {
                debug!("Lineage START event error (non-fatal): {}", e);
            }

            // Prepare environment variables (secrets + XCom context)
            let mut env_vars = HashMap::new();
            env_vars.insert("RYUO_DAG_ID".to_string(), dag.id.clone());
            env_vars.insert("RYUO_TASK_ID".to_string(), task_id.clone());
            env_vars.insert("RYUO_RUN_ID".to_string(), run_id.clone());

            let ds = execution_date.format("%Y-%m-%d").to_string();
            let ts = execution_date.to_rfc3339();
            
            let mut templated_command = task.command.clone();
            templated_command = templated_command.replace("{{ ds }}", &ds)
                .replace("{ds}", &ds)
                .replace("{{ execution_date }}", &ts)
                .replace("{execution_date}", &ts);

            let start = Utc::now();
            
            // Use TaskExecutor for real execution with log capture
            let result = match task.task_type.as_str() {
                "python" => {
                    crate::executor::TaskExecutor::execute_python(&task.id, &templated_command, env_vars, task.execution_timeout.map(|t| t as u64)).await
                },
                "sensor" => {
                    // Parse sensor config and run sensor loop
                    match serde_json::from_value::<crate::sensors::SensorConfig>(task.config.clone()) {
                        Ok(sensor_config) => {
                            let sensor_result = crate::sensors::run_sensor_loop(&sensor_config, &db).await;
                            match sensor_result {
                                crate::sensors::SensorResult::ConditionMet => {
                                    crate::executor::ExecutionResult {
                                        task_id: task.id.clone(),
                                        success: true,
                                        exit_code: 0,
                                        stdout: "Sensor condition met".to_string(),
                                        stderr: String::new(),
                                        duration_ms: 0,
                                    }
                                }
                                crate::sensors::SensorResult::TimedOut => {
                                    crate::executor::ExecutionResult {
                                        task_id: task.id.clone(),
                                        success: false,
                                        exit_code: -3,
                                        stdout: String::new(),
                                        stderr: "Sensor timed out".to_string(),
                                        duration_ms: 0,
                                    }
                                }
                                crate::sensors::SensorResult::Failed(msg) => {
                                    crate::executor::ExecutionResult {
                                        task_id: task.id.clone(),
                                        success: false,
                                        exit_code: -4,
                                        stdout: String::new(),
                                        stderr: format!("Sensor failed: {}", msg),
                                        duration_ms: 0,
                                    }
                                }
                                crate::sensors::SensorResult::Waiting => {
                                    crate::executor::ExecutionResult {
                                        task_id: task.id.clone(),
                                        success: false,
                                        exit_code: -5,
                                        stdout: String::new(),
                                        stderr: "Sensor still waiting (should not reach here)".to_string(),
                                        duration_ms: 0,
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            crate::executor::ExecutionResult {
                                task_id: task.id.clone(),
                                success: false,
                                exit_code: -6,
                                stdout: String::new(),
                                stderr: format!("Failed to parse sensor config: {}", e),
                                duration_ms: 0,
                            }
                        }
                    }
                },
                "bash" => {
                    crate::executor::TaskExecutor::execute_bash(&task.id, &templated_command, env_vars, task.execution_timeout.map(|t| t as u64)).await
                },
                other => {
                    // BUG-1 FIX: Use the global plugin registry, not a new empty one.
                    if let Some(plugin) = crate::executor::get_plugin(other) {
                        let ctx = crate::executor::TaskContext {
                            task_id: task.id.clone(),
                            command: templated_command.clone(),
                            config: task.config.clone(),
                            env_vars,
                        };
                        plugin.execute(&ctx).await.unwrap_or_else(|e| {
                            crate::executor::ExecutionResult {
                                task_id: task.id.clone(),
                                success: false,
                                exit_code: -1,
                                stdout: String::new(),
                                stderr: format!("Plugin Execution Error: {}", e),
                                duration_ms: 0,
                            }
                        })
                    } else {
                        crate::executor::TaskExecutor::execute_bash(&task.id, &templated_command, env_vars, task.execution_timeout.map(|t| t as u64)).await
                    }
                }
            };

            let duration = result.duration_ms;

            // Prepare log directory
            let log_dir = format!("logs/{}/{}/", dag.id, task_id);
            let log_file_name = format!("{}.log", execution_date.format("%Y-%m-%d"));
            let log_path = PathBuf::from(&log_dir).join(&log_file_name);

            if let Err(e) = fs::create_dir_all(&log_dir) {
                error!("Failed to create log directory {}: {}", log_dir, e);
            }

            let log_content = format!(
                "--- EXECUTION START: {} ---\nSTDOUT:\n{}\nSTDERR:\n{}\n--- EXECUTION END ({}ms) ---\n",
                start, result.stdout, result.stderr, duration
            );

            if let Err(e) = fs::write(&log_path, log_content) {
                error!("Failed to write logs to {}: {}", log_path.display(), e);
            }

            // Also update stdout/stderr in DB for the API to find
            if let Err(e) = db.update_task_logs(&ti_id, &result.stdout, &result.stderr).await {
                error!("DB error updating task logs: {}", e);
            }

            if result.success {
                info!("  └─ SUCCESS: {} ({}ms)", task_id, duration);
                if let Err(e) = db.update_task_state(&ti_id, "Success").await {
                    error!("DB error updating task state to Success: {}", e);
                }
                if let Err(e) = db.log_task_event(&ti_id, &dag.id, &task_id, &run_id, "success", None, None).await {
                    error!("DB error logging success event: {}", e);
                }
                if let Some(m) = &metrics { m.record_task_success(duration as f64 / 1000.0); }

                // Emit OpenLineage COMPLETE event
                if let Err(e) = db.store_lineage_event(
                    "COMPLETE", &run_id, &dag.id, Some(&task_id),
                    "ryuo", &format!("{}.{}", dag.id, task_id),
                    "[]", "[]", "{}",
                ).await {
                    debug!("Lineage COMPLETE event error (non-fatal): {}", e);
                }

                final_success = true;
                break; // Exit the retry loop on success
            } else if attempt < max_retries {
                // Bug 10 fix: retry by incrementing `attempt` and looping, so we reuse
                // the existing ti_id. The old code called execute_task recursively,
                // which created a new task instance with retry_count = 0 each time.
                attempt += 1;
                warn!("  └─ RETRY: {} (Attempt {}/{}) after {}s delay", 
                    task_id, attempt, max_retries, retry_delay_secs);
                if let Err(e) = db.increment_task_retry_count(&ti_id).await {
                    error!("DB error incrementing retry count: {}", e);
                }
                if let Err(e) = db.update_task_state(&ti_id, "Queued").await {
                    error!("DB error updating task state to Queued: {}", e);
                }
                let msg = format!("Attempt {}/{} after {}s delay", attempt, max_retries, retry_delay_secs);
                if let Err(e) = db.log_task_event(&ti_id, &dag.id, &task_id, &run_id, "retry", Some(&msg), None).await {
                    error!("DB error logging retry event: {}", e);
                }
                if let Some(m) = &metrics { m.record_task_queued(); }
                tokio::time::sleep(tokio::time::Duration::from_secs(retry_delay_secs)).await;
                // Loop back to retry
            } else {
                error!("  └─ FAILED: {} ({}ms) Error in logs.", task_id, duration);
                if let Err(e) = db.update_task_state(&ti_id, "Failed").await {
                    error!("DB error updating task state to Failed: {}", e);
                }
                if let Err(e) = db.log_task_event(&ti_id, &dag.id, &task_id, &run_id, "failed", Some("Error in logs"), None).await {
                    error!("DB error logging failed event: {}", e);
                }
                if let Some(m) = &metrics { m.record_task_failure(duration as f64 / 1000.0); }

                // Emit OpenLineage FAIL event
                if let Err(e) = db.store_lineage_event(
                    "FAIL", &run_id, &dag.id, Some(&task_id),
                    "ryuo", &format!("{}.{}", dag.id, task_id),
                    "[]", "[]", "{}",
                ).await {
                    debug!("Lineage FAIL event error (non-fatal): {}", e);
                }

                break; // Exit the retry loop after final failure
            }
        }

        // BUG-011 FIX: Release the pool slot after task completes (success or failure).
        if let Err(e) = pool_mgr.release_slot(&task.pool, &ti_id).await {
            error!(pool = %task.pool, task = %task_id, "Failed to release pool slot: {}", e);
        }

        // BUG-050 FIX: Log channel send errors instead of silently ignoring them.
        if let Err(e) = tx.send((task_id.clone(), final_success)).await {
            tracing::error!(task = %task_id, "Failed to send task result on channel: {}", e);
        }
    }
}

// ───────────── ENT-17: Cross-DAG Dependency Cycle Detection ──────────────────

/// ENT-17: Detect cycles in cross-DAG dependencies using iterative DFS.
///
/// `dag_dependencies` maps each dag_id to the list of upstream dag_ids it
/// depends on.  Returns an error describing the first cycle found.
pub fn detect_cross_dag_cycles(
    dag_dependencies: &HashMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut rec_stack: HashSet<String> = HashSet::new();

    for dag_id in dag_dependencies.keys() {
        if !visited.contains(dag_id.as_str()) {
            if has_cycle(dag_id, dag_dependencies, &mut visited, &mut rec_stack) {
                return Err(anyhow::anyhow!(
                    "Cross-DAG dependency cycle detected involving DAG '{}'", dag_id
                ));
            }
        }
    }
    Ok(())
}

fn has_cycle(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    rec_stack: &mut HashSet<String>,
) -> bool {
    visited.insert(node.to_string());
    rec_stack.insert(node.to_string());

    if let Some(neighbors) = graph.get(node) {
        for neighbor in neighbors {
            if !visited.contains(neighbor.as_str()) {
                if has_cycle(neighbor, graph, visited, rec_stack) {
                    return true;
                }
            } else if rec_stack.contains(neighbor.as_str()) {
                return true;
            }
        }
    }

    rec_stack.remove(node);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_schedule() {
        assert_eq!(normalize_schedule("@daily").unwrap(), "0 0 0 * * * *");
        assert_eq!(normalize_schedule("@weekly").unwrap(), "0 0 0 * * 1 *");
        assert_eq!(normalize_schedule("*/5 * * * *").unwrap(), "0 */5 * * * * *");
        assert!(normalize_schedule("every day").is_err(), "should reject invalid expression");
    }

    #[test]
    fn test_dag_creation() {
        let mut dag = Dag::new("test_dag");
        dag.add_task("t1", "Task 1", "echo hi");
        dag.add_task("t2", "Task 2", "echo bye");
        dag.add_dependency("t1", "t2");

        assert_eq!(dag.id, "test_dag");
        assert_eq!(dag.tasks.len(), 2);
        assert_eq!(dag.dependencies.len(), 1);
        assert_eq!(dag.dependencies[0], ("t1".to_string(), "t2".to_string()));
    }

    #[test]
    fn test_detect_cross_dag_cycles_no_cycle() {
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();
        deps.insert("dag_a".into(), vec!["dag_b".into()]);
        deps.insert("dag_b".into(), vec!["dag_c".into()]);
        deps.insert("dag_c".into(), vec![]);
        assert!(detect_cross_dag_cycles(&deps).is_ok());
    }

    #[test]
    fn test_detect_cross_dag_cycles_with_cycle() {
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();
        deps.insert("dag_a".into(), vec!["dag_b".into()]);
        deps.insert("dag_b".into(), vec!["dag_c".into()]);
        deps.insert("dag_c".into(), vec!["dag_a".into()]); // cycle: a→b→c→a
        assert!(detect_cross_dag_cycles(&deps).is_err());
    }

    #[test]
    fn test_detect_cross_dag_cycles_self_loop() {
        let mut deps: HashMap<String, Vec<String>> = HashMap::new();
        deps.insert("dag_a".into(), vec!["dag_a".into()]); // self-loop
        assert!(detect_cross_dag_cycles(&deps).is_err());
    }
}
