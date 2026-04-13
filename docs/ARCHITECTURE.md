# Architecture Overview — RYUO System Design

Ryuo replaces Python's heavy, process-based, GIL-bound orchestration architecture with Rust's high-performance, async-first paradigm.

### Key Architectural Advantages

1. **Concurrency Model (Tokio vs Python processes):** Airflow spawns heavyweight OS processes for parallel scheduling, creating heavy PostgreSQL lock contention. Ryuo uses Rust's `tokio` async runtime with lightweight async tasks (~2KB memory footprint) that yield instead of blocking, enabling orders of magnitude more parallel task executions per core.

2. **Native Connectors & Automated Conversion:** An AST-parsing and AI-agentic layer transpiles Airflow Python DAGs into native Rust code. Databricks, Snowflake, and PostgreSQL connectors execute directly in Rust with memory-efficient frameworks.

3. **Single Binary Deployment:** Ryuo compiles to a single ~15MB binary containing the web UI, REST API, scheduler, and worker executor — replacing Airflow's multi-process infrastructure (Webserver, Schedulers, Triggerer, Celery/Redis, Workers).

4. **Distributed gRPC Swarm:** Horizontal scaling via lightweight gRPC Swarm (Worker/Controller) architecture with graceful heartbeat management, node loss handling, and task requeuing.

5. **Built-in Security:** AES-256-GCM encrypted vault, team-based multi-tenancy, path traversal protection, and RBAC at the middleware level.

---

## System Components

RYUO is a single-binary orchestration engine with the following logical components:

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

**Implementation:** Same Rust binary, different CLI subcommand (`ryuo worker`).

### 3. Database (PostgreSQL)

RYUO uses PostgreSQL as its primary (and only production) database, accessed through a unified trait abstraction (`Arc<dyn DatabaseBackend>`).

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
| BigQuery | `BigQueryConnector` | REST API | OAuth token auth, query execution |
| Redshift | `RedshiftConnector` | `sqlx` PostgreSQL | Real SQL execution via PG wire protocol |
| MySQL | `MySqlConnector` | `sqlx` MySQL | Async queries, type normalization |
| MS SQL | `MsSqlConnector` | `tiberius` TDS | Async queries, type normalization |
| dbt | `DbtConnector` | CLI shell | Runs `dbt compile/run/test`, captures JSON logs, secret redaction |

**Cross-cutting:** All connectors share a retry policy (`with_retry`) with configurable backoff, timeout, and auth context (`ConnectorContext`).

### 6. Migration Pipeline

Airflow-to-Ryuo transpilation system spanning three modules:

- **Static AST Parser** (`src/airflow_ast_parser.rs`) — Parses Python DAG files into an intermediate representation (IR) without executing Python. Extracts DAG definitions, operator instantiations, dependency expressions (`>>`, `set_upstream`), and schedule metadata. Validates unique task IDs, edge references, and detects cycles.
- **DAG Code Generator** (`src/dag_codegen.rs`) — Transforms AST IR into native Rust DAG modules. Emits `todo!()` for unsupported `PythonOperator` logic with fallback shim payloads. Produces migration reports (converted tasks, placeholder tasks, required manual actions).
- **CLI Migrate Command** (`src/bin/ryuo-cli.rs`) — `ryuo-cli migrate <path>` drives the full pipeline: discover → parse → generate → validate → report. Supports `--strict`, `--report-format json|md`, `--output-dir`, and `--use-shim-fallback` flags.

### 7. Agentic Migration Layer

AI-assisted conversion for unresolved Python and dbt logic, implemented in `src/agentic.rs`:

- **LLM Provider Abstraction** — `LlmProvider` trait with OpenAI and Anthropic implementations. Provider-agnostic prompt templates, policy checks, and token/cost telemetry.
- **Python-to-Rust Agent** — Iterative loop: analyze Python callable → plan Rust equivalent → generate code → `cargo check` + lint policy validation → repair loop until passing or retry budget exhausted.
- **dbt-to-Rust Agent** — Loads dbt manifest, expands Jinja SQL with deterministic context, builds dependency graph of SQL transformations, maps nodes to connector execution stages, and generates a Rust orchestration module.
- **Safety** — Blocks dangerous APIs by policy, forces explicit error handling, validates all generated code compiles before acceptance.

### 8. Event-Driven Architecture & Sensors

Event bus and sensor framework for reactive orchestration, implemented in `src/event_framework.rs` and `src/sensors.rs`:

- **Event Bus** — Broadcast channel-based in-memory event log with source glob matching, JSON path conditions, and metadata filters
- **Webhook Receiver** — HTTP endpoint for ingesting external events into the event bus
- **Event-Triggered DAGs** — DAG execution triggered when incoming events match configured patterns
- **Sensor Framework** — Configurable sensors with poke (tight loop) and reschedule (release slot) modes:
  - **File Sensor** — Watch filesystem paths for existence or modification
  - **HTTP Sensor** — Poll HTTP endpoints, match response codes and body patterns
  - **SQL Sensor** — Execute queries against databases, evaluate row count or value conditions
  - **External Task Sensor** — Wait for upstream DAG/task completion across DAG boundaries

### 9. Configuration & Operations

Operational tooling implemented in `src/config_ops.rs` and `src/devops.rs`:

