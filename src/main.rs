use anyhow::Result;
use scheduler::{Dag, Scheduler};
use db::Db;
use rusqlite::params;
use std::env;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::sync::mpsc;
use chrono::Utc;
use swarm::SwarmState;
use vault::Vault;

mod scheduler;
mod python_parser;
mod db;
mod web;
mod swarm;
mod worker;
mod vault;
mod executor;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // 🐝 WORKER MODE
    if args.len() > 1 && args[1] == "worker" {
        let controller_addr = args.iter().position(|a| a == "--controller").and_then(|i| args.get(i + 1)).map(|s| s.to_string()).unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
        let worker_id = args.iter().position(|a| a == "--id").and_then(|i| args.get(i + 1)).map(|s| s.to_string()).unwrap_or_else(|| format!("worker-{}", &uuid::Uuid::new_v4().to_string()[..8]));
        let capacity: i32 = args.iter().position(|a| a == "--capacity").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(4);
        let labels: Vec<String> = args.iter().position(|a| a == "--labels").and_then(|i| args.get(i + 1)).map(|s| s.split(',').map(|l| l.trim().to_string()).collect()).unwrap_or_default();
        println!("🌪️ VORTEX Swarm Worker v0.6.0");
        return worker::run_worker(&controller_addr, &worker_id, capacity, labels).await;
    }

    // 🌪️ CONTROLLER MODE
    println!("🌪️ VORTEX Orchestrator v0.6.0 - Pillar 3 Operational");

    // Pillar 3: Initialize Secret Vault
    let vault = match Vault::new() {
        Ok(v) => { println!("🔐 Secret Vault initialized (AES-256-GCM)."); Some(Arc::new(v)) },
        Err(e) => { println!("⚠️ Secret Vault DISABLED: {}. Secrets will not be available.", e); None }
    };

    // Initialize DB
    let db = Arc::new(Db::init("vortex.db")?);
    println!("🗄️ Database initialized.");

    // Recovery Mode
    let interrupted = db.get_interrupted_tasks()?;
    if !interrupted.is_empty() {
        println!("⚠️ Recovery Mode: Found {} interrupted tasks from previous run.", interrupted.len());
        for (ti_id, dag_id, task_id) in interrupted {
            println!("  - Marking instance {} ({}/{}) as Failed", ti_id, dag_id, task_id);
            let _ = db.update_task_state(&ti_id, "Failed");
        }
    }

    let all_dags = Arc::new(Mutex::new(HashMap::new()));
    {
        let mut map = all_dags.lock().unwrap();
        let bench = create_benchmark_dag();
        println!("🛠️ Registering core DAG: {}", bench.id);
        map.insert(bench.id.clone(), Arc::new(bench));

        // Scan dags/
        let dags_dir = "dags";
        if std::path::Path::new(dags_dir).exists() {
            if let Ok(entries) = std::fs::read_dir(dags_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("py") {
                        if let Some(path_str) = path.to_str() {
                            println!("🐍 Loading DAG file: {}", path_str);
                            match python_parser::parse_python_dag(path_str) {
                                Ok(dags) => {
                                    for dag in dags { 
                                        println!("✅ Loaded DAG: {}", dag.id);
                                        let dag_id = dag.id.clone();
                                        map.insert(dag_id.clone(), Arc::new(dag));
                                        
                                        // Pillar 4: Force create version record for physical files
                                        let _ = db.store_dag_version(&dag_id, path_str);
                                    }
                                },
                                Err(e) => {
                                    println!("❌ Failed to parse DAG file {}: {}", path_str, e);
                                }
                            }
                        }
                    }
                }
            }
        }
        for dag in map.values() { db.register_dag(dag)?; }
    }
    println!("✅ Loaded DAGs.");

    // Swarm
    let swarm_enabled = args.iter().any(|a| a == "--swarm");
    let swarm_port: u16 = args.iter().position(|a| a == "--swarm-port").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(50051);
    let swarm_state = Arc::new(SwarmState::new(Arc::clone(&db), swarm_enabled, vault.clone()));

    if swarm_enabled {
        let grpc_state = Arc::clone(&swarm_state);
        let health_state = Arc::clone(&swarm_state);
        
        // Spawn gRPC server
        tokio::spawn(async move {
            let addr = format!("0.0.0.0:{}", swarm_port).parse().unwrap();
            let server = swarm::create_grpc_server(grpc_state);
            println!("🐝 Swarm Controller listening on {}", addr);
            let _ = tonic::transport::Server::builder().add_service(server).serve(addr).await;
        });

        // Pillar 4: Spawn Health Check Loop
        tokio::spawn(async move {
            health_state.health_check_loop().await;
        });
    }

    let (tx, mut rx) = mpsc::channel::<scheduler::ScheduleRequest>(32);

    // Web UI
    let db_web = Arc::clone(&db);
    let tx_web = tx.clone();
    let swarm_web = Arc::clone(&swarm_state);
    let vault_web = vault.clone();
    let dags_web = Arc::clone(&all_dags);
    tokio::spawn(async move {
        let server = web::WebServer::new(db_web, tx_web, swarm_web, vault_web, dags_web);
        server.run(3000).await;
    });

    // Scheduler Loop
    let db_sched = Arc::clone(&db);
    let dags_sched = Arc::clone(&all_dags);
    let swarm_sched = Arc::clone(&swarm_state);
    tokio::spawn(async move {
        println!("🌀 Scheduler loop started.");
        while let Some(req) = rx.recv().await {
            println!("🔔 Scheduler received request: {:?}", req);
            let dag = {
                let map = dags_sched.lock().unwrap();
                map.get(&req.dag_id).cloned()
            };

            if let Some(dag) = dag {
                let worker_count = swarm_sched.active_worker_count().await;
                println!("🔎 Scheduler: Found DAG {}. Swarm enabled: {}. Active workers: {}", req.dag_id, swarm_sched.enabled, worker_count);

                if swarm_sched.enabled && worker_count > 0 {
                    println!("🐝 Scheduler: Dispatching to SWARM mode.");
                    let dag_run_id = uuid::Uuid::new_v4().to_string();
                    let execution_date = Utc::now();
                    let _ = db_sched.create_dag_run(&dag_run_id, &req.dag_id, execution_date, &req.triggered_by);
                    let _ = db_sched.update_dag_run_state(&dag_run_id, "Running");
                    
                    let mut pre_finished_tasks = std::collections::HashSet::new();
                    if let scheduler::RunType::RetryFromFailure = req.run_type {
                        if let Ok(runs) = db_sched.get_dag_runs(&req.dag_id) {
                            if let Some(last_failed) = runs.iter().find(|r| r["state"] == "Failed") {
                                if let Some(run_id) = last_failed["id"].as_str() {
                                     let conn = db_sched.conn.lock().unwrap();
                                     let mut stmt = conn.prepare("SELECT task_id FROM task_instances WHERE dag_id = ?1 AND execution_date = (SELECT execution_date FROM dag_runs WHERE id = ?2) AND state = 'Success'").unwrap();
                                     let rows = stmt.query_map(params![req.dag_id, run_id], |row| row.get::<_, String>(0)).unwrap();
                                     for r in rows { if let Ok(tid) = r { pre_finished_tasks.insert(tid); } }
                                }
                            }
                        }
                    }

                    // --- Swarm Dependency Orchestrator ---
                    let dag_clone = Arc::clone(&dag);
                    let db_clone = Arc::clone(&db_sched);
                    let swarm_clone = Arc::clone(&swarm_sched);
                    let run_id_clone = dag_run_id.clone();
                    let execution_date_clone = execution_date;
                    
                    tokio::spawn(async move {
                        let mut in_degree = std::collections::HashMap::new();
                        let mut adj = std::collections::HashMap::new();

                        for task_id in dag_clone.tasks.keys() {
                            in_degree.insert(task_id.clone(), 0);
                            adj.insert(task_id.clone(), Vec::new());
                        }

                        for (up, down) in &dag_clone.dependencies {
                            if let Some(deg) = in_degree.get_mut(down) { *deg += 1; }
                            if let Some(v) = adj.get_mut(up) { v.push(down.clone()); }
                        }

                        // Mark pre-finished tasks as success and adjust degrees
                        let finished_tasks = pre_finished_tasks.clone();
                        for tid in &pre_finished_tasks {
                            if let Some(downstream) = adj.get(tid) {
                                for down in downstream {
                                    if let Some(deg) = in_degree.get_mut(down) { *deg -= 1; }
                                }
                            }
                        }

                        let (tx_done, mut rx_done) = tokio::sync::mpsc::channel(100);
                        let mut tasks_remaining = dag_clone.tasks.len() - finished_tasks.len();
                        
                        // Queue initial tasks
                        for (tid, &deg) in in_degree.iter() {
                            if deg == 0 && !finished_tasks.contains(tid) {
                                let task = dag_clone.tasks.get(tid).unwrap();
                                let ti_id = uuid::Uuid::new_v4().to_string();
                                let _ = db_clone.create_task_instance(&ti_id, &dag_clone.id, tid, "Queued", execution_date_clone, &run_id_clone);
                                
                                swarm_clone.enqueue_task(swarm::PendingTask {
                                    task_instance_id: ti_id.clone(), dag_id: dag_clone.id.clone(), task_id: tid.clone(),
                                    command: task.command.clone(), dag_run_id: run_id_clone.clone(),
                                    task_type: task.task_type.clone(), config_json: task.config.to_string(),
                                    max_retries: task.max_retries, retry_delay_secs: task.retry_delay_secs,
                                    required_secrets: vec!["STRESS_TEST_SECRET".to_string()],
                                }).await;

                                // Monitor this specific task
                                let db_mon = Arc::clone(&db_clone);
                                let tx_mon = tx_done.clone();
                                let tid_mon = tid.clone();
                                tokio::spawn(async move {
                                    loop {
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                        if let Ok((_, state)) = db_mon.get_task_instance_retry_info(&ti_id) {
                                            if state == "Success" { let _ = tx_mon.send((tid_mon, true)).await; break; }
                                            if state == "Failed" { let _ = tx_mon.send((tid_mon, false)).await; break; }
                                        }
                                    }
                                });
                            }
                        }

                        let mut all_success = true;
                        while tasks_remaining > 0 {
                            if let Some((finished_tid, success)) = rx_done.recv().await {
                                tasks_remaining -= 1;
                                if !success { all_success = false; }
                                
                                if let Some(downstream) = adj.get(&finished_tid) {
                                    for down in downstream {
                                        let deg = in_degree.get_mut(down).unwrap();
                                        *deg -= 1;
                                        if *deg == 0 {
                                            let task = dag_clone.tasks.get(down).unwrap();
                                            let ti_id = uuid::Uuid::new_v4().to_string();
                                            let _ = db_clone.create_task_instance(&ti_id, &dag_clone.id, down, "Queued", execution_date_clone, &run_id_clone);
                                            
                                            swarm_clone.enqueue_task(swarm::PendingTask {
                                                task_instance_id: ti_id.clone(), dag_id: dag_clone.id.clone(), task_id: down.clone(),
                                                command: task.command.clone(), dag_run_id: run_id_clone.clone(),
                                                task_type: task.task_type.clone(), config_json: task.config.to_string(),
                                                max_retries: task.max_retries, retry_delay_secs: task.retry_delay_secs,
                                                required_secrets: vec!["STRESS_TEST_SECRET".to_string()],
                                            }).await;

                                            let db_mon = Arc::clone(&db_clone);
                                            let tx_mon = tx_done.clone();
                                            let down_mon = down.clone();
                                            tokio::spawn(async move {
                                                loop {
                                                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                                    if let Ok((_, state)) = db_mon.get_task_instance_retry_info(&ti_id) {
                                                        if state == "Success" { let _ = tx_mon.send((down_mon, true)).await; break; }
                                                        if state == "Failed" { let _ = tx_mon.send((down_mon, false)).await; break; }
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        let _ = db_clone.update_dag_run_state(&run_id_clone, if all_success { "Success" } else { "Failed" });
                        println!("🏁 Swarm Orchestrator: DAG Run {} finished (Success: {})", run_id_clone, all_success);
                    });
                } else {
                    let scheduler = Scheduler::new_with_arc(Arc::clone(&dag), Arc::clone(&db_sched));
                    match req.run_type {
                        scheduler::RunType::Full => { let _ = scheduler.run_with_trigger(&req.triggered_by).await; },
                        scheduler::RunType::RetryFromFailure => { 
                             println!("⚠️ Standalone Retry not implemented yet (Swarm mode recommended)");
                             let _ = scheduler.run_with_trigger(&req.triggered_by).await;
                        }
                    }
                }
            }
        }
    });

    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn create_benchmark_dag() -> Dag {
    let mut dag = Dag::new("parallel_benchmark");
    dag.add_task("t1", "Warm-up", "echo 'Vortex engine warm-up...'");
    dag.add_task("t2", "A", "sleep 1 && echo 'Ingestion A complete'");
    dag.add_task("t3", "B", "sleep 1 && echo 'Ingestion B complete'");
    dag.add_task("t4", "C", "sleep 1 && echo 'Ingestion C complete'");
    dag.add_task("t5", "Final", "echo 'All data processed. Vortex out.'");
    dag.add_dependency("t1", "t2"); dag.add_dependency("t1", "t3"); dag.add_dependency("t1", "t4");
    dag.add_dependency("t2", "t5"); dag.add_dependency("t3", "t5"); dag.add_dependency("t4", "t5");
    dag
}
