use tracing::{info, warn};
use tonic::{Request, Response, Status};
use std::sync::Arc;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::RwLock;
use chrono::Utc;
use subtle::ConstantTimeEq;
use crate::db_trait::DatabaseBackend;
use crate::vault::Vault;

use crate::proto;

use proto::swarm_controller_server::{SwarmController, SwarmControllerServer};
use proto::*;
use std::fs;
use std::path::Path;

/// SECURITY (BUG-H11): Sanitize a path component to prevent directory traversal.
/// Replaces any character not in `[a-zA-Z0-9_-]` with `_`. Rejects empty input.
pub fn sanitize_path_component(input: &str) -> Result<String, &'static str> {
    if input.is_empty() {
        return Err("path component must not be empty");
    }
    Ok(input.chars().map(|c| {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            c
        } else {
            '_'
        }
    }).collect())
}

#[derive(Debug, Clone)]
pub struct WorkerState {
    pub worker_id: String,
    pub hostname: String,
    pub capacity: i32,
    pub active_tasks: i32,
    pub labels: Vec<String>,
    pub last_heartbeat: chrono::DateTime<Utc>,
    pub draining: bool,
}

#[derive(Debug, Clone)]
pub struct PendingTask {
    pub task_instance_id: String,
    pub dag_id: String,
    pub task_id: String,
    pub command: String,
    pub dag_run_id: String,
    pub task_type: String,
    pub config_json: String,
    pub max_retries: i32,
    pub retry_delay_secs: i32,
    pub required_secrets: Vec<String>,
    pub execution_timeout_secs: i32,  // BUG-16: Per-task timeout
}

pub struct SwarmState {
    pub workers: RwLock<HashMap<String, WorkerState>>,
    /// PERF-8: VecDeque for O(1) front removal when dispatching tasks.
    pub task_queue: RwLock<VecDeque<PendingTask>>,
    pub db: Arc<dyn DatabaseBackend>,
    pub vault: Option<Arc<Vault>>,
    pub metrics: Option<Arc<crate::metrics::RyuoMetrics>>,
    pub enabled: bool,
    /// Token used to authenticate gRPC requests from workers (BUG-C7).
    /// Loaded from `RYUO_GRPC_AUTH_TOKEN`. When `None`, auth is disabled (dev mode only).
    pub grpc_auth_token: Option<String>,
    /// PERF-6: Atomic counter of total registered workers for O(1) count.
    /// Counts all registered entries; stale workers are purged by health_check_cycle.
    pub worker_count: AtomicI64,
}

impl SwarmState {
    pub fn new(
        db: Arc<dyn DatabaseBackend>,
        enabled: bool,
        vault: Option<Arc<Vault>>,
        metrics: Option<Arc<crate::metrics::RyuoMetrics>>,
        grpc_auth_token: Option<String>,
    ) -> Self {
        Self {
            workers: RwLock::new(HashMap::new()),
            task_queue: RwLock::new(VecDeque::new()),
            db,
            vault,
            metrics,
            enabled,
            grpc_auth_token,
            worker_count: AtomicI64::new(0),
        }
    }

    pub async fn enqueue_task(&self, task: PendingTask) {
        let mut queue = self.task_queue.write().await;
        info!("🐝 Swarm: Task queued for remote execution: {}/{}", task.dag_id, task.task_id);
        queue.push_back(task); // PERF-8: push_back for VecDeque
    }

    pub async fn active_worker_count(&self) -> usize {
        // PERF-6: O(1) atomic read instead of iterating the workers HashMap.
        // Counts all registered workers; stale entries are removed by health_check_cycle.
        self.worker_count.load(Ordering::Relaxed).max(0) as usize
    }

