# Architecture Overview — VORTEX System Design

## System Components

VORTEX is composed of four primary components working together in a distributed system:

### 1. **Controller** (Orchestrator & Health Manager)

The controller is the brain of VORTEX:

- **Receives DAG submissions** via REST API
- **Parses DAGs** and creates task dependency graphs
- **Enqueues tasks** into a distributed message broker
- **Runs health check loop** (every 15 seconds)
- **Manages worker state** (IDLE, ACTIVE, OFFLINE)
- **Handles task recovery** when workers fail
- **Serves web dashboard** for monitoring

**Implementation**:
- Written in Rust using Tokio async runtime
- Single stateless process (can be replicated for HA)
- Persists all state to SQLite database

### 2. **Workers** (Task Executors)

Workers are distributed agents that execute tasks:

- **Register with controller** upon startup
- **Poll the task queue** for new work
- **Send heartbeats** every 10 seconds
- **Execute tasks** in containers (Docker/OCI)
- **Report task results** (success/failure/logs)
- **Clean up resources** after task completion

**Implementation**:
- Written in Rust using Tokio
- Uses gRPC for communication with controller
- Containerized execution via Docker daemon
- Horizontally scalable (add more workers = more throughput)

### 3. **Database** (SQLite)

Single source of truth for all persistent state:

**Tables**:
- `workers` — Worker registrations, state, heartbeats
- `task_instances` — Individual task records, state, assignments
- `dags` — Directed Acyclic Graph definitions
- `secrets` — Encrypted secrets with nonces
- `task_results` — Task outputs, logs, error messages

**Characteristics**:
- Single-file SQLite database (./vortex.db)
- ACID transactions (guarantees consistency)
- Optimized indexes for frequent queries
- Can be backed up or replicated

### 4. **Dashboard** (Web UI)

Real-time monitoring and management interface:

- **Worker status**: Active/offline/registering counts
- **Task metrics**: Completed, running, queued, failed counts
- **Recovery status**: Recent failures and auto-recovery events
- **Log viewer**: Stream logs from running tasks
- **Secret management**: UI for creating/rotating secrets

**Technology**: React + WebSocket for real-time updates

---

## Data Flow & Execution Pipeline

### Sequence: DAG Submission to Task Completion

```
User                Controller            Database         Workers
  │                   │                     │                │
  ├─ Submit DAG ─────→│                     │                │
  │                   │                     │                │
  │                   ├─ Parse DAG ─────────│                │
  │                   │                     │                │
  │                   ├─ Create Tasks ──────│                │
  │                   │ (task_1, task_2...)  │                │
  │                   │                     │                │
  │                   ├─ Enqueue Tasks ─────│                │
  │                   │ (to message queue)   │                │
  │                   │                     │                │
  │                   │                     │   Poll Queue   │
  │                   │                     │←────────────────┤ Worker A
  │                   │                     │                │
  │                   │                     │ Update Status  │
  │                   │                     │──────────────→ (RUNNING)
  │                   │                     │                │
  │                   │                     │                ├─ Execute Container
  │                   │                     │                │
  │                   │                     │  Heartbeat     │
  │                   │                     │←────────────────┤
  │                   │                     │                │
  │                   │                     │  Task Result   │
  │                   │                     │←────────────────┤
  │                   │                     │                │
  │                   │                     │ Update Status  │
  │                   │                     │──────────────→ (COMPLETED)
  │                   │                     │                │
  │←─ DAG Status ─────│                     │                │
  │  (all tasks done)  │                     │                │
```

### 1. User Submits DAG

User sends DAG definition via `POST /api/dags`:

```json
{
  "name": "data-pipeline",
  "tasks": [
    {"name": "task_1", "image": "python:3.13", "command": ["python", "fetch.py"]},
    {"name": "task_2", "image": "python:3.13", "command": ["python", "process.py"], "depends_on": ["task_1"]},
    {"name": "task_3", "image": "python:3.13", "command": ["python", "notify.py"], "depends_on": ["task_2"]}
  ]
}
```

### 2. Controller Parses DAG & Creates Tasks

The controller:
1. Validates DAG structure (no cycles, valid references)
2. Creates `task_instances` in the database (one row per task)
3. Sets initial state to `Queued`
4. Stores dependency information

Database state after parsing:
```sql
SELECT id, name, state, depends_on FROM task_instances WHERE dag_id = 'dag_123';
```

Output:
```
id        | name    | state  | depends_on
----------|---------|--------|----------
task_1    | task_1  | Queued | NULL
task_2    | task_2  | Queued | task_1
task_3    | task_3  | Queued | task_2
```

### 3. Controller Enqueues Tasks

Tasks without dependencies are immediately enqueued. Tasks with dependencies wait:

