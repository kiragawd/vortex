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

**Implementation:** Rust + Tokio async runtime. All state persisted to PostgreSQL via a unified `DatabaseBackend` trait (`Arc<dyn DatabaseBackend>`).

### 2. Workers (Task Executors)

Distributed worker processes that connect to the controller via gRPC:

- **Register** with controller on startup (hostname, capacity, labels)
- **Poll** for tasks based on available capacity
- **Execute tasks** directly via `sh -c` (bash), `python3` (python), or custom **Plugins** (HTTP, etc.)
- **Send heartbeats** every 15 seconds
- **Report results** (stdout, stderr, duration, success/failure) back to controller
- **Secrets injection** — decrypted secrets are passed as environment variables

**Implementation:** Same Rust binary, different CLI subcommand (`vortex worker`).

### 3. Database (PostgreSQL)

VORTEX uses PostgreSQL as its primary (and only production) database, accessed through a unified trait abstraction (`Arc<dyn DatabaseBackend>`).

| Table | Purpose |
|-------|---------|
| `dags` | DAG definitions, schedule, team assignment, pause state |
| `tasks` | Task definitions (command, type, config, group, timeout, retry) |
| `task_instances` | Execution records (state, logs, duration, worker_id, run_id) |
| `dag_runs` | Run records with state, triggered_by, and timestamps |
| `dag_versions` | Snapshots linking DAGs to source files for rollbacks |
| `audit_log` | Permanent trail of security and operational events |
| `workers` | Worker registrations (hostname, capacity, heartbeat, state) |
| `users` | RBAC accounts with API keys and team IDs |
| `teams` | Multi-tenancy isolation with resource quotas |
| `secrets` | AES-256-GCM encrypted key-value pairs |
| `task_xcom` | Cross-task communication key-value store |
| `pools` | Concurrency-limiting resource pools |
| `pool_slots` | Active slot allocations for pools |
| `dag_callbacks` | Per-DAG webhook/notification configuration |

### 4. Web Dashboard

Enterprise single-page application embedded in the binary via `rust-embed`:

- **Technology:** React 18 + TypeScript + Vite 5 + Tailwind CSS 3.4
- **State Management:** Zustand for global state, TanStack React Query for server state
- **Features:** Dark/light theme toggle, DAG management, run monitoring, compliance dashboard, RBAC management, monitoring, settings
- **SPA Routing:** React Router v6 with server-side fallback (serves `index.html` for all non-API, non-file paths)
- **Auth:** Login form → API token stored in Zustand/localStorage
- **Pages:** Dashboard, DAGs, DAG Detail, Runs, Compliance, RBAC, Monitoring, Settings
- **RBAC:** Admin sees all DAGs; Operator/Viewer with a `team_id` sees only their team's DAGs; Operator/Viewer with no team sees only unassigned DAGs (Bug #14 fix)
- **Auto-refresh:** 5-second polling for DAG status and Swarm health
### 5. Enterprise Connector Subsystem

A unified abstraction for external data systems, defined in `src/enterprise_connector.rs` with implementations in `src/connectors.rs`:

- **Connector Trait (`EnterpriseConnector`)** — Async interface for connect, health check, execute, stream, introspect, and close operations
- **Connector Registry** — Dynamic name-based registration and lookup (`ConnectorRegistry`)
- **Capability System** — Each connector declares its capabilities (Transactions, BatchRead, StreamingRead, AsyncJobs, ArrowZeroCopy, PushdownPredicates)
- **Connector Kinds** — `Database`, `Warehouse`, `Api`, `Transformation`

**Implemented connectors:**

| Connector | Module | Driver | Key Features |
|-----------|--------|--------|-------------|
| PostgreSQL | `PostgresEnterpriseConnector` | `sqlx::PgPool` | Connection pooling, streaming fetch, query instrumentation |
| Snowflake | `SnowflakeConnector` | REST API | Key-pair/OAuth auth, async query polling, Arrow result format |
| Databricks | `DatabricksConnector` | REST API | SQL Warehouse mode + Jobs API mode, async polling |
| MySQL | `MySqlConnector` | `sqlx` MySQL | Async queries, type normalization |
| MS SQL | `MsSqlConnector` | `tiberius` TDS | Async queries, type normalization |
| dbt | `DbtConnector` | CLI shell | Runs `dbt compile/run/test`, captures JSON logs, secret redaction |

**Cross-cutting:** All connectors share a retry policy (`with_retry`) with configurable backoff, timeout, and auth context (`ConnectorContext`).

### 6. Migration Pipeline

Airflow-to-Vortex transpilation system spanning three modules:

- **Static AST Parser** (`src/airflow_ast_parser.rs`) — Parses Python DAG files into an intermediate representation (IR) without executing Python. Extracts DAG definitions, operator instantiations, dependency expressions (`>>`, `set_upstream`), and schedule metadata. Validates unique task IDs, edge references, and detects cycles.
- **DAG Code Generator** (`src/dag_codegen.rs`) — Transforms AST IR into native Rust DAG modules. Emits `todo!()` for unsupported `PythonOperator` logic with fallback shim payloads. Produces migration reports (converted tasks, placeholder tasks, required manual actions).
- **CLI Migrate Command** (`src/bin/vortex-cli.rs`) — `vortex-cli migrate <path>` drives the full pipeline: discover → parse → generate → validate → report. Supports `--strict`, `--report-format json|md`, `--output-dir`, and `--use-shim-fallback` flags.

### 7. Agentic Migration Layer

AI-assisted conversion for unresolved Python and dbt logic, implemented in `src/agentic.rs`:

- **LLM Provider Abstraction** — `LlmProvider` trait with OpenAI and Anthropic implementations. Provider-agnostic prompt templates, policy checks, and token/cost telemetry.
- **Python-to-Rust Agent** — Iterative loop: analyze Python callable → plan Rust equivalent → generate code → `cargo check` + lint policy validation → repair loop until passing or retry budget exhausted.
- **dbt-to-Rust Agent** — Loads dbt manifest, expands Jinja SQL with deterministic context, builds dependency graph of SQL transformations, maps nodes to connector execution stages, and generates a Rust orchestration module.
- **Safety** — Blocks dangerous APIs by policy, forces explicit error handling, validates all generated code compiles before acceptance.
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

> **Concurrency note:** The in-degree map used for dependency orchestration is protected by a `tokio::sync::Mutex` (not `std::sync::Mutex`). This is intentional — the `Mutex` guard is held briefly across `.await` points when decrementing in-degrees, and using a sync mutex here would block the Tokio worker thread. All scheduler lock acquisitions use `.lock().await`.

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
- PostgreSQL state is fully recovered
- `Running` tasks with no heartbeat are marked `Failed`
- Workers re-register on next heartbeat

### Task Failure with Retries

```
T+0s    Task fails (exit code != 0)
T+0s    Controller checks attempt < max_retries (local counter inside execute_task)
T+Ns    After retry_delay_secs, task re-executes using the same task_instance ID (state resets Queued→Running)
        retry_count column incremented in DB on each attempt
        Repeat until success or max_retries exhausted
T+end   Final state (Success or Failed) reported via channel; tx.send fires once
```

> **Note:** Retries are a tight in-process loop inside `execute_task`. The same `ti_id` UUID is reused across all retry attempts so the DB always shows a single task instance per task per DAG run.

---

## Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| **Runtime** | Rust + Tokio | Async concurrency, memory safety, no GC |
| **Database** | PostgreSQL (SQLx) | ACID, production-grade, advisory locks for HA |
| **Web API** | Axum | Lightweight, tower middleware, async |
| **gRPC** | Tonic + Prost | Type-safe Protobuf, streaming |
| **Python Bridge** | PyO3 | Native CPython embedding (Requires trusted DAG files; AST sandboxing planned) |
| **Encryption** | AES-256-GCM (aes-gcm) | NIST-approved, authenticated encryption |
| **Enterprise Connectors** | sqlx, tiberius, reqwest | Unified trait with Postgres, Snowflake, Databricks, MySQL, MSSQL, dbt |
| **Migration Pipeline** | rustpython-parser, codegen | Static AST parsing, Rust code generation |
| **Agentic Layer** | OpenAI / Anthropic APIs | LLM-assisted Python-to-Rust and dbt-to-Rust conversion |
| **Dashboard** | Vanilla JS + Tailwind + D3 + Dagre | No build step, embedded via rust-embed |
| **Task Execution** | Direct process spawn | `sh -c` for bash, `python3` for python |

---

## Related Documentation

- [API Reference](./API_REFERENCE.md) — Complete REST API documentation
- [CLI Reference](./CLI_REFERENCE.md) — CLI command reference
- [Deployment Guide](./DEPLOYMENT.md) — Build, configure, and run
- [Python Integration](./PYTHON_INTEGRATION.md) — DAG authoring
- [Secrets Vault](./SECRETS_VAULT.md) — Encrypted secrets
- [Resilience](./RESILIENCE.md) — Auto-recovery
- [High Availability](./high-availability.md) — HA deployment with leader election
- [Migration Guide](./MIGRATION_GUIDE.md) — Airflow-to-Vortex DAG migration
- [Connector API](./CONNECTOR_API.md) — Enterprise connector trait and implementations