    pub async fn get_workers_info(&self) -> Vec<serde_json::Value> {
        let workers = self.workers.read().await;
        let cutoff = Utc::now() - chrono::Duration::seconds(60);
        let total = workers.len();
        // PERF-7: Limit to 100 workers per call to bound response size.
        if total > 100 {
            tracing::debug!("get_workers_info: returning 100 of {} total workers", total);
        }
        workers.values().take(100).map(|w| {
            let status = if w.draining { "draining" } else if w.last_heartbeat > cutoff { "active" } else { "stale" };
            serde_json::json!({
                "worker_id": w.worker_id, "hostname": w.hostname, "capacity": w.capacity,
                "active_tasks": w.active_tasks, "labels": w.labels, "last_heartbeat": w.last_heartbeat,
                "status": status, "total_workers": total
            })
        }).collect()
    }

    pub async fn drain_worker(&self, worker_id: &str) -> bool {
        let mut workers = self.workers.write().await;
        if let Some(w) = workers.get_mut(worker_id) { w.draining = true; true } else { false }
    }

    pub async fn remove_worker(&self, worker_id: &str) -> bool {
        let removed = self.workers.write().await.remove(worker_id).is_some();
        if removed {
            // PERF-6: Keep atomic worker count in sync on removal.
            self.worker_count.fetch_sub(1, Ordering::Relaxed);
        }
        removed
    }

    pub async fn queue_depth(&self) -> usize {
        self.task_queue.read().await.len()
    }

    /// BUG-1 FIX: Single iteration of the health check, extracted so main.rs
    /// can call it in a loop that re-checks HA leadership between iterations.
    pub async fn health_check_cycle(&self) {
        if !self.enabled { return; }

        // Metrics
        if let Some(m) = &self.metrics {
            m.set_workers_active(self.active_worker_count().await as i64);
            m.set_queue_depth(self.queue_depth().await as i64);
        }

        // 1. Detect Offline Workers
        if let Ok(stale_workers) = self.db.mark_stale_workers_offline(60).await {
            for worker_id in stale_workers {
                warn!("⚠️ Swarm: Worker {} missed heartbeats. Marking OFFLINE.", worker_id);
                
                // 2. Re-queue tasks assigned to this worker
                if let Ok(count) = self.db.requeue_worker_tasks(&worker_id).await {
                    if count > 0 {
                        warn!("♻️ Swarm: Re-queued {} tasks from offline worker {}.", count, worker_id);
                        
                        // 3. Move them back to in-memory queue for scheduling
                        if let Ok(tasks) = self.db.get_interrupted_tasks_by_worker(&worker_id).await {
                            let mut queue = self.task_queue.write().await;
                            for t in tasks {
                                // t is (task_instance_id, dag_id, task_id, command, run_id, task_type, config_json, max_retries, retry_delay_secs, execution_timeout_secs)
                                queue.push_back(PendingTask { // PERF-8: push_back for VecDeque
                                    task_instance_id: t.0,
                                    dag_id: t.1,
                                    task_id: t.2,
                                    command: t.3,
                                    dag_run_id: t.4,
                                    task_type: t.5,
                                    config_json: t.6,
                                    max_retries: t.7,
                                    retry_delay_secs: t.8,
                                    required_secrets: vec![], // Resolved at poll time
                                    execution_timeout_secs: t.9,  // BUG-H3 FIX: propagate from DB
                                });
                            }
                        }
                        let _ = self.db.clear_worker_id_from_queued_tasks(&worker_id).await;
                    }
                }

                // 4. Remove from in-memory state
                let _ = self.remove_worker(&worker_id).await;
            }
        }
    }
}

pub struct SwarmService {
    pub state: Arc<SwarmState>,
}

#[tonic::async_trait]
impl SwarmController for SwarmService {
    async fn register_worker(&self, request: Request<WorkerInfo>) -> Result<Response<RegisterResponse>, Status> {
        let info = request.into_inner();
        let mut workers = self.state.workers.write().await;
        info!("🐝 Swarm: Worker registered: {}", info.worker_id);
        
        // Persistent Worker State
        let labels_str = info.labels.join(",");
        let _ = self.state.db.upsert_worker(&info.worker_id, &info.hostname, info.capacity, &labels_str).await;

        workers.insert(info.worker_id.clone(), WorkerState {
            worker_id: info.worker_id, hostname: info.hostname, capacity: info.capacity,
            active_tasks: 0, labels: info.labels, last_heartbeat: Utc::now(), draining: false,
        });
        // PERF-6: Increment atomic worker counter alongside HashMap insert.
        self.state.worker_count.fetch_add(1, Ordering::Relaxed);
        Ok(Response::new(RegisterResponse { accepted: true, message: "Welcome to the RYUO Swarm".to_string() }))
    }

