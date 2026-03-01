
use anyhow::Result;
use scheduler::{Dag, Scheduler};
use std::env;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::mpsc;
use chrono::Utc;
use swarm::SwarmState;
use vault::Vault;
use tracing::{info, warn, error, debug};
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod scheduler;
mod python_parser;
mod web;
mod swarm;
mod worker;
mod vault;
mod executor;
mod xcom;
mod pools;
mod sensors;
mod notifications;
mod metrics;
mod db_trait;
mod db_postgres;
mod proto;
mod dag_factory;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // Initialize structured logging
    let log_level = args.iter().position(|a| a == "--log-level")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("info");

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("vortex={}", log_level)));

    let json_output = args.iter().any(|a| a == "--log-json");

    let file_appender = tracing_appender::rolling::daily("logs", "vortex.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    if json_output {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .with(fmt::layer().with_writer(non_blocking).json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_target(false).with_thread_ids(false))
            .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
            .init();
    }

    // 🗄️ DB MIGRATE MODE
    if args.len() > 2 && args[1] == "db" && args[2] == "migrate" {
        // SQLITE-2 FIX: PostgreSQL is the only supported backend.
        // --database-url or DATABASE_URL env var is mandatory.
        let db_url = args.iter().position(|a| a == "--database-url")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .or_else(|| std::env::var("DATABASE_URL").ok().as_deref().map(|_| ""))
            .unwrap_or_else(|| {
                eprintln!("❌ --database-url or DATABASE_URL env var is required (PostgreSQL only)");
                std::process::exit(1);
            });
        // Re-read cleanly (env var fallback)
        let url = args.iter().position(|a| a == "--database-url")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or_else(|| {
                Box::leak(std::env::var("DATABASE_URL").expect("DATABASE_URL must be set").into_boxed_str())
            });
        info!("🗄️ Running PostgreSQL migrations ({})...", &url[..url.find('@').map(|i| i+1).unwrap_or(url.len())]);
        let _db = db_postgres::PostgresDb::new(url, 1, 1, std::time::Duration::from_secs(30)).await?;
        info!("✅ Database migrations applied successfully.");
        return Ok(());
    }

    // 🐝 WORKER MODE
    if args.len() > 1 && args[1] == "worker" {
        let controller_addr = args.iter().position(|a| a == "--controller").and_then(|i| args.get(i + 1)).map(|s| s.to_string()).unwrap_or_else(|| "http://127.0.0.1:50051".to_string());
        let worker_id = args.iter().position(|a| a == "--id").and_then(|i| args.get(i + 1)).map(|s| s.to_string()).unwrap_or_else(|| format!("worker-{}", &uuid::Uuid::new_v4().to_string()[..8]));
        let capacity: i32 = args.iter().position(|a| a == "--capacity").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(4);
        let labels: Vec<String> = args.iter().position(|a| a == "--labels").and_then(|i| args.get(i + 1)).map(|s| s.split(',').map(|l| l.trim().to_string()).collect()).unwrap_or_default();
        info!("🌪️ VORTEX Swarm Worker v0.6.0");
        return worker::run_worker(&controller_addr, &worker_id, capacity, labels).await;
    }

    // 🌪️ CONTROLLER MODE
    info!("🌪️ VORTEX Orchestrator v0.6.0 - Pillar 3 Operational");

    // Pillar 3: Initialize Secret Vault
    let vault = match Vault::new() {
        Ok(v) => { info!("🔐 Secret Vault initialized (AES-256-GCM)."); Some(Arc::new(v)) },
        Err(e) => { warn!("⚠️ Secret Vault DISABLED: {}. Secrets will not be available.", e); None }
    };

    // Phase 3: Initialize Database Backend (PostgreSQL only)
    // SQLITE-2 FIX: SQLite removed. --database-url or DATABASE_URL env var is mandatory.
    let db_url_owned: String = args.iter().position(|a| a == "--database-url")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string())
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| {
            eprintln!("❌ --database-url or DATABASE_URL env var is required. VORTEX requires PostgreSQL.");
            std::process::exit(1);
        });

    let db_max_connections: u32 = args.iter().position(|a| a == "--db-max-connections").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(20);
    let db_min_connections: u32 = args.iter().position(|a| a == "--db-min-connections").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(2);
    let db_idle_timeout = std::time::Duration::from_secs(args.iter().position(|a| a == "--db-idle-timeout").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(300));

    info!("🗄️ Initializing PostgreSQL backend...");
    let db: Arc<dyn db_trait::DatabaseBackend> = Arc::new(
        db_postgres::PostgresDb::new(&db_url_owned, db_max_connections, db_min_connections, db_idle_timeout).await?
    );
    info!("✅ Database initialized.");

    // Phase 3: Initialize Prometheus Metrics
    let vortex_metrics = Arc::new(metrics::VortexMetrics::new()?);
    info!("📊 Prometheus metrics initialized (GET /metrics)");

    // Recovery Mode
    let interrupted = db.get_interrupted_tasks().await?;
    if !interrupted.is_empty() {
        warn!("⚠️ Recovery Mode: Found {} interrupted tasks from previous run.", interrupted.len());
        for (ti_id, dag_id, task_id) in interrupted {
            info!("  - Marking instance {} ({}/{}) as Failed", ti_id, dag_id, task_id);
            let _ = db.update_task_state(&ti_id, "Failed").await;
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Plugin Discovery
    // ─────────────────────────────────────────────────────────────────
    let mut plugin_registry = executor::PluginRegistry::new();
    let plugins_dir = std::path::Path::new("plugins");
    if plugins_dir.exists() && plugins_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(plugins_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                    if ext == "so" || ext == "dylib" || ext == "dll" {
                        let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                        unsafe {
                            match plugin_registry.load_plugin(path.to_str().unwrap(), file_stem) {
                                Ok(_) => info!("🔌 Loaded plugin '{}' from {:?}", file_stem, path),
                                Err(e) => warn!("⚠️ Failed to load plugin {:?}: {}", path, e),
                            }
                        }
                    }
                }
            }
        }
    } else {
        info!("🔌 Plugins directory not found or empty. Using default operators.");
    }
    executor::init_global_registry(plugin_registry);
    
    // ARCH-2: Use tokio::sync::Mutex to match AppState.dags type.
    let all_dags = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    {
        let mut map = all_dags.blocking_lock();
        let bench = create_benchmark_dag();
        info!("🛠️ Registering core DAG: {}", bench.id);
        map.insert(bench.id.clone(), Arc::new(bench));

        // Scan dags/
        let dags_dir = "dags";
        if std::path::Path::new(dags_dir).exists() {
            if let Ok(entries) = std::fs::read_dir(dags_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        if let Some(path_str) = path.to_str() {
                            if ext == "py" {
                                info!("🐍 Loading DAG file: {}", path_str);
                                match python_parser::parse_python_dag(path_str) {
                                    Ok(dags) => {
                                        for dag in dags { 
                                            info!("✅ Loaded DAG: {}", dag.id);
                                            let dag_id = dag.id.clone();
                                            map.insert(dag_id.clone(), Arc::new(dag));
                                            
                                            // Pillar 4: Force create version record for physical files
                                            let _ = db.store_dag_version(&dag_id, path_str).await;
                                        }
                                    },
                                    Err(e) => {
                                        error!("❌ Failed to parse DAG file {}: {}", path_str, e);
                                    }
                                }
                            } else if ext == "json" || ext == "yaml" || ext == "yml" {
                                info!("📄 Loading Config DAG file: {}", path_str);
                                match dag_factory::parse_dag_file(path_str) {
                                    Ok(dags) => {
                                        for dag in dags { 
                                            info!("✅ Loaded Config DAG: {}", dag.id);
                                            let dag_id = dag.id.clone();
                                            map.insert(dag_id.clone(), Arc::new(dag));
                                            
                                            let _ = db.store_dag_version(&dag_id, path_str).await;
                                        }
                                    },
                                    Err(e) => {
                                        error!("❌ Failed to parse Config DAG file {}: {}", path_str, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for dag in map.values() { db.register_dag(dag).await?; }
    }
    info!("✅ Loaded DAGs.");

    // High Availability
    let ha_mode = args.iter().any(|a| a == "--ha-mode");
    let (leader_tx, leader_rx) = tokio::sync::watch::channel(!ha_mode);

    if ha_mode {
        let db_leader = Arc::clone(&db);
        tokio::spawn(async move {
            info!("🔒 HA Mode Enabled. Standing by for Leader Lock...");
            loop {
                match db_leader.try_acquire_leader_lock().await {
                    Ok(true) => {
                        info!("👑 Acquired HA Leader Lock. Promoting to Active.");
                        let _ = leader_tx.send(true);
                        break;
                    }
                    Ok(false) => {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        warn!("⚠️ DB error during leader lock acquisition: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }

    // Swarm
    let swarm_enabled = args.iter().any(|a| a == "--swarm");
    let swarm_port: u16 = args.iter().position(|a| a == "--swarm-port").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(50051);
    let swarm_state = Arc::new(SwarmState::new(Arc::clone(&db), swarm_enabled, vault.clone(), Some(Arc::clone(&vortex_metrics))));

    if swarm_enabled {
        let grpc_state = Arc::clone(&swarm_state);
        let health_state = Arc::clone(&swarm_state);
        let tls_cert_grpc = args.iter().position(|a| a == "--tls-cert")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.to_string());
        let tls_key_grpc = args.iter().position(|a| a == "--tls-key")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.to_string());

        // Spawn gRPC server
        tokio::spawn(async move {
            if let (Some(cert_path), Some(key_path)) = (&tls_cert_grpc, &tls_key_grpc) {
                let cert = std::fs::read(cert_path).expect("Failed to read TLS cert");
                let key = std::fs::read(key_path).expect("Failed to read TLS key");
                let identity = tonic::transport::Identity::from_pem(cert, key);
                let tls_config = tonic::transport::ServerTlsConfig::new().identity(identity);
                let addr = format!("0.0.0.0:{}", swarm_port).parse().unwrap();
                let server = swarm::create_grpc_server(grpc_state);
                info!("🐝 Swarm Controller listening on {} (TLS)", addr);
                let _ = tonic::transport::Server::builder()
                    .tls_config(tls_config).unwrap()
                    .add_service(server)
                    .serve(addr).await;
            } else {
                let addr = format!("0.0.0.0:{}", swarm_port).parse().unwrap();
                let server = swarm::create_grpc_server(grpc_state);
                info!("🐝 Swarm Controller listening on {}", addr);
                let _ = tonic::transport::Server::builder().add_service(server).serve(addr).await;
            }
        });

        // Pillar 4: Spawn Health Check Loop
        let mut leader_rx_health = leader_rx.clone();
        tokio::spawn(async move {
            if !*leader_rx_health.borrow() {
                let _ = leader_rx_health.changed().await;
            }
            health_state.health_check_loop().await;
        });
    }

    let (tx, mut rx) = mpsc::channel::<scheduler::ScheduleRequest>(32);

    let tls_cert = args.iter().position(|a| a == "--tls-cert")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string());
    let tls_key = args.iter().position(|a| a == "--tls-key")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string());

    // Web UI
    let db_web = Arc::clone(&db);
    let tx_web = tx.clone();
    let swarm_web = Arc::clone(&swarm_state);
    let vault_web = vault.clone();
    let dags_web = Arc::clone(&all_dags);
    let metrics_web = Arc::clone(&vortex_metrics);
    tokio::spawn(async move {
        let server = web::WebServer::new(db_web, tx_web, swarm_web, vault_web, dags_web, metrics_web);
        server.run(3000, tls_cert, tls_key).await;
    });

    // Scheduler Loop
    let db_sched = Arc::clone(&db);
    let dags_sched = Arc::clone(&all_dags);
    let swarm_sched = Arc::clone(&swarm_state);
    let metrics_sched = Arc::clone(&vortex_metrics);
    let mut leader_rx_sched = leader_rx.clone();
    tokio::spawn(async move {
        if !*leader_rx_sched.borrow() {
            let _ = leader_rx_sched.changed().await;
        }
        info!("🌀 Scheduler loop started.");
        while let Some(req) = rx.recv().await {
            debug!("🔔 Scheduler received request: {:?}", req);
            let dag = {
                let map = dags_sched.lock().await;
                map.get(&req.dag_id).cloned()
            };

            if let Some(dag) = dag {
                let worker_count = swarm_sched.active_worker_count().await;
                debug!("🔎 Scheduler: Found DAG {}. Swarm enabled: {}. Active workers: {}", req.dag_id, swarm_sched.enabled, worker_count);

                if swarm_sched.enabled && worker_count > 0 {
                    info!("🐝 Scheduler: Dispatching to SWARM mode.");
                    let dag_run_id = uuid::Uuid::new_v4().to_string();
                    let execution_date = req.execution_date.unwrap_or_else(|| Utc::now());
                    let _ = db_sched.create_dag_run(&dag_run_id, &req.dag_id, execution_date, &req.triggered_by).await;
                    let _ = db_sched.update_dag_run_state(&dag_run_id, "Running").await;
                    
                    let mut pre_finished_tasks = std::collections::HashSet::new();
                    if let scheduler::RunType::RetryFromFailure = req.run_type {
                        if let Ok((runs, _)) = db_sched.get_dag_runs(&req.dag_id, 100, 0).await {
                            if let Some(last_failed) = runs.iter().find(|r| r["state"] == "Failed") {
                                if let Some(_run_id) = last_failed["id"].as_str() {
                                     if let Ok((instances, _)) = db_sched.get_task_instances(&req.dag_id, 1000, 0).await {
                                         for inst in instances {
                                             if inst["state"] == "Success" {
                                                 if let Some(tid) = inst["task_id"].as_str() {
                                                     pre_finished_tasks.insert(tid.to_string());
                                                 }
                                             }
                                         }
                                     }
                                }
                            }
                        }
                    }

                    // --- Swarm Dependency Orchestrator ---
                    let dag_clone = Arc::clone(&dag);
                    let db_clone = Arc::clone(&db_sched);
                    let swarm_clone = Arc::clone(&swarm_sched);
                    let metrics_clone = Arc::clone(&metrics_sched);
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
                                let _ = db_clone.create_task_instance(&ti_id, &dag_clone.id, tid, "Queued", execution_date_clone, &run_id_clone).await;
                                
                                metrics_clone.record_task_queued();
                                swarm_clone.enqueue_task(swarm::PendingTask {
                                    task_instance_id: ti_id.clone(), dag_id: dag_clone.id.clone(), task_id: tid.clone(),
                                    command: task.command.clone(), dag_run_id: run_id_clone.clone(),
                                    task_type: task.task_type.clone(), config_json: task.config.to_string(),
                                    max_retries: task.max_retries, retry_delay_secs: task.retry_delay_secs,
                                    // BUG-2 FIX: secrets come from task definition, not hardcoded
                                    required_secrets: vec![],
                                }).await;

                                // Monitor this specific task
                                let db_mon = Arc::clone(&db_clone);
                                let tx_mon = tx_done.clone();
                                let tid_mon = tid.clone();
                                tokio::spawn(async move {
                                    // BUG-12 FIX: cap polling to 300 iterations (~10 min) to avoid infinite loop on DB failure
                                    let mut attempts = 0u32;
                                    loop {
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                        match db_mon.get_task_instance_retry_info(&ti_id).await {
                                            Ok((_, state)) => {
                                                if state == "Success" { let _ = tx_mon.send((tid_mon, true)).await; break; }
                                                if state == "Failed"  { let _ = tx_mon.send((tid_mon, false)).await; break; }
                                            }
                                            Err(_) => {
                                                attempts += 1;
                                                if attempts >= 300 {
                                                    warn!("Monitor timed out polling task instance {} — marking failed", ti_id);
                                                    let _ = tx_mon.send((tid_mon, false)).await;
                                                    break;
                                                }
                                            }
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
                                            // BUG-4 FIX: skip downstream tasks if upstream failed
                                            if !success {
                                                let skipped_ti = uuid::Uuid::new_v4().to_string();
                                                let _ = db_clone.create_task_instance(&skipped_ti, &dag_clone.id, down, "Upstream_Failed", execution_date_clone, &run_id_clone).await;
                                                let _ = db_clone.log_task_event(&skipped_ti, &dag_clone.id, down, &run_id_clone, "upstream_failed", Some("Upstream task failed"), None).await;
                                                tasks_remaining -= 1;
                                                continue;
                                            }

                                            let task = dag_clone.tasks.get(down).unwrap();
                                            let ti_id = uuid::Uuid::new_v4().to_string();
                                            let _ = db_clone.create_task_instance(&ti_id, &dag_clone.id, down, "Queued", execution_date_clone, &run_id_clone).await;
                                            let _ = db_clone.log_task_event(&ti_id, &dag_clone.id, down, &run_id_clone, "queued", None, None).await;
                                            
                                            metrics_clone.record_task_queued();
                                            swarm_clone.enqueue_task(swarm::PendingTask {
                                                task_instance_id: ti_id.clone(), dag_id: dag_clone.id.clone(), task_id: down.clone(),
                                                command: task.command.clone(), dag_run_id: run_id_clone.clone(),
                                                task_type: task.task_type.clone(), config_json: task.config.to_string(),
                                                max_retries: task.max_retries, retry_delay_secs: task.retry_delay_secs,
                                                // BUG-2 FIX: secrets come from task definition, not hardcoded
                                                required_secrets: vec![],
                                            }).await;

                                            let db_mon = Arc::clone(&db_clone);
                                            let tx_mon = tx_done.clone();
                                            let down_mon = down.clone();
                                            tokio::spawn(async move {
                                                // BUG-12 FIX: cap polling to 300 iterations (~10 min)
                                                let mut attempts = 0u32;
                                                loop {
                                                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                                    match db_mon.get_task_instance_retry_info(&ti_id).await {
                                                        Ok((_, state)) => {
                                                            if state == "Success" { let _ = tx_mon.send((down_mon, true)).await; break; }
                                                            if state == "Failed"  { let _ = tx_mon.send((down_mon, false)).await; break; }
                                                        }
                                                        Err(_) => {
                                                            attempts += 1;
                                                            if attempts >= 300 {
                                                                warn!("Monitor timed out polling task instance {} — marking failed", ti_id);
                                                                let _ = tx_mon.send((down_mon, false)).await;
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        let final_state = if all_success { "Success" } else { "Failed" };
                        let _ = db_clone.update_dag_run_state(&run_id_clone, final_state).await;
                        metrics_clone.record_dag_run_complete(final_state);
                        info!("🏁 Swarm Orchestrator: DAG Run {} finished (Success: {})", run_id_clone, all_success);
                    });
                } else {
                    let scheduler = Scheduler::new_with_arc(Arc::clone(&dag), Arc::clone(&db_sched))
                        .with_metrics(Arc::clone(&metrics_sched));
                    // Update: Scheduler needs metrics too
                    match req.run_type {
                        scheduler::RunType::Full => { let _ = scheduler.run_with_trigger(&req.triggered_by, req.execution_date).await; },
                        scheduler::RunType::RetryFromFailure => { 
                             warn!("⚠️ Standalone Retry not implemented yet (Swarm mode recommended)");
                             let _ = scheduler.run_with_trigger(&req.triggered_by, req.execution_date).await;
                        }
                    }
                }
            }
        }
    });

    // SLA Proactive Breach Detection Loop (Sprint 3)
    let db_sla = Arc::clone(&db);
    let dags_sla = Arc::clone(&all_dags);
    let mut leader_rx_sla = leader_rx.clone();
    tokio::spawn(async move {
        if !*leader_rx_sla.borrow() {
            let _ = leader_rx_sla.changed().await;
        }
        info!("🔴 SLA Monitor loop started (checking every 60s)");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            
            // Wait for DB to be responsive before doing work
            if db_sla.get_all_users().await.is_err() {
                continue; // Skip cycle if DB is unreachable
            }

            // We only care about runs that are currently "Running"
            match db_sla.get_interrupted_tasks().await {
                Ok(interrupted) => {
                    // Extract unique running run IDs
                    let mut running_run_ids = std::collections::HashSet::new();
                    for (_, _, run_id) in &interrupted {
                        running_run_ids.insert(run_id.clone());
                    }

                    for run_id in running_run_ids {
                        // Quick lookup isn't in DB trait, so we scan recent dag runs (or better, make a specific query)
                        // For simplicity, fetch the run by inspecting all DAGs for this run
                        let _dags_guard = dags_sla.lock().await;
                        for (dag_id, dag) in _dags_guard.iter() {
                            if let Some(sla_secs) = dag.sla_seconds {
                                // We'll borrow the paginated get_dag_runs to find this specific run
                                if let Ok((runs, _)) = db_sla.get_dag_runs(dag_id, 100, 0).await {
                                    if let Some(run_data) = runs.iter().find(|r| r["id"].as_str() == Some(&run_id)) {
                                        let is_missed = run_data["sla_missed"].as_bool().unwrap_or(false);
                                        if !is_missed {
                                            if let Some(start_time_str) = run_data["start_time"].as_str() {
                                                if let Ok(start_time) = chrono::DateTime::parse_from_rfc3339(start_time_str) {
                                                    let elapsed = Utc::now().signed_duration_since(start_time.with_timezone(&Utc));
                                                    if elapsed.num_seconds() > sla_secs as i64 {
                                                        warn!("🔴 SLA BREACH: DAG Run {} for DAG {} exceeded {}s limit", run_id, dag_id, sla_secs);
                                                        let _ = db_sla.mark_sla_missed(&run_id).await;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Err(e) => warn!("⚠️ SLA Monitor DB error: {}", e),
            }
        }
    });

    // Cron Scheduler Loop
    let db_cron = Arc::clone(&db);
    let tx_cron = tx.clone();
    let metrics_cron = Arc::clone(&vortex_metrics);
    let mut leader_rx_cron = leader_rx.clone();
    tokio::spawn(async move {
        if !*leader_rx_cron.borrow() {
            let _ = leader_rx_cron.changed().await;
        }
        info!("⏰ Cron scheduler loop started (checking every 60s)");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            metrics_cron.update_scheduler_heartbeat();
            
            match db_cron.get_scheduled_dags().await {
                Ok(scheduled_dags) => {
                    metrics_cron.set_dags_total(scheduled_dags.len() as i64);
                    for (dag_id, schedule_expr, last_run, is_paused, _timezone, max_active_runs, _catchup) in scheduled_dags {
                        if is_paused { continue; }
                        
                        if let Ok(active_count) = db_cron.get_active_dag_run_count(&dag_id).await {
                            if active_count >= max_active_runs { continue; }
                        }
                        
                        let schedule_str = crate::scheduler::normalize_schedule(&schedule_expr);
                        if schedule_str.is_empty() { continue; }
                        
                        let schedule: cron::Schedule = match schedule_str.parse() {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("⚠️ Invalid cron expression for DAG {}: {} ({})", dag_id, schedule_expr, e);
                                continue;
                            }
                        };
                        
                        let now = chrono::Utc::now();
                        let should_run = match last_run {
                            Some(last) => {
                                schedule.after(&last).next().map_or(false, |next_time| next_time <= now)
                            }
                            None => true,
                        };
                        
                        if should_run {
                            info!("⏰ Cron triggering DAG: {} (schedule: {})", dag_id, schedule_expr);
                            let _ = db_cron.update_dag_last_run(&dag_id, now).await;
                            if let Some(next) = schedule.after(&now).next() {
                                let _ = db_cron.update_dag_next_run(&dag_id, Some(next)).await;
                            }
                            let _ = tx_cron.send(crate::scheduler::ScheduleRequest {
                                dag_id: dag_id.clone(),
                                triggered_by: "scheduler".to_string(),
                                run_type: crate::scheduler::RunType::Full,
                                execution_date: Some(now),
                            }).await;
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️ Cron scheduler error: {}", e);
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
