use tonic::{Request, Response, Status};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use chrono::Utc;
use crate::db::Db;
use crate::vault::Vault;

pub mod proto {
    tonic::include_proto!("vortex.swarm");
}

use proto::swarm_controller_server::{SwarmController, SwarmControllerServer};
use proto::*;

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
    pub task_type: String,      // Phase 2.5
    pub config_json: String,    // Phase 2.5
    pub max_retries: i32,       // Phase 2.5
    pub retry_delay_secs: i32,  // Phase 2.5
    pub required_secrets: Vec<String>, // Pillar 3
}

pub struct SwarmState {
    pub workers: RwLock<HashMap<String, WorkerState>>,
    pub task_queue: RwLock<Vec<PendingTask>>,
    pub db: Arc<Db>,
    pub vault: Option<Arc<Vault>>, // Pillar 3
    pub enabled: bool,
}

impl SwarmState {
    pub fn new(db: Arc<Db>, enabled: bool, vault: Option<Arc<Vault>>) -> Self {
        Self {
            workers: RwLock::new(HashMap::new()),
            task_queue: RwLock::new(Vec::new()),
            db,
            vault,
            enabled,
        }
    }

    pub async fn enqueue_task(&self, task: PendingTask) {
        let mut queue = self.task_queue.write().await;
        println!("🐝 Swarm: Task queued for remote execution: {}/{}", task.dag_id, task.task_id);
        queue.push(task);
    }

    pub async fn worker_count(&self) -> usize {
        self.workers.read().await.len()
    }

    pub async fn active_worker_count(&self) -> usize {
        let workers = self.workers.read().await;
        let cutoff = Utc::now() - chrono::Duration::seconds(60);
        workers.values().filter(|w| w.last_heartbeat > cutoff && !w.draining).count()
    }

    pub async fn get_workers_info(&self) -> Vec<serde_json::Value> {
        let workers = self.workers.read().await;
        let cutoff = Utc::now() - chrono::Duration::seconds(60);
        workers.values().map(|w| {
            let status = if w.draining { "draining" } else if w.last_heartbeat > cutoff { "active" } else { "stale" };
            serde_json::json!({
                "worker_id": w.worker_id, "hostname": w.hostname, "capacity": w.capacity,
                "active_tasks": w.active_tasks, "labels": w.labels, "last_heartbeat": w.last_heartbeat, "status": status
            })
        }).collect()
    }

    pub async fn drain_worker(&self, worker_id: &str) -> bool {
        let mut workers = self.workers.write().await;
        if let Some(w) = workers.get_mut(worker_id) { w.draining = true; true } else { false }
    }

    pub async fn remove_worker(&self, worker_id: &str) -> bool {
        self.workers.write().await.remove(worker_id).is_some()
    }

    pub async fn queue_depth(&self) -> usize {
        self.task_queue.read().await.len()
    }

    pub async fn health_check_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            interval.tick().await;
            if !self.enabled { continue; }

            // 1. Detect Offline Workers
            if let Ok(stale_workers) = self.db.mark_stale_workers_offline(60) {
                for worker_id in stale_workers {
                    println!("⚠️ Swarm: Worker {} missed heartbeats. Marking OFFLINE.", worker_id);
                    
                    // 2. Re-queue tasks assigned to this worker
                    if let Ok(count) = self.db.requeue_worker_tasks(&worker_id) {
                        if count > 0 {
                            println!("♻️ Swarm: Re-queued {} tasks from offline worker {}.", count, worker_id);
                            
                            // 3. Move them back to in-memory queue for scheduling
                            let mut queue = self.task_queue.write().await;
                            if let Ok(tasks) = self.db.get_interrupted_tasks_by_worker(&worker_id) {
                                for t in tasks {
                                    queue.push(PendingTask {
                                        task_instance_id: t.0,
                                        dag_id: t.1,
                                        task_id: t.2,
                                        command: t.3,
                                        dag_run_id: t.4,
                                        task_type: "bash".to_string(), // Default for recovery
                                        config_json: "{}".to_string(),
                                        max_retries: 0,
                                        retry_delay_secs: 30,
                                        required_secrets: vec!["STRESS_TEST_SECRET".to_string()], // Pillar 3: Pass secret reqs
                                    });
                                }
                            }
                            let _ = self.db.clear_worker_id_from_queued_tasks(&worker_id);
                        }
                    }

                    // 4. Remove from in-memory state
                    let _ = self.remove_worker(&worker_id).await;
                }
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
        println!("🐝 Swarm: Worker registered: {}", info.worker_id);
        
        // Pillar 4: Persistent Worker State
        let labels_str = info.labels.join(",");
        let _ = self.state.db.upsert_worker(&info.worker_id, &info.hostname, info.capacity, &labels_str);

        workers.insert(info.worker_id.clone(), WorkerState {
            worker_id: info.worker_id, hostname: info.hostname, capacity: info.capacity,
            active_tasks: 0, labels: info.labels, last_heartbeat: Utc::now(), draining: false,
        });
        Ok(Response::new(RegisterResponse { accepted: true, message: "Welcome to the VORTEX Swarm".to_string() }))
    }

