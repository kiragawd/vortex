# VORTEX 🌪️

**VORTEX** is a high-performance, single-binary enterprise orchestration engine designed to replace Apache Airflow with disruptive speed and simplicity.

Built in **Rust** with native **Python** DAG support via PyO3, VORTEX delivers sub-second scheduling, visual DAG monitoring, encrypted secret management, and distributed task execution — all from a single binary.

## Why VORTEX?

Because your data pipelines shouldn't spend more time scheduling tasks than executing them.

| Feature | Airflow | VORTEX |
|---------|---------|--------|
| **Startup** | Minutes (webserver + scheduler + workers + Redis + DB) | Seconds (single binary) |
| **Scheduling** | Python-based, GIL-bound | Lock-free Rust async (Tokio) |
| **Dependencies** | Python, Redis, Celery, PostgreSQL | Just Rust + Python runtime |
| **Binary Size** | ~500MB+ installed | ~15MB single binary |
| **DAG Compatibility** | Native | Airflow-compatible shim layer |

## Architecture

```
┌────────────────────────────────────────────────────────┐
│                  VORTEX Controller                      │
│                                                        │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐            │
│  │ REST API │  │Scheduler │  │ DAG Parser│            │
│  │ (Axum)   │  │ (Tokio)  │  │  (PyO3)   │            │
│  └─────┬────┘  └────┬─────┘  └─────┬─────┘            │
│        │            │              │                    │
│        └────────────┼──────────────┘                    │
│                     │                                   │
│              ┌──────┴──────┐                            │
│              │ PostgreSQL  │                            │
│              │ (Primary DB)│                            │
│              └──────┬──────┘                            │
│                     │                                   │
│              ┌──────┴──────┐                            │
│              │ gRPC Swarm  │                            │
│              │ Controller  │                            │
│              └──────┬──────┘                            │
│                     │                                   │
└─────────────────────┼──────────────────────────────────┘
                      │ gRPC
         ┌────────────┼────────────┐
         │            │            │
    ┌────┴────┐  ┌────┴────┐  ┌───┴─────┐
    │Worker 1 │  │Worker 2 │  │Worker N │
    │(Rust)   │  │(Rust)   │  │(Rust)   │
    └─────────┘  └─────────┘  └─────────┘
```

## Features

### Core Engine
- **Async-first scheduler** — Tokio-based, lock-free parallel task execution
- **Dependency-aware orchestration** — Topological sort with fan-out/fan-in support
- **Python DAG support** — Write DAGs in Python, execute at Rust speed via PyO3
- **Dynamic DAG Generation** — Support for loops and parameterization (Jinja/f-strings)
- **Airflow compatibility shim** — `from vortex import DAG, BashOperator, PythonOperator`

### Extensibility & Power
- **Plugin System** — Trait-based custom operators (e.g., S3, SQL, Slack)
- **Dynamic Loading** — Load `.so` / `.dylib` plugins from `plugins/` at runtime
- **Task Groups** — Logical and visual nesting of tasks for complex pipelines
- **DAG Factory** — Generate DAGs from YAML/JSON configs for non-Python users

### Web Dashboard (Built-in)
- **Visual DAG graphs** — Interactive D3.js + Dagre dependency visualization
- **Status Aggregation** — Real-time state coloring for Task Groups and DAGs
- **Run History** — Collapsible accordion with per-run graph snapshots
- **Code Editor** — In-browser DAG source editing with live re-parse
- **Audit Log** — Comprehensive trail of user actions (logins, triggers, DAG updates)
- **Temporal Analysis** — Gantt charts for execution bottlenecks and Calendar for scheduling
- **RBAC** — Admin / Operator / Viewer role-based access control
- **Team Isolation** — Multi-tenant support with per-team quotas and RBAC

### Distributed Execution (Swarm)
- **gRPC worker protocol** — Workers register, poll, execute, and report via Protobuf
- **Auto-recovery** — Dead worker detection, task re-queuing, health check loop
- **Worker draining** — Graceful shutdown with task completion

### Security & Reliability
- **AES-256-GCM encrypted vault** — Secrets encrypted at rest with unique nonces
- **One-Click Rollbacks** — Side-by-side version diffing and immediate rollback
- **Task Timeouts** — Configurable execution limits with auto-kill enforcement
- **RBAC enforcement** — Middleware-level role checks on all API endpoints
- **PostgreSQL Connectivity** — Connection pooling and production-grade migrations

## ⚠️ Production Considerations

By default, VORTEX runs as a single-node controller, which introduces a Single Point of Failure (SPOF). For production environments, it is strongly recommended to run VORTEX behind a supervisor (like `systemd` or Kubernetes deployments) configured to automatically restart the process on failure.

For true active-standby High Availability (HA) across multiple machines, VORTEX supports a leader election mode using PostgreSQL advisory locks.

See the [High Availability Guide](./docs/high-availability.md) for full setup instructions and architectural details.

## Current Limitations & Roadmap

While VORTEX provides a highly performant execution engine, it intentionally foregoes some of the larger ecosystem features found in legacy orchestrators like Airflow. The following features are currently missing or planned for future releases:

- **Provider/Connector Ecosystem:** VORTEX includes a native HTTP operator and a dynamic plugin system. It does not ship with thousands of pre-built integrations (AWS, GCP, Snowflake, etc.).
- **Dataset-Triggered Scheduling:** Data-aware scheduling and Dataset triggers are not currently implemented, but are on the **Roadmap**.
- **Dynamic Task Mapping:** Runtime task fan-out (e.g., `task.expand()`) is not yet supported. Static DAGs cover the vast majority of use cases; dynamic mapping is on the **Roadmap**.
- **Authentication (SSO/LDAP):** Authentication is handled natively via API keys, which is appropriate for a v1 OSS release. OAuth 2.0, SAML, and LDAP integrations are not included.
- **Kubernetes Executor:** VORTEX scales horizontally via its built-in gRPC Swarm (Worker/Controller pattern), which efficiently manages multi-node workloads. A native pod-per-task K8s executor is considered **v2 territory**.
- **Quality of Life Enhancements:** 
  - **Data Lineage:** OpenLineage / Atlas integrations are not supported.
  - **Connection UI:** connection management is scoped to the Secrets Vault; named connections with UI builder are not implemented.
  - **Custom Timetables:** Schedules rely on cron and standard presets rather than custom timetable classes.

## Getting Started

### Prerequisites

- **Rust** — Latest stable (1.70+)
- **Python** — 3.13+ or 3.14+
- **PostgreSQL** — 14+ (Recommended for production)
- **protoc** — Protocol Buffers compiler (for gRPC)

### Build

```bash
git clone https://github.com/kiragawd/vortex.git
cd vortex

# Python 3.14+ requires this env var
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

cargo build --release
```

### Run Controller + Swarm

```bash
# Terminal 1: Start server with PostgreSQL
./target/release/vortex server --swarm --database-url "postgres://user:pass@localhost/vortex"

# Terminal 2: Start a worker
./target/release/vortex worker --controller http://localhost:50051 --capacity 4
```

### Access Dashboard

Open **http://localhost:3000** in your browser.

**Default credentials:** `admin` / `admin`

### Create a DAG

Create `dags/my_pipeline.py`:

```python
from vortex import DAG, BashOperator, TaskGroup

with DAG("my_pipeline", schedule_interval="@daily") as dag:
    with TaskGroup("ingestion") as tg:
        t1 = BashOperator(task_id="extract", bash_command="echo 'Extracting...'")
        t2 = BashOperator(task_id="transform", bash_command="echo 'Transforming...'")
        t1 >> t2
    
    finish = BashOperator(task_id="finish", bash_command="echo 'Done!'")
    tg >> finish
```

The DAG is automatically loaded on server startup or can be uploaded via the web UI.

## CLI Reference

VORTEX comes with a dedicated CLI for automation.

```bash
vortex dags list
vortex dags trigger <dag_id>
vortex dags backfill <dag_id> --start 2026-01-01 --end 2026-02-01 --parallel 4
vortex secrets set MY_KEY MY_VAL
vortex users create new_user --role Operator
```

Run `vortex --help` for full command reference.

## Database Schema

VORTEX uses a unified relational schema (PostgreSQL recommended) with the following tables:

- **`dags`** — DAG definitions, schedule, team assignment, pause state
- **`tasks`** — Task definitions (id, command, type, config, group, timeout, retry)
- **`task_instances`** — Execution records with state, logs, duration, run_id, worker_id
- **`dag_runs`** — Run records with state, triggered_by, timestamps
- **`dag_versions`** — Snapshots linking DAGs to source files for rollbacks
- **`audit_log`** — Permanent trail of security and operational events
- **`workers`** — Worker registrations, heartbeats, capacity
- **`users`** — RBAC user accounts with API keys and team IDs
- **`teams`** — Multi-tenancy isolation with resource quotas
- **`secrets`** — AES-256-GCM encrypted key-value secrets

## Project Structure

```
vortex/
├── src/
│   ├── main.rs           # Entry point, CLI parsing, orchestration loop
│   ├── scheduler.rs      # DAG/Task structs, dependency-aware scheduler
│   ├── db_trait.rs       # Unified database abstraction
│   ├── db_postgres.rs    # PostgreSQL implementation
│   ├── db_sqlite.rs      # SQLite implementation (dev only)
│   ├── web.rs            # Axum REST API + static asset serving
│   ├── swarm.rs          # gRPC Swarm controller
│   ├── worker.rs         # gRPC Swarm worker
│   ├── proto.rs          # Consolidated gRPC definitions
│   ├── executor.rs       # Plugin-based task execution (bash/python/http)
│   ├── vault.rs          # AES-256-GCM encryption for secrets
│   ├── python_parser.rs  # PyO3 + Dynamic DAG logic
│   ├── dag_factory.rs    # YAML/JSON DAG generation
│   ├── metrics.rs        # Prometheus instrumentation
│   └── lib.rs            # Library exports
├── python/vortex/        # Python Airflow-compatibility shim
├── assets/index.html     # Single-file Web Dashboard (D3 + Dagre)
├── plugins/              # Dynamic .so/.dylib operator plugins
├── migrations/           # SQLx database migration scripts
├── dags/                 # DAG files (auto-loaded on startup)
├── proto/                # gRPC Protobuf definitions
├── tests/                # Unit + integration tests
└── docs/                 # Full documentation
```

## Documentation

- **[Architecture](./docs/ARCHITECTURE.md)** — System design and data flow
- **[API Reference](./docs/API_REFERENCE.md)** — Complete REST API with examples
- **[Deployment Guide](./docs/DEPLOYMENT.md)** — Build, configure, and run in production
- **[Python Integration](./docs/PHASE_2_PYTHON_INTEGRATION.md)** — DAG authoring with Python
- **[Secrets Vault](./docs/PILLAR_3_SECRETS_VAULT.md)** — Encrypted secret management
- **[Resilience](./docs/PILLAR_4_RESILIENCE.md)** — Auto-recovery and health monitoring

## Testing

```bash
# Rust unit + integration tests
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --all

# UI tests (Playwright)
npm install && npm test
```

## License

**Dual-licensed:**

- **Personal & Open Source:** MIT License — Free for personal projects, education, and non-commercial work
- **Enterprise:** Commercial license required for business use or SaaS

See [LICENSE.md](./LICENSE.md) for full terms.