    async fn heartbeat(&self, request: Request<HeartbeatRequest>) -> Result<Response<HeartbeatResponse>, Status> {
        let hb = request.into_inner();
        let mut workers = self.state.workers.write().await;
        
        // DB Heartbeat
        let _ = self.state.db.update_worker_heartbeat(&hb.worker_id, hb.active_tasks).await;

        let should_drain = if let Some(worker) = workers.get_mut(&hb.worker_id) {
            worker.last_heartbeat = Utc::now();
            worker.active_tasks = hb.active_tasks;
            worker.draining
        } else { true };
        Ok(Response::new(HeartbeatResponse { acknowledged: true, should_drain }))
    }

    async fn poll_task(&self, request: Request<PollTaskRequest>) -> Result<Response<PollTaskResponse>, Status> {
        let poll = request.into_inner();
        // PERF-8: Collect tasks under lock then release early to avoid holding the lock
        // during DB/vault calls and to fix the latent deadlock in the secret-error requeue path.
        let polled: Vec<PendingTask> = {
            let mut queue = self.state.task_queue.write().await;
            let count = std::cmp::min(poll.available_slots as usize, queue.len());
            (0..count).filter_map(|_| queue.pop_front()).collect()
        };

        let mut tasks = Vec::new();
        for t in polled {
            info!("🐝 Swarm: Dispatching {}/{} to worker {}", t.dag_id, t.task_id, poll.worker_id);
            // Assign task to worker in DB
            let _ = self.state.db.assign_task_to_worker(&t.task_instance_id, &poll.worker_id).await;
            // BUG-M3 FIX: Atomically update in-memory worker active_tasks count alongside DB assignment.
            {
                let mut workers = self.state.workers.write().await;
                if let Some(worker) = workers.get_mut(&poll.worker_id) {
                    worker.active_tasks += 1;
                }
            }
            // ARCH-1: Log errors on critical task state transitions instead of silently discarding.
            if let Err(e) = self.state.db.log_task_event(&t.task_instance_id, &t.dag_id, &t.task_id, &t.dag_run_id, "started", None, Some(&poll.worker_id)).await {
                tracing::error!(task = %t.task_id, dag = %t.dag_id, "Failed to log task start event: {}", e);
            }

            if let Some(m) = &self.state.metrics { m.record_task_start(); }

            // PERF-9: Resolve and Decrypt Secrets in a single batch DB query.
            let mut resolved_secrets = HashMap::new();
            let mut secret_errors: Vec<String> = Vec::new();
            if let Some(vault) = &self.state.vault {
                if !t.required_secrets.is_empty() {
                    match self.state.db.get_secrets_batch(&t.required_secrets).await {
                        Ok(batch) => {
                            for (secret_key, val) in batch {
                                match val {
                                    Some(encrypted) => {
                                        match vault.decrypt(&encrypted) {
                                            Ok(decrypted) => { resolved_secrets.insert(secret_key, decrypted); }
                                            Err(e) => { secret_errors.push(format!("{}: decryption failed: {}", secret_key, e)); }
                                        }
                                    }
                                    None => { secret_errors.push(format!("{}: not found in vault", secret_key)); }
                                }
                            }
                        }
                        Err(e) => { secret_errors.push(format!("batch secret lookup failed: {}", e)); }
                    }
                }
            }
            if !secret_errors.is_empty() {
                // SECURITY (BUG-M2): Log detailed secret errors only to tracing (stderr),
                // never to task events (stored in DB) to avoid leaking vault structure.
                tracing::error!(task = %t.task_id, dag = %t.dag_id, "Failed to resolve required secrets: {:?}", secret_errors);
                if let Err(e) = self.state.db.log_task_event(
                    &t.task_instance_id, &t.dag_id, &t.task_id, &t.dag_run_id,
                    "secret_error", Some("Failed to resolve required secrets — task re-queued"), None
                ).await {
                    tracing::error!(task = %t.task_id, "Failed to log secret_error event: {}", e);
                }
                // BUG-H2 FIX: Re-queue the task instead of dropping it silently.
                // Update state back to Queued so scheduler can retry.
                // ARCH-1: Log error if state update fails so the issue is visible.
                if let Err(e) = self.state.db.update_task_state(&t.task_instance_id, "Queued").await {
                    tracing::error!(task = %t.task_id, "Failed to reset task state to Queued after secret error: {}", e);
                }
                self.state.enqueue_task(t).await;
                continue;
            }

            tasks.push(TaskAssignment {
                task_instance_id: t.task_instance_id,
                dag_id: t.dag_id,
                task_id: t.task_id,
                command: t.command,
                dag_run_id: t.dag_run_id,
                secrets: resolved_secrets,
                task_type: t.task_type,
                config_json: t.config_json,
                max_retries: t.max_retries,
                retry_delay_secs: t.retry_delay_secs,
                execution_timeout_secs: t.execution_timeout_secs,  // BUG-16
            });
        }
        Ok(Response::new(PollTaskResponse { tasks }))
    }

