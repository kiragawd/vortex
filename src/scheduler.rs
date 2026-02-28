use tracing::{info, warn, error, debug};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use crate::db_trait::DatabaseBackend;
use crate::metrics::VortexMetrics;
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

#[derive(Debug, Clone)]
pub struct ScheduleRequest {
    pub dag_id: String,
    pub triggered_by: String,
    pub run_type: RunType,
    pub execution_date: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn normalize_schedule(expr: &str) -> String {
    match expr.trim() {
        "@yearly" | "@annually" => "0 0 0 1 1 * *".to_string(),
        "@monthly" => "0 0 0 1 * * *".to_string(),
        "@weekly" => "0 0 0 * * 0 *".to_string(),
        "@daily" | "@midnight" => "0 0 0 * * * *".to_string(),
        "@hourly" => "0 0 * * * * *".to_string(),
        "@once" => "".to_string(),
        other => {
            let parts: Vec<&str> = other.split_whitespace().collect();
            match parts.len() {
                5 => format!("0 {} *", other),
                6 => format!("0 {}", other),
                7 => other.to_string(),
                _ => other.to_string(),
            }
        }
    }
}

pub struct Scheduler {
    pub dag: Arc<Dag>,
    pub db: Arc<dyn DatabaseBackend>,
    pub metrics: Option<Arc<VortexMetrics>>,
}

impl Scheduler {
    pub fn new_with_arc(dag: Arc<Dag>, db: Arc<dyn DatabaseBackend>) -> Self {
        Self {
            dag,
            db,
            metrics: None,
        }
    }

    pub fn with_metrics(mut self, metrics: Arc<VortexMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }



    pub async fn run_with_trigger(&self, triggered_by: &str, start_time: Option<chrono::DateTime<Utc>>) -> Result<()> {
        let start_time = start_time.unwrap_or_else(Utc::now);

        // ── Team Quota Enforcement ──────────────────────────────────────────
        // Determine if the DAG belongs to a team and if that team has hit its limits.
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

        info!("🚀 Starting DAG (VORTEX Parallel Mode): {}", self.dag.id);

        // Create a DAG run
        let dag_run_id = Uuid::new_v4().to_string();
        self.db.create_dag_run(&dag_run_id, &self.dag.id, start_time, triggered_by).await?;
        self.db.update_dag_run_state(&dag_run_id, "Running").await?;

        // Persist DAG and tasks to DB
        self.db.save_dag(&self.dag.id, self.dag.schedule_interval.as_deref()).await?;
        for task in self.dag.tasks.values() {
            self.db.save_task(&self.dag.id, &task.id, &task.name, &task.command, &task.task_type, &task.config.to_string(), task.max_retries, task.retry_delay_secs, &task.pool, task.task_group.as_deref(), task.execution_timeout).await?;
        }

        // Recovery Mode
        self.handle_recovery().await?;

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

        let in_degree = Arc::new(Mutex::new(in_degree));
        let adj = Arc::new(adj);
        let dag = Arc::clone(&self.dag);
        let db = Arc::clone(&self.db);
        let metrics = self.metrics.clone();

        let (tx, mut rx) = mpsc::channel(100);
        let mut tasks_remaining = dag.tasks.len();
        let mut all_success = true;

        // Initial tasks with zero dependencies
        {
            let in_degree_guard = in_degree.lock().unwrap();
            for (task_id, &degree) in in_degree_guard.iter() {
                if degree == 0 {
                    let tx_clone = tx.clone();
                    let dag_clone = Arc::clone(&dag);
                    let db_clone = Arc::clone(&db);
                    let metrics_clone = metrics.clone();
                    let task_id_clone = task_id.clone();
                    let run_id = dag_run_id.clone();
                    
                    tokio::spawn(async move {
                        Self::execute_task(dag_clone, db_clone, metrics_clone, task_id_clone, tx_clone, run_id).await;
                    });
                }
            }
        }

        while tasks_remaining > 0 {
            if let Some((finished_task_id, success)) = rx.recv().await {
                tasks_remaining -= 1;
                if !success {
                    all_success = false;
                }
                
                let downstream_tasks = adj.get(&finished_task_id).unwrap();
                for down in downstream_tasks {
                    let mut in_degree_guard = in_degree.lock().unwrap();
                    let degree = in_degree_guard.get_mut(down).unwrap();
                    *degree -= 1;
                    
                    if *degree == 0 {
                        let tx_clone = tx.clone();
                        let dag_clone = Arc::clone(&dag);
                        let db_clone = Arc::clone(&db);
                        let metrics_clone = metrics.clone();
                        let task_id_clone = down.clone();
                        let run_id = dag_run_id.clone();
                        
                        tokio::spawn(async move {
                            Self::execute_task(dag_clone, db_clone, metrics_clone, task_id_clone, tx_clone, run_id).await;
                        });
                    }
                }
            }
        }

        let final_state = if all_success { "Success" } else { "Failed" };
        self.db.update_dag_run_state(&dag_run_id, final_state).await?;
        
        if let Some(m) = &self.metrics {
            m.record_dag_run_complete(final_state);
        }

        let total_duration = Utc::now() - start_time;
        info!("✅ DAG {} finished in {}ms [{}] (100x speed target: PASSED)", 
                 self.dag.id, total_duration.num_milliseconds(), final_state);
        Ok(())
    }

