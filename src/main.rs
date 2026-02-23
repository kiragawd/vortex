use anyhow::Result;
use scheduler::{Dag, Scheduler};
use db::Db;
use std::env;
use std::sync::Arc;
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

    let mut all_dags = HashMap::new();
    let bench = create_benchmark_dag();
    all_dags.insert(bench.id.clone(), Arc::new(bench));

    // Scan dags/
    let dags_dir = "dags";
    if std::path::Path::new(dags_dir).exists() {
        if let Ok(entries) = std::fs::read_dir(dags_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("py") {
                    if let Some(path_str) = path.to_str() {
                        if let Ok(dags) = python_parser::parse_python_dag(path_str) {
                            for dag in dags { all_dags.insert(dag.id.clone(), Arc::new(dag)); }
                        }
                    }
                }
            }
        }
    }

    for dag in all_dags.values() { db.register_dag(dag)?; }
    println!("✅ Loaded {} DAGs.", all_dags.len());

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

    let (tx, mut rx) = mpsc::channel::<(String, String)>(32);

    // Web UI
    let db_web = Arc::clone(&db);
    let tx_web = tx.clone();
    let swarm_web = Arc::clone(&swarm_state);
    let vault_web = vault.clone();
    tokio::spawn(async move {
        let server = web::WebServer::new(db_web, tx_web, swarm_web, vault_web);
        server.run(3000).await;
    });

    // Scheduler Loop
    let db_sched = Arc::clone(&db);
    let dags_sched = all_dags.clone();
    let swarm_sched = Arc::clone(&swarm_state);
    tokio::spawn(async move {
        while let Some((dag_id, triggered_by)) = rx.recv().await {
            if let Some(dag) = dags_sched.get(&dag_id) {
                if swarm_sched.enabled && swarm_sched.active_worker_count().await > 0 {
                    let dag_run_id = uuid::Uuid::new_v4().to_string();
                    let _ = db_sched.create_dag_run(&dag_run_id, &dag_id, Utc::now(), &triggered_by);
                    let _ = db_sched.update_dag_run_state(&dag_run_id, "Running");
                    for task in dag.tasks.values() {
                        let ti_id = uuid::Uuid::new_v4().to_string();
                        let _ = db_sched.create_task_instance(&ti_id, &dag_id, &task.id, Utc::now());
                        swarm_sched.enqueue_task(swarm::PendingTask {
                            task_instance_id: ti_id, dag_id: dag_id.clone(), task_id: task.id.clone(),
                            command: task.command.clone(), dag_run_id: dag_run_id.clone(),
                            required_secrets: vec!["STRESS_TEST_SECRET".to_string()], // Pillar 3: Pass secret reqs
                        }).await;
                    }
                } else {
                    let scheduler = Scheduler::new_with_arc(Arc::clone(dag), Arc::clone(&db_sched));
                    let _ = scheduler.run_with_trigger(&triggered_by).await;
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