    async fn report_task_result(&self, request: Request<TaskResult>) -> Result<Response<TaskResultAck>, Status> {
        let result = request.into_inner();
        
        // BUG-M3 FIX: Atomically decrement in-memory worker active_tasks on result.
        {
            let mut workers = self.state.workers.write().await;
            if let Some(worker) = workers.get_mut(&result.worker_id) {
                worker.active_tasks = (worker.active_tasks - 1).max(0);
            }
        }
        
        // Convert proto::TaskResult to executor::ExecutionResult for DB storage
        let exec_result = crate::executor::ExecutionResult {
            task_id: result.task_id.clone(),
            success: result.success,
            exit_code: if result.success { 0 } else { 1 },
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            duration_ms: result.duration_ms as u64,
        };
        let _ = self.state.db.store_task_result(&result.task_instance_id, &exec_result).await;

        if result.success {
            // BUG-H1 FIX: Pass actual dag_run_id instead of empty string
            // ARCH-1: Log error on critical success event failure.
            if let Err(e) = self.state.db.log_task_event(&result.task_instance_id, &result.dag_id, &result.task_id, &result.dag_run_id, "success", None, Some(&result.worker_id)).await {
                tracing::error!(task = %result.task_id, "Failed to log task success event: {}", e);
            }
            if let Some(m) = &self.state.metrics { m.record_task_success(result.duration_ms as f64 / 1000.0); }
        }

        // Retry Logic
        if !result.success {
            // BUG-H1 FIX: Pass actual dag_run_id instead of empty string
            // ARCH-1: Log error on critical failure event.
            if let Err(e) = self.state.db.log_task_event(&result.task_instance_id, &result.dag_id, &result.task_id, &result.dag_run_id, "failed", Some("Task failed on worker"), Some(&result.worker_id)).await {
                tracing::error!(task = %result.task_id, "Failed to log task failed event: {}", e);
            }
            if let Some(m) = &self.state.metrics { m.record_task_failure(result.duration_ms as f64 / 1000.0); }
            if let Ok((retry_count, _)) = self.state.db.get_task_instance_retry_info(&result.task_instance_id).await {
                if retry_count < result.max_retries {
                    warn!("♻️ Swarm: Task {} failed. Retrying ({}/{}).", result.task_id, retry_count + 1, result.max_retries);
                    let _ = self.state.db.increment_task_retry_count(&result.task_instance_id).await;
                    let _ = self.state.db.update_task_state(&result.task_instance_id, "Queued").await;
                    let msg = format!("Retrying task: attempt {}/{}", retry_count + 1, result.max_retries);
                    // BUG-H1 FIX: Pass actual dag_run_id instead of empty string
                    let _ = self.state.db.log_task_event(&result.task_instance_id, &result.dag_id, &result.task_id, &result.dag_run_id, "retry", Some(&msg), Some(&result.worker_id)).await;
                    
                    // Re-enqueue after delay
                    let state_clone = Arc::clone(&self.state);
                    let ti_id = result.task_instance_id.clone();
                    let retry_delay = result.retry_delay_secs;
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay as u64)).await;
                        
                        if let Ok(Some(details)) = state_clone.db.get_task_instance_details_full(&ti_id).await {
                            let (dag_id, task_id, command, dag_run_id, task_type, config_json, max_retries, retry_delay_secs, execution_timeout_secs) = details;
                            let required_secrets: Vec<String> = vec![];
                            state_clone.enqueue_task(PendingTask {
                                task_instance_id: ti_id,
                                dag_id,
                                task_id,
                                command,
                                dag_run_id,
                                task_type,
                                config_json,
                                max_retries,
                                retry_delay_secs,
                                required_secrets,
                                execution_timeout_secs,  // BUG-H3 FIX: propagated from DB
                            }).await;
                        }
                    });
                }
            }
        }
        
        let state_str = if result.success { "Success" } else { "Failed" };
        // SECURITY (BUG-H11): Sanitize dag_id and task_id to prevent directory traversal
        // (e.g., dag_id="../../etc" writing outside the logs directory).
        let safe_dag_id = sanitize_path_component(&result.dag_id).unwrap_or_else(|_| "_unknown_dag".to_string());
        let safe_task_id = sanitize_path_component(&result.task_id).unwrap_or_else(|_| "_unknown_task".to_string());
        let log_dir = format!("logs/{}/{}", safe_dag_id, safe_task_id);
        if let Err(e) = fs::create_dir_all(&log_dir) {
            warn!("⚠️ Swarm: Failed to create log directory {}: {}", log_dir, e);
        } else {
            let log_path = Path::new(&log_dir).join(format!("{}.log", Utc::now().format("%Y-%m-%d")));
            let log_content = format!(
                "--- REMOTE EXECUTION (Worker: {}) ---\nSTDOUT:\n{}\nSTDERR:\n{}\n--- STATUS: {} ---\n",
                result.worker_id, result.stdout, result.stderr, state_str
            );
            if let Err(e) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .and_then(|mut file| {
                    use std::io::Write;
                    file.write_all(log_content.as_bytes())
                }) {
                warn!("⚠️ Swarm: Failed to write to log file {:?}: {}", log_path, e);
            }
        }
        
        Ok(Response::new(TaskResultAck { acknowledged: true }))
    }
}