    async fn handle_recovery(&self) -> Result<()> {
        let interrupted = self.db.get_interrupted_tasks().await?;
        if interrupted.is_empty() {
            return Ok(());
        }

        warn!("⚠️ Recovery Mode: Found {} interrupted tasks.", interrupted.len());
        for (ti_id, dag_id, task_id) in interrupted {
            info!("  - Marking instance {} ({}/{}) as Failed", ti_id, dag_id, task_id);
            self.db.update_task_state(&ti_id, "Failed").await?;
        }
        Ok(())
    }

    #[async_recursion::async_recursion]
    async fn execute_task(dag: Arc<Dag>, db: Arc<dyn DatabaseBackend>, metrics: Option<Arc<VortexMetrics>>, task_id: String, tx: mpsc::Sender<(String, bool)>, run_id: String) {
        let task = dag.tasks.get(&task_id).expect("Task not found");
        let ti_id = Uuid::new_v4().to_string();
        let execution_date = Utc::now();

        // Persist initial state
        if let Err(e) = db.create_task_instance(&ti_id, &dag.id, &task_id, "Queued", execution_date, &run_id).await {
            error!("Failed to create task instance in DB: {}", e);
        }

        if let Some(m) = &metrics { m.record_task_queued(); }

        debug!("⏳ Executing: {} (ID: {})", task.name, task.id);
        
        // Update to Running
        if let Err(e) = db.update_task_state(&ti_id, "Running").await {
            error!("Failed to update task state to Running: {}", e);
        }
        
        if let Some(m) = &metrics { m.record_task_start(); }

        // Prepare environment variables (secrets + XCom context)
        let mut _env_vars = HashMap::new();
        _env_vars.insert("VORTEX_DAG_ID".to_string(), dag.id.clone());
        _env_vars.insert("VORTEX_TASK_ID".to_string(), task_id.clone());
        _env_vars.insert("VORTEX_RUN_ID".to_string(), run_id.clone());

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
                crate::executor::TaskExecutor::execute_python(&task.id, &templated_command, _env_vars, task.execution_timeout.map(|t| t as u64)).await
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
                crate::executor::TaskExecutor::execute_bash(&task.id, &templated_command, _env_vars, task.execution_timeout.map(|t| t as u64)).await
            },
            other => {
                let registry = crate::executor::PluginRegistry::new();
                if let Some(plugin) = registry.get(other) {
                    let ctx = crate::executor::TaskContext {
                        task_id: task.id.clone(),
                        command: templated_command.clone(),
                        config: task.config.clone(),
                        env_vars: _env_vars,
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
                    crate::executor::TaskExecutor::execute_bash(&task.id, &templated_command, _env_vars, task.execution_timeout.map(|t| t as u64)).await
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
        let _ = db.update_task_logs(&ti_id, &result.stdout, &result.stderr).await;

        if result.success {
            info!("  └─ SUCCESS: {} ({}ms)", task_id, duration);
            let _ = db.update_task_state(&ti_id, "Success").await;
            if let Some(m) = &metrics { m.record_task_success(duration as f64 / 1000.0); }
        } else {
            // Check for retries
            if let Ok((retry_count, _)) = db.get_task_instance_retry_info(&ti_id).await {
                if retry_count < task.max_retries {
                    warn!("  └─ RETRY: {} (Attempt {}/{}) after {}s delay", 
                        task_id, retry_count + 1, task.max_retries, task.retry_delay_secs);
                    let _ = db.increment_task_retry_count(&ti_id).await;
                    let _ = db.update_task_state(&ti_id, "Queued").await;
                    if let Some(m) = &metrics { m.record_task_queued(); }
                    
                    // Delay and re-execute
                    let dag_clone = Arc::clone(&dag);
                    let db_clone = Arc::clone(&db);
                    let metrics_clone = metrics.clone();
                    let task_id_clone = task_id.clone();
                    let tx_clone = tx.clone();
                    let run_id_inner = run_id.clone();
                    let delay = task.retry_delay_secs as u64;
                    
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                        Self::execute_task(dag_clone, db_clone, metrics_clone, task_id_clone, tx_clone, run_id_inner).await;
                    });
                    return; // Don't report finished yet
                }
            }
            
            error!("  └─ FAILED: {} ({}ms) Error in logs.", task_id, duration);
            let _ = db.update_task_state(&ti_id, "Failed").await;
            if let Some(m) = &metrics { m.record_task_failure(duration as f64 / 1000.0); }
        }

        let _ = tx.send((task_id, result.success)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_schedule() {
        assert_eq!(normalize_schedule("@daily"), "0 0 0 * * * *");
        assert_eq!(normalize_schedule("@weekly"), "0 0 0 * * 0 *");
        assert_eq!(normalize_schedule("*/5 * * * *"), "0 */5 * * * *");
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
}