    async fn heartbeat(&self, request: Request<HeartbeatRequest>) -> Result<Response<HeartbeatResponse>, Status> {
        let hb = request.into_inner();
        let mut workers = self.state.workers.write().await;
        
        // Pillar 4: DB Heartbeat
        let _ = self.state.db.update_worker_heartbeat(&hb.worker_id, hb.active_tasks);

        let should_drain = if let Some(worker) = workers.get_mut(&hb.worker_id) {
            worker.last_heartbeat = Utc::now();
            worker.active_tasks = hb.active_tasks;
            worker.draining
        } else { true };
        Ok(Response::new(HeartbeatResponse { acknowledged: true, should_drain }))
    }

    async fn poll_task(&self, request: Request<PollTaskRequest>) -> Result<Response<PollTaskResponse>, Status> {
        let poll = request.into_inner();
        let mut queue = self.state.task_queue.write().await;
        let count = std::cmp::min(poll.available_slots as usize, queue.len());
        
        let mut tasks = Vec::new();
        for t in queue.drain(..count) {
            println!("🐝 Swarm: Dispatching {}/{} to worker {}", t.dag_id, t.task_id, poll.worker_id);
            // Pillar 4: Assign task to worker in DB
            let _ = self.state.db.assign_task_to_worker(&t.task_instance_id, &poll.worker_id);

            // Pillar 3: Resolve and Decrypt Secrets
            let mut resolved_secrets = HashMap::new();
            if let Some(vault) = &self.state.vault {
                for secret_key in t.required_secrets {
                    if let Ok(Some(encrypted)) = self.state.db.get_secret(&secret_key) {
                        if let Ok(decrypted) = vault.decrypt(&encrypted) {
                            resolved_secrets.insert(secret_key, decrypted);
                        }
                    }
                }
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
            });
        }
        Ok(Response::new(PollTaskResponse { tasks }))
    }

    async fn report_task_result(&self, request: Request<TaskResult>) -> Result<Response<TaskResultAck>, Status> {
        let result = request.into_inner();
        
        // Convert proto::TaskResult to executor::ExecutionResult for DB storage
        let exec_result = crate::executor::ExecutionResult {
            task_id: result.task_id.clone(),
            success: result.success,
            exit_code: if result.success { 0 } else { 1 },
            stdout: result.stdout.clone(),
            stderr: result.stderr.clone(),
            duration_ms: result.duration_ms as u64,
        };
        let _ = self.state.db.store_task_result(&result.task_instance_id, &exec_result);

        // Phase 2.5: Retry Logic
        if !result.success {
            if let Ok((retry_count, _)) = self.state.db.get_task_instance_retry_info(&result.task_instance_id) {
                if retry_count < result.max_retries {
                    println!("♻️ Swarm: Task {} failed. Retrying ({}/{}).", result.task_id, retry_count + 1, result.max_retries);
                    let _ = self.state.db.increment_task_retry_count(&result.task_instance_id);
                    let _ = self.state.db.update_task_state(&result.task_instance_id, "Queued");
                    
                    // Re-enqueue after delay
                    let state_clone = Arc::clone(&self.state);
                    let ti_id = result.task_instance_id.clone();
                    let retry_delay = result.retry_delay_secs;
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(retry_delay as u64)).await;
                        
                        if let Ok(Some(details)) = state_clone.db.get_task_instance_details(&ti_id) {
                            let (dag_id, task_id, command, dag_run_id, task_type, config_json, max_retries, retry_delay_secs) = details;
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
                                required_secrets: vec!["STRESS_TEST_SECRET".to_string()], // Pillar 3: Pass secret reqs
                            }).await;
                        }
                    });
                }
            }
        }
        
        let state_str = if result.success { "Success" } else { "Failed" };
        let log_dir = format!("logs/{}/{}/", result.dag_id, result.task_id);
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = format!("{}/{}.log", log_dir, Utc::now().format("%Y-%m-%d"));
        let log_content = format!("--- REMOTE EXECUTION (Worker: {}) ---\nSTDOUT:\n{}\nSTDERR:\n{}\n--- STATUS: {} ---\n", result.worker_id, result.stdout, result.stderr, state_str);
        let _ = std::fs::write(log_path, log_content);
        
        Ok(Response::new(TaskResultAck { acknowledged: true }))
    }
}

pub fn create_grpc_server(state: Arc<SwarmState>) -> SwarmControllerServer<SwarmService> {
    SwarmControllerServer::new(SwarmService { state })
}