/// gRPC authentication interceptor that validates bearer tokens on every request (BUG-C7).
/// Uses constant-time comparison via `subtle::ConstantTimeEq` to prevent timing attacks (SEC-11).
#[derive(Clone)]
pub struct GrpcAuthInterceptor {
    auth_token: Option<String>,
}

impl tonic::service::Interceptor for GrpcAuthInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        let Some(expected) = &self.auth_token else {
            // No auth token configured (dev mode) — allow all requests
            return Ok(req);
        };

        let provided = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Status::unauthenticated("Invalid or missing auth token"))?;

        let provided = provided.strip_prefix("Bearer ").unwrap_or(provided);

        // Constant-time comparison to prevent timing attacks (SEC-11)
        if expected.as_bytes().ct_eq(provided.as_bytes()).into() {
            Ok(req)
        } else {
            Err(Status::unauthenticated("Invalid or missing auth token"))
        }
    }
}

/// Build the gRPC SwarmController server with authentication interceptor.
/// The auth token is read from `SwarmState::grpc_auth_token`.
pub fn create_grpc_server(
    state: Arc<SwarmState>,
) -> tonic::service::interceptor::InterceptedService<SwarmControllerServer<SwarmService>, GrpcAuthInterceptor> {
    let interceptor = GrpcAuthInterceptor {
        auth_token: state.grpc_auth_token.clone(),
    };
    SwarmControllerServer::with_interceptor(SwarmService { state }, interceptor)
}
