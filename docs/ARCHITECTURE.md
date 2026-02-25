# Architecture Overview — VORTEX System Design

## System Components

VORTEX is a single-binary orchestration engine with four logical components:

### 1. Controller (Orchestrator)

The main process that runs the scheduler, API server, and Swarm coordinator:

- **Parses DAGs** from Python files via PyO3 runtime and regex parser
- **Schedules tasks** using topological sort with dependency-aware fan-out
- **Serves REST API** (Axum on port 3000) for DAG management and the web dashboard
- **Runs gRPC Swarm controller** (Tonic on port 50051) for distributed workers
- **Health check loop** (every 15 seconds) — detects stale workers, requeues tasks
- **Recovery on startup** — marks interrupted `Running` tasks as `Failed`

**Implementation:** Rust + Tokio async runtime. All state persisted to SQLite.

### 2. Workers (Task Executors)

Distributed worker processes that connect to the controller via gRPC:

- **Register** with controller on startup (hostname, capacity, labels)
- **Poll** for tasks based on available capacity
- **Execute tasks** directly via `sh -c` (bash) or `python3` (python) — no Docker required
- **Send heartbeats** every 15 seconds
- **Report results** (stdout, stderr, duration, success/failure) back to controller
- **Secrets injection** — decrypted secrets are passed as environment variables

**Implementation:** Same Rust binary, different CLI subcommand (`vortex worker`).

### 3. Database (SQLite)

Single-file database (`vortex.db`) as the source of truth:

| Table | Purpose |
|-------|---------|
| `dags` | DAG definitions (id, schedule, pause state, timezone, max_active_runs) |
| `tasks` | Task definitions (command, type, config, retry settings) |
| `task_instances` | Execution records (state, stdout, stderr, duration, worker_id, run_id) |
| `dag_runs` | Run records (state, execution_date, triggered_by) |
| `dag_versions` | Version tracking linking DAGs to filesystem paths |
| `workers` | Worker registrations (hostname, capacity, heartbeat, state) |
| `users` | RBAC accounts (username, password_hash, role, api_key) |
| `secrets` | AES-256-GCM encrypted key-value pairs |

### 4. Web Dashboard

Single-page application embedded in the binary via `rust-embed`:

- **Technology:** Vanilla JavaScript + Tailwind CSS (CDN) + D3.js + Dagre-D3
- **Features:** Visual DAG graphs, run history, code editor, secret management, user management
- **Auth:** Login form → API key stored in `localStorage`
- **RBAC:** Admin sees all; Operator sees DAGs + triggers; Viewer is read-only
- **Auto-refresh:** 5-second polling for DAG status and Swarm health

---

## Execution Flow

### DAG Trigger → Task Completion

```
User (Web UI / API)        Controller             Workers
       │                      │                      │
       ├─ POST /trigger ──────│                      │
       │                      │                      │
       │                      ├─ Create dag_run      │
       │                      ├─ Topo-sort tasks     │
       │                      ├─ Enqueue root tasks  │
       │                      │   (in-degree = 0)    │
       │                      │                      │
       │                      │    poll_task (gRPC)   │
       │                      │◄─────────────────────┤
       │                      ├─ Assign tasks ───────│
       │                      │                      │
       │                      │                      ├─ sh -c "echo ..."
       │                      │                      │
       │                      │  report_result (gRPC) │
       │                      │◄─────────────────────┤
       │                      │                      │
       │                      ├─ Update DB state     │
       │                      ├─ Check downstream    │
       │                      ├─ Enqueue next tasks  │
       │                      │   (in-degree → 0)    │
       │                      │                      │
       │                      │         ... repeat ...│
       │                      │                      │
       │                      ├─ All done → dag_run  │
       │                      │   state = Success    │
       │◄─ Poll refresh ──────│                      │
```

### Dependency Orchestration (Swarm Mode)

When `--swarm` is enabled and workers are connected:

1. Controller creates a `dag_run` record
2. Builds in-degree map from DAG dependencies
3. Enqueues all tasks with in-degree 0
4. Spawns monitor tasks that poll DB for completion
5. When a task completes, decrements downstream in-degrees
6. Tasks reaching in-degree 0 are enqueued
7. Continues until all tasks finish
8. Updates `dag_run` state to `Success` or `Failed`

### Standalone Mode (No Workers)

When `--swarm` is not enabled or no workers are connected, the controller uses the built-in `Scheduler` which executes tasks locally using Tokio spawn with the `TaskExecutor`.

---

## Failure Scenarios

### Worker Crash

```
T+0s    Worker A starts task_123 (state=Running, worker_id=A)
T+60s   Worker A stops heartbeating
T+75s   Health check detects stale heartbeat (60s timeout + 15s check)
        → worker state = Offline
        → task_123 state = Queued, worker_id = NULL
T+77s   Worker B picks up task_123 via poll
        → Executes from scratch
T+80s   Task completes successfully
```

**Recovery latency:** ~75 seconds worst case.

### Controller Crash

Workers continue executing current tasks independently. On controller restart:
- SQLite state is fully recovered
- `Running` tasks with no heartbeat are marked `Failed`
- Workers re-register on next heartbeat

### Task Failure with Retries

```
T+0s    Task fails (exit code != 0)
T+0s    Controller checks retry_count < max_retries
T+Ns    After retry_delay_secs, task re-enqueued (state=Queued)
T+Ns    Worker picks up and re-executes
        Repeat until success or max_retries exhausted
```

---

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Runtime** | Rust + Tokio | Async concurrency, memory safety, no GC |
| **Database** | SQLite (rusqlite) | Zero-config, ACID, single-file |
| **Web API** | Axum | Lightweight, tower middleware, async |
| **gRPC** | Tonic + Prost | Type-safe Protobuf, streaming |
| **Python Bridge** | PyO3 | Native CPython embedding |
| **Encryption** | AES-256-GCM (aes-gcm) | NIST-approved, authenticated encryption |
| **Dashboard** | Vanilla JS + Tailwind + D3 + Dagre | No build step, embedded via rust-embed |
| **Task Execution** | Direct process spawn | `sh -c` for bash, `python3` for python |

---

## Related Documentation

- [API Reference](./API_REFERENCE.md) — Complete REST API documentation
- [Deployment Guide](./DEPLOYMENT.md) — Build, configure, and run
- [Python Integration](./PHASE_2_PYTHON_INTEGRATION.md) — DAG authoring
- [Secrets Vault](./PILLAR_3_SECRETS_VAULT.md) — Encrypted secrets
- [Resilience](./PILLAR_4_RESILIENCE.md) — Auto-recovery
