use tracing::{info, warn, error};
use anyhow::Result;
use std::time::Duration;

use crate::executor::TaskExecutor;

use crate::proto;

use proto::swarm_controller_client::SwarmControllerClient;
use proto::*;

pub async fn run_worker(controller_addr: &str, worker_id: &str, capacity: i32, labels: Vec<String>) -> Result<()> {
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    info!("🐝 VORTEX Worker starting...");
    info!("   ├─ Worker ID: {}", worker_id);
    info!("   ├─ Hostname: {}", host);
    info!("   ├─ Capacity: {} concurrent tasks", capacity);
    info!("   ├─ Labels: {:?}", labels);
    info!("   └─ Controller: {}", controller_addr);

    // Connect to controller with retry
    // TODO: Add --tls-ca flag for worker TLS: if provided, use tonic::transport::Channel
    //       with ClientTlsConfig::new().ca_certificate(...) for mTLS/TLS to the controller.
    let channel = loop {
        match tonic::transport::Endpoint::from_shared(controller_addr.to_string())?
            .connect()
            .await 
        {
            Ok(c) => break c,
            Err(e) => {
                warn!("⚠️ Cannot connect to controller: {}. Retrying in 5s...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    };
    let mut client = SwarmControllerClient::new(channel.clone());

    // Register
    let reg_response = client.register_worker(WorkerInfo {
        worker_id: worker_id.to_string(),
        hostname: host.clone(),
        capacity,
        labels: labels.clone(),
    }).await?.into_inner();

    if !reg_response.accepted {
        anyhow::bail!("Worker registration rejected: {}", reg_response.message);
    }
    info!("✅ Registered with controller: {}", reg_response.message);

    let active_tasks = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let should_exit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Heartbeat loop
    let hb_worker_id = worker_id.to_string();
    let hb_active = active_tasks.clone();
    let hb_exit = should_exit.clone();
    let mut hb_client = SwarmControllerClient::new(channel.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            
            let response = hb_client.heartbeat(HeartbeatRequest {
                worker_id: hb_worker_id.clone(),
                active_tasks: hb_active.load(std::sync::atomic::Ordering::Relaxed),
                cpu_usage: 0.0,
                memory_usage: 0.0,
            }).await;

            match response {
                Ok(resp) => {
                    if resp.into_inner().should_drain {
                        warn!("⚠️ Controller requested drain. Finishing active tasks...");
                        hb_exit.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                }
                Err(e) => {
                    warn!("⚠️ Heartbeat failed: {}. Continuing...", e);
                }
            }
        }
    });

    // Main poll loop
    info!("🚀 Worker polling for tasks...");
    loop {
        if should_exit.load(std::sync::atomic::Ordering::Relaxed) 
            && active_tasks.load(std::sync::atomic::Ordering::Relaxed) == 0 
        {
            info!("👋 Worker draining complete. Exiting.");
            break;
        }

        if should_exit.load(std::sync::atomic::Ordering::Relaxed) {
             tokio::time::sleep(Duration::from_secs(1)).await;
             continue;
        }

        let current_active = active_tasks.load(std::sync::atomic::Ordering::Relaxed);
        let available = capacity - current_active;

        if available <= 0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        let poll_response = match client.poll_task(PollTaskRequest {
            worker_id: worker_id.to_string(),
            available_slots: available,
        }).await {
            Ok(r) => r.into_inner(),
            Err(e) => {
                warn!("⚠️ Poll error: {}. Retrying...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;

                // Bug 24 fix: gRPC channels reconnect transparently, but the
                // controller's in-memory worker registry is lost on its restart.
                // Re-register whenever we get back a NOT_FOUND or UNAUTHENTICATED
                // status, so task polling resumes correctly without manual
                // intervention. Other errors are treated as transient.
                let code = e.code();
                if code == tonic::Code::NotFound || code == tonic::Code::Unauthenticated || code == tonic::Code::Unavailable {
                    warn!("🔄 Re-registering worker after controller loss...");
                    match client.register_worker(WorkerInfo {
                        worker_id: worker_id.to_string(),
                        hostname: host.clone(),
                        capacity,
                        labels: labels.clone(),
                    }).await {
                        Ok(resp) => {
                            let r = resp.into_inner();
                            if r.accepted {
                                info!("✅ Re-registered with controller: {}", r.message);
                            } else {
                                warn!("⚠️ Re-registration rejected: {}", r.message);
                            }
                        }
                        Err(re) => warn!("⚠️ Re-registration also failed: {}", re),
                    }
                }
                continue;
            }
        };

        if poll_response.tasks.is_empty() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        for task in poll_response.tasks {
            let active = active_tasks.clone();
            let mut report_client = client.clone();
            let wid = worker_id.to_string();

            active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            tokio::spawn(async move {
                let result = execute_task_remote(&task, &wid).await;
                if let Err(e) = report_client.report_task_result(result).await {
                    error!("❌ Failed to report task result to swarm: {}", e);
                }
                active.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            });
        }
    }

    Ok(())
}

async fn execute_task_remote(task: &TaskAssignment, worker_id: &str) -> TaskResult {
    info!("⏳ Executing: {}/{} (type: {}, instance: {})", 
        task.dag_id, task.task_id, task.task_type, task.task_instance_id);
    
    let result = match task.task_type.as_str() {
        "python" => {
            TaskExecutor::execute_python(&task.task_id, &task.command, task.secrets.clone(), None).await
        },
        "bash" => {
            TaskExecutor::execute_bash(&task.task_id, &task.command, task.secrets.clone(), None).await
        },
        other_type => {
            if let Some(plugin) = crate::executor::get_plugin(other_type) {
                let ctx = crate::executor::TaskContext {
                    task_id: task.task_id.clone(),
                    command: task.command.clone(),
                    config: serde_json::from_str(&task.config_json).unwrap_or(serde_json::json!({})),
                    env_vars: task.secrets.clone(),
                };
                match plugin.execute(&ctx).await {
                    Ok(res) => res,
                    Err(e) => crate::executor::ExecutionResult {
                        task_id: task.task_id.clone(),
                        success: false,
                        exit_code: -1,
                        stdout: String::new(),
                        stderr: format!("Plugin error: {}", e),
                        duration_ms: 0,
                    }
                }
            } else {
                TaskExecutor::execute_bash(&task.task_id, &task.command, task.secrets.clone(), None).await
            }
        }
    };

    info!("  └─ {}: {}/{} ({}ms)", 
        if result.success { "SUCCESS" } else { "FAILED" },
        task.dag_id, task.task_id, result.duration_ms);

    TaskResult {
        worker_id: worker_id.to_string(),
        task_instance_id: task.task_instance_id.clone(),
        dag_id: task.dag_id.clone(),
        task_id: task.task_id.clone(),
        success: result.success,
        stdout: result.stdout,
        stderr: result.stderr,
        duration_ms: result.duration_ms as i64,
        max_retries: task.max_retries,
        retry_delay_secs: task.retry_delay_secs,
    }
}
