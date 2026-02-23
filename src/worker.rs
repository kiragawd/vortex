use anyhow::Result;
use tokio::process::Command;
use chrono::Utc;
use std::time::Duration;

use crate::executor::TaskExecutor;

pub mod proto {
    tonic::include_proto!("vortex.swarm");
}

use proto::swarm_controller_client::SwarmControllerClient;
use proto::*;

pub async fn run_worker(controller_addr: &str, worker_id: &str, capacity: i32, labels: Vec<String>) -> Result<()> {
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    println!("🐝 VORTEX Worker starting...");
    println!("   ├─ Worker ID: {}", worker_id);
    println!("   ├─ Hostname: {}", host);
    println!("   ├─ Capacity: {} concurrent tasks", capacity);
    println!("   ├─ Labels: {:?}", labels);
    println!("   └─ Controller: {}", controller_addr);

    // Connect to controller with retry
    let mut client = loop {
        match SwarmControllerClient::connect(controller_addr.to_string()).await {
            Ok(c) => break c,
            Err(e) => {
                eprintln!("⚠️ Cannot connect to controller: {}. Retrying in 5s...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    };

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
    println!("✅ Registered with controller: {}", reg_response.message);

    let active_tasks = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let should_exit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Heartbeat loop
    let hb_worker_id = worker_id.to_string();
    let hb_addr = controller_addr.to_string();
    let hb_active = active_tasks.clone();
    let hb_exit = should_exit.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            
            let mut hb_client = match SwarmControllerClient::connect(hb_addr.clone()).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let response = hb_client.heartbeat(HeartbeatRequest {
                worker_id: hb_worker_id.clone(),
                active_tasks: hb_active.load(std::sync::atomic::Ordering::Relaxed),
                cpu_usage: 0.0,
                memory_usage: 0.0,
            }).await;

            if let Ok(resp) = response {
                if resp.into_inner().should_drain {
                    println!("⚠️ Controller requested drain. Finishing active tasks...");
                    hb_exit.store(true, std::sync::atomic::Ordering::Relaxed);
                    break;
                }
            }
        }
    });

    // Main poll loop
    println!("🚀 Worker polling for tasks...");
    loop {
        if should_exit.load(std::sync::atomic::Ordering::Relaxed) 
            && active_tasks.load(std::sync::atomic::Ordering::Relaxed) == 0 
        {
            println!("👋 Worker draining complete. Exiting.");
            break;
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
                eprintln!("⚠️ Poll error: {}. Retrying...", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                client = match SwarmControllerClient::connect(controller_addr.to_string()).await {
                    Ok(c) => c,
                    Err(_) => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };
                continue;
            }
        };

        if poll_response.tasks.is_empty() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        for task in poll_response.tasks {
            let active = active_tasks.clone();
            let addr = controller_addr.to_string();
            let wid = worker_id.to_string();

            active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            tokio::spawn(async move {
                let result = execute_task_remote(&task, &wid).await;
                if let Ok(mut report_client) = SwarmControllerClient::connect(addr).await {
                    let _ = report_client.report_task_result(result).await;
                }
                active.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            });
        }
    }

    Ok(())
}

async fn execute_task_remote(task: &TaskAssignment, worker_id: &str) -> TaskResult {
    println!("⏳ Executing: {}/{} (instance: {})", task.dag_id, task.task_id, task.task_instance_id);
    
    // Determine task type (bash or python)
    // For now, we'll try to guess from the command or assume it's bash unless it looks like python
    // Actually, in a real system, the TaskAssignment should have a task_type field.
    // Since we don't have it yet, let's check for a hint in the command or use bash as default.
    // Phase 2.2 says: "When worker receives a task from queue, check task type"
    // Since our proto doesn't have task_type, I'll assume we should use TaskExecutor based on some logic.
    
    let result = if task.command.starts_with("python:") {
        let code = task.command.strip_prefix("python:").unwrap_or(&task.command);
        TaskExecutor::execute_python(&task.task_id, code, task.secrets.clone()).await
    } else {
        TaskExecutor::execute_bash(&task.task_id, &task.command, task.secrets.clone()).await
    };

    println!("  └─ {}: {}/{} ({}ms)", 
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
    }
}