- **Config Manager** — Environment-scoped configuration (dev/staging/prod) with inheritance resolution
- **Feature Flags** — Boolean feature flag management with `RwLock`-protected in-memory store
- **Git-Sync** — DAG repository synchronization with SSH/HTTP/token auth and sync state tracking
- **Health Checks** — Operational health check types and maintenance window definitions
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
| **Database** | PostgreSQL (SQLx 0.7) | ACID, production-grade, advisory locks for HA |
| **Web API** | Axum 0.7 | Lightweight, tower middleware, async |
| **gRPC** | Tonic 0.12 + Prost 0.13 | Type-safe Protobuf, streaming |
| **CLI** | Clap 4.5 | Derive-based argument parsing |
| **Python Bridge** | PyO3 0.23 | Native CPython embedding (requires `--allow-unsafe-dag-exec` opt-in) |
| **Encryption** | AES-256-GCM (aes-gcm 0.10) | NIST-approved, authenticated encryption |
| **TLS** | Rustls 0.23 + axum-server 0.7 | TLS for REST API, rustls-based |
| **Enterprise Connectors** | sqlx, tiberius, reqwest | Unified trait with Postgres, Snowflake, Databricks, MySQL, MSSQL, dbt |
| **Migration Pipeline** | Regex-based AST parser, syn 2.0 | Static AST parsing, Rust code generation |
| **Agentic Layer** | OpenAI / Anthropic APIs | LLM-assisted Python-to-Rust and dbt-to-Rust conversion |
| **Dashboard** | React 18.3 + TypeScript 5.3 + Vite 5.1 | SPA with Tailwind CSS 3.4, Zustand, React Query, embedded via rust-embed |
| **Charts** | Recharts 2.12 | Gantt visualization, temporal analysis |
| **Logging** | tracing + tracing-subscriber | Structured logging with JSON and env-filter support |
| **Metrics** | Prometheus 0.13 | Built-in `/metrics` endpoint |
| **Email** | Lettre 0.11 | SMTP notifications |
| **Plugins** | libloading 0.9 | Dynamic `.so`/`.dylib` loading at runtime |
| **Task Execution** | Direct process spawn | `sh -c` for bash, `python3` for python |

---

## Related Documentation

- [Authentication & Security](./AUTHENTICATION.md) — IAM, RBAC, secrets, and security model
- [Scheduling](./SCHEDULING.md) — Cron, dataset triggers, cross-DAG deps, dynamic mapping
- [Observability](./OBSERVABILITY.md) — Lineage, incident management, tracing, and metrics
- [Events & Sensors](./EVENTS_SENSORS.md) — Event bus, webhooks, and sensor framework
- [Compliance](./COMPLIANCE.md) — Audit logging, approval workflows, and governance
- [Configuration](./CONFIGURATION.md) — Config management, feature flags, and Git-Sync
- [Dashboard](./DASHBOARD.md) — React SPA features and development
- [API Reference](./API_REFERENCE.md) — Complete REST API documentation
- [CLI Reference](./CLI_REFERENCE.md) — CLI command reference
- [Deployment Guide](./DEPLOYMENT.md) — Build, configure, Docker, Kubernetes, and Helm
- [Python Integration](./PYTHON_INTEGRATION.md) — DAG authoring
- [Secrets Vault](./SECRETS_VAULT.md) — Encrypted secrets
- [Resilience](./RESILIENCE.md) — Auto-recovery and disaster recovery
- [Plugins](./PLUGINS.md) — Custom operator development and SDK
- [High Availability](./high-availability.md) — HA deployment with leader election
- [Migration Guide](./MIGRATION_GUIDE.md) — Airflow-to-Ryuo DAG migration
- [Connector API](./CONNECTOR_API.md) — Enterprise connector trait and implementations

---

## Glossary

| Term | Definition |
|------|-----------|
| **Controller** | The central Ryuo server process that accepts API calls, manages the task queue, and coordinates workers via gRPC. Also called "server". Runs as `ryuo server`. |
| **Worker** | A process that connects to the controller via gRPC, polls for tasks, executes them, and reports results. Part of the swarm. Runs as `ryuo worker`. |
| **Swarm** | The collection of worker processes managed by the controller via gRPC on port 50051. Enabled with `--swarm` on the controller. |
| **DAG** | Directed Acyclic Graph — a workflow definition composed of tasks and their dependency edges. DAGs are defined in Python or YAML and registered with the controller. |
| **Task** | A unit of work within a DAG (e.g., a bash command or Python callable). Each task has a unique `task_id` within its DAG. |
| **Task Instance** | A single execution of a task within a specific DAG run. Tracks state (`Queued`, `Running`, `Success`, `Failed`), stdout/stderr, duration, and retry count. Also called "task execution". |
| **DAG Run** | One complete execution of an entire DAG from trigger to final state. Each run has a unique `run_id` and is associated with an `execution_date`. |
| **XCom** | Cross-communication — a key/value store that allows tasks in the same DAG run to pass data to each other. Stored in the `task_xcom` table. |
| **Vault** | The encrypted secret storage subsystem. Secrets are encrypted with AES-256-GCM using `RYUO_SECRET_KEY` before storage and decrypted only at task execution time. |
| **Sensor** | A task that polls an external system (filesystem, HTTP endpoint, SQL query, or upstream DAG) until a condition is met, then completes successfully. |
| **Pool** | A named concurrency limiter. Tasks assigned to a pool consume slots; when the pool is full, additional tasks wait in the queue. |
| **Backfill** | Triggering runs for a date range in the past, allowing historical data reprocessing. |
| **Controller Address** | The gRPC endpoint workers connect to, format: `http://host:50051` (plaintext) or `https://host:50051` (TLS). Used with `--controller` flag. |
| **Team** | A multi-tenancy unit. Users and DAGs can be assigned to a team; non-admin users only see their own team's resources. |
| **Approval Workflow** | A gate requiring admin approval before a DAG change (trigger, edit, etc.) is applied. Enabled via the `approval_workflows` feature flag. |
