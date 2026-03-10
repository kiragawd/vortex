# Vortex Codebase Analysis Report

This document outlines the major architectural flaws, logical bugs, and security vulnerabilities identified during the comprehensive code review of the Vortex orchestration engine.

## 1. High Availability (HA) Split-Brain Vulnerability
**Severity**: **Critical**
**Location**: `src/main.rs` (Cron Scheduler, DAG Scheduler, Swarm Health Check, and SLA Monitor loops)
**Description**: The HA implementation uses a `tokio::sync::watch::channel` (`leader_rx`) to broadcast leadership status. The various background event loops in `main.rs` wait for `leader_rx` to become `true` exactly once at startup, and then enter infinite `loop { ... }` blocks. 
If the controller loses the HA leader lock during runtime (e.g., database connection blip, heartbeat timeout), the `leader_rx` channel is updated to `false`. However, the event loops **never check the channel again** or suspend their execution. This guarantees a split-brain scenario where multiple standby nodes take over and simultaneously dispatch duplicate cron jobs, swarm tasks, and health checks.

## 2. Process Starvation via `tokio::process::Command` Leakage
**Severity**: **Critical**
**Location**: `src/executor.rs` (`execute_bash` and `execute_python`)
**Description**: Task execution wraps `tokio::process::Command` OS spawns in a `tokio::time::timeout` wrapper. By default, Tokio's `Command` does not reap or kill the child process if the future is dropped. If a task exceeds its `execution_timeout`, the Tokio timeout future aborts, abandoning the underlying `sh` or `python3` process. These orphaned processes continue running indefinitely in the background, consuming CPU and memory until the worker runs out of system resources entirely. `cmd.kill_on_drop(true)` must be explicitly set.

## 3. Path Traversal in Web API (`upload_dag`)
**Severity**: **Critical**
**Location**: `src/web.rs` (`upload_dag`)
**Description**: The multipart form parser for DAG uploads writes the payload directly to a file using the client-provided `file_name` argument without sanitization:
```rust
let file_path = format!("{}/{}", dags_dir, file_name);
fs::write(&file_path, &data);
```
An attacker or malicious user can pass a `file_name` such as `../../../etc/cron.d/evil.py` or `.ssh/authorized_keys`, allowing arbitrary file overwrites and leading to remote code execution (RCE) on the controller node.

## 4. Swarm Result Drop & Indefinite Task Hanging
**Severity**: **High**
**Location**: `src/worker.rs` (`execute_task_remote`) and `src/swarm.rs`
**Description**: When a task completes on a remote Swarm worker, it attempts to report the result to the controller using `report_client.report_task_result(result).await`. If this gRPC call fails (e.g., transient network partition, controller restart), the worker logs an error, decrements its `active_tasks` counter, and moves on without retrying. The controller observes the decremented `active_tasks` via heartbeats, but the task instance state is left permanently as `Running`. The task is dropped into the ether and blocks downstream execution forever until manually intervened or shot down by the SLA monitor.

## 5. TOCTOU Pool Limit Bypass
**Severity**: **High**
**Location**: `src/pools.rs` (`PoolManager::acquire_slot`)
**Description**: The `PoolManager` implements a fast-path TOCTOU check to see if a pool is full (`occupied >= total_slots`). If it passes, it calls `self.db.acquire_pool_slot()`. The DB layer correctly uses a `SELECT FOR UPDATE` to avoid concurrency issues, returning `Ok(false)` if the pool actually filled up in the microsecond gap. However, the `PoolManager` blindly swallows this boolean using the `?` operator (which only bubbles `Err`s, not `Ok(false)`) and proceeds to return `Ok(true)` to the scheduler. 
This means if tasks race for the last DB slot, the DB prevents the race but the `PoolManager` lies to the scheduler, letting tasks bypass the concurrency pool limit completely.

## 6. DAG Run State Abandonment & Swarm Task Mangle
**Severity**: **High**
**Location**: `src/main.rs`, `src/db_postgres.rs` (`get_interrupted_tasks`)
**Description**: Vortex relies heavily on in-memory `tokio::spawn` loops (`rx` channels) to track the runtime execution flow of a DAG. 
1. If the controller process crashes, all active `dag_runs` are abandoned forever in the `Running` state because there is no DB-driven state machine to rehydrate their channels on restart.
2. At startup (and shutdown), `main.rs` gets all interrupted tasks (`SELECT ... WHERE state = 'Running'`) and marks them as `Failed`. In a Swarm environment, remote workers are perfectly healthy and actively executing these tasks. Marking them `Failed` on the controller corrupts their state, forcing the UI and scheduler out-of-sync with the physical workers.

## 7. Pool Slot Implosion via Cascading Deletions
**Severity**: **Medium**
**Location**: `src/pools.rs` (`delete_pool`), `src/db_postgres.rs`
**Description**: When a user commands `delete_pool`, the `PoolManager` logs a warning if `occupied > 0` but deletes the pool anyway. The PostgreSQL schema links `task_instances` to slots via `pool_slots` with `ON DELETE CASCADE`. Active tasks immediately lose their slots. If the pool is quickly recreated, the slot counts reset to 0, completely breaking concurrency limits and putting the tracker into an inconsistent state.

## 8. Zero DAG Run Concurrency Constraints
**Severity**: **Medium**
**Location**: `src/scheduler.rs` (`run_with_trigger`)
**Description**: In typical orchestration systems (like Airflow), DAG executions are constrained by a unique compound key `(dag_id, execution_date)`. In Vortex, `run_with_trigger` creates a new UUID for every invocation, regardless of the `execution_date`. Firing an API trigger 100 times concurrently for the identical timestamp spins up 100 parallel identical DAG runs, leading to mass duplication of work and potential database deadlocks.

## 9. Error Cloaking in `get_dag_tasks` Endpoint
**Severity**: **Low**
**Location**: `src/web.rs` (`get_dag_tasks`)
**Description**: The web API uses `state.db.get_dag_by_id(&id).await.unwrap_or_default()` when querying a DAG. If the database crashes or networking fails, `unwrap_or_default()` silently coerces the `Err` into `None`. This makes genuine internal database 500 errors surface to the end-user identically as a `404 DAG Not Found`, severely hampering debugging efforts.