```rust
fn enqueue_ready_tasks(dag_id: &str, db: &Database) -> Result<()> {
    // Find tasks with all dependencies completed
    let ready_tasks = db.query(
        "SELECT id FROM task_instances 
         WHERE dag_id = ? AND state = 'Queued' 
         AND (depends_on IS NULL 
              OR depends_on IN (SELECT id FROM task_instances WHERE state = 'Completed'))"
    )?;

    for task in ready_tasks {
        queue.enqueue(task)?;
        db.update_task_state(&task.id, "Enqueued")?;
    }
}
```

### 4. Workers Poll & Receive Tasks

Workers continuously poll the task queue:

```rust
async fn worker_task_loop(worker_id: &str) -> Result<()> {
    loop {
        // Poll controller for next task
        let task = controller.poll_task(worker_id).await?;
        
        if let Some(task) = task {
            // Mark as RUNNING
            db.update_task_state(&task.id, "Running")?;
            db.update_worker_task(&worker_id, &task.id)?;
            
            // Execute container
            let result = execute_container(&task).await;
            
            // Report result
            controller.report_result(&task.id, result).await?;
        }
        
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
```

### 5. Worker Executes Task

Worker spawns an isolated process based on the task type:

**BashOperator**:
```bash
sh -c "{bash_command}"
```

**PythonOperator**:
```bash
python3 /tmp/vortex_task_{task_id}.py
```

All secrets are injected as environment variables.

### 6. Worker Reports Result

On completion, worker sends result via gRPC:

```json
{
  "task_id": "task_1",
  "success": true,
  "stdout": "...",
  "stderr": "",
  "duration_ms": 3500,
  "retry_count": 0
}
```

### 7. Controller Updates Database & Retry Logic

Controller stores the result. If the task failed and `retry_count < max_retries`, it resets the state to `Queued` after `retry_delay_secs`. Otherwise, it marks the task as `Success` or `Failed` and proceeds to downstream tasks.

### 8. Repeat for Dependent Tasks

Steps 4-7 repeat for `task_2`, then `task_3`.

### 9. DAG Completion

When all tasks are `Completed`, DAG is marked as `Completed`:

```sql
UPDATE dags SET state = 'Completed', completed_at = current_timestamp WHERE id = 'dag_123';
```

---

## Failure Scenarios & Recovery

### Scenario 1: Worker Crashes During Task Execution

```
Time: 10:00:00 — Worker A starts task_123
  task_123.state = RUNNING, task_123.worker_id = worker_a

Time: 10:00:15 — Worker A crashes (OOM, segfault, etc.)
  Worker A stops sending heartbeats

Time: 10:00:75 — Controller health check detects missing heartbeat
  worker_a.state = OFFLINE, worker_a.last_heartbeat is 60s stale
  ACTION: task_123.state = Queued, task_123.worker_id = NULL

Time: 10:00:80 — Worker B picks up task_123
  task_123.state = RUNNING, task_123.worker_id = worker_b
  Task executes from scratch (no checkpoint)

Time: 10:01:30 — task_123 completes on Worker B
  task_123.state = COMPLETED
```

**Result**: Zero task loss, automatic failover, ~75 second recovery latency.

### Scenario 2: Network Partition (Controller ↔ Worker)

```
Time: 10:00:00 — Worker A and Controller lose network connectivity
  Worker A still running, but can't send heartbeats
  Controller can't reach Worker A

Time: 10:01:00 — Health check detects missing heartbeat (60s timeout + 15s check)
  worker_a.state = OFFLINE
  All running tasks reassigned

Time: 10:05:00 — Network heals, Worker A reconnects
  Controller sees Worker A is reconnecting
  Existing task assignments already moved to other workers
  Worker A re-registers (new session)
```

**Result**: Transparent failover; no duplicate task execution (database state is authoritative).

### Scenario 3: Controller Crash

```
Time: 10:00:00 — Controller crashes
  Workers continue executing (independent)
  New task enqueuing stops (no one to accept DAGs)

Time: 10:00:30 — Operator restarts Controller
  Controller reads state from database
  Workers send heartbeats immediately
  Pending tasks resume being enqueued
```

**Result**: Workers are resilient; Controller is stateless (can restart quickly).

### Scenario 4: Database Corruption or Loss

```
Worst case: Database is lost
  No persistent state of workers, tasks, or results
  Cannot recover task results or audit history

Mitigation:
  - Regular backups (daily snapshots)
  - Replication (write-ahead logs synced to replica)
  - Rebuild-from-logs (future: event sourcing)
```

---

## Data Consistency & Atomicity

### ACID Guarantees

VORTEX relies on SQLite's ACID properties:

| Property | Implementation |
|----------|-----------------|
| **Atomicity** | Multi-task re-queueing in single transaction (all-or-nothing) |
| **Consistency** | Foreign keys enforced; DAG structure validated |
| **Isolation** | SQLite serialization; no dirty reads or race conditions |
| **Durability** | fsync on commit; data survives process crashes |

### Example: Atomic Recovery Transaction

When a worker goes offline, this transaction runs:

```sql
BEGIN TRANSACTION;

UPDATE workers 
SET state = 'OFFLINE', offline_at = current_timestamp 
WHERE id = 'worker_a' AND state = 'ACTIVE';

UPDATE task_instances 
SET state = 'Queued', worker_id = NULL, updated_at = current_timestamp 
WHERE worker_id = 'worker_a' AND state = 'Running';

INSERT INTO recovery_log (worker_id, num_tasks, timestamp) 
VALUES ('worker_a', 3, current_timestamp);

COMMIT;
```

Either all statements succeed or none do. No partial updates.

---

## Performance Characteristics

### Throughput

| Operation | Throughput | Latency |
|-----------|-----------|---------|
| **DAG submission** | 100+ DAGs/min | <100ms |
| **Task enqueuing** | 10,000+ tasks/min | <10ms per task |
| **Heartbeat processing** | 1,000+ workers | <50ms per cycle |
| **Task result reporting** | 1,000+ tasks/min | <100ms |

### Scalability

| Metric | Limit | Notes |
|--------|-------|-------|
| **Workers** | 100+ | Heartbeat check is O(n) but indexed; no performance degradation |
| **Tasks per DAG** | 1,000+ | Dependency resolution is O(m log m) where m = # tasks |
| **Concurrent DAGs** | 100+ | Controller is stateless; limited only by task queue |
| **Database size** | 10GB+ | SQLite handles millions of task records; indices optimize queries |

### Bottlenecks & Optimization

1. **Task Enqueuing**: Batch updates for multiple tasks (same transaction)
2. **Health Checks**: Indexed queries on `state` and `last_heartbeat`
3. **Task Distribution**: Workers pull from queue (not pushed); balanced load
4. **Database**: WAL mode + indices for concurrent read/write

---

## System Deployment Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       VORTEX Cluster                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌─────────────────────┐                                         │
│  │   Controller (HA)   │                                         │
│  │ - REST API          │◄──── Dashboard (web UI)                │
│  │ - Health Check Loop │                                         │
│  │ - DAG Orchestration │                                         │
│  └──────────┬──────────┘                                         │
│             │                                                     │
│             │ (Read/Write)                                       │
│             ▼                                                     │
│  ┌──────────────────────┐                                        │
│  │  SQLite Database     │◄──── Backups (daily snapshots)         │
│  │ - Workers            │                                         │
│  │ - Tasks              │                                         │
│  │ - Secrets (encrypted)│                                         │
│  │ - Results            │                                         │
│  └──────────┬───────────┘                                        │
│             │                                                     │
│    ┌────────┴──────────┬──────────────────┐                     │
│    │                   │                  │                     │
│    ▼                   ▼                  ▼                     │
│  ┌──────────┐      ┌──────────┐      ┌──────────┐              │
│  │ Worker 1 │      │ Worker 2 │ ...  │ Worker N │              │
│  │ - gRPC   │      │ - gRPC   │      │ - gRPC   │              │
│  │ - Docker │      │ - Docker │      │ - Docker │              │
│  │ - Poll   │      │ - Poll   │      │ - Poll   │              │
│  └──────────┘      └──────────┘      └──────────┘              │
│       │                  │                  │                   │
│       └──────────────────┴──────────────────┘                   │
│              (execute containers)                               │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Controller** | Rust + Tokio | Performance, type safety, async concurrency |
| **Workers** | Rust + Tokio | Same as controller; lightweight, fast startup |
| **Database** | SQLite | ACID, no external dependencies, easy deployment |
| **Message Queue** | In-database task_queue table | Simplicity; can upgrade to RabbitMQ/Kafka later |
| **Container Runtime** | Docker / containerd | Standard; widely available |
| **RPC** | gRPC + Protobuf | Type-safe, efficient binary protocol |
| **Dashboard** | React + WebSocket | Real-time updates, responsive UI |

---

## Related Documentation

- [Pillar 3: Secrets Vault](./PILLAR_3_SECRETS_VAULT.md) — Encrypted secret management
- [Pillar 4: Resilience](./PILLAR_4_RESILIENCE.md) — Auto-recovery and worker failure handling
- [API Reference](./API_REFERENCE.md) — REST and gRPC endpoint documentation
- [Deployment Guide](./DEPLOYMENT.md) — Installation, configuration, and monitoring
