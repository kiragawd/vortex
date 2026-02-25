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
│              │   SQLite    │                            │
│              │  (vortex.db)│                            │
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
- **Airflow compatibility shim** — `from vortex import DAG, BashOperator, PythonOperator`

### Web Dashboard (Built-in)
- **Visual DAG graphs** — Interactive D3.js + Dagre dependency visualization
- **Live status monitoring** — Real-time task state coloring (Success/Failed/Running)
- **Run History** — Collapsible accordion with per-run graph snapshots
- **Code Editor** — In-browser DAG source editing with live re-parse
- **RBAC** — Admin / Operator / Viewer role-based access control
- **Login system** — API key authentication with localStorage persistence

### Distributed Execution (Swarm)
- **gRPC worker protocol** — Workers register, poll, execute, and report via Protobuf
- **Auto-recovery** — Dead worker detection, task re-queuing, health check loop
- **Worker draining** — Graceful shutdown with task completion

### Security (Pillar 3)
- **AES-256-GCM encrypted vault** — Secrets encrypted at rest with unique nonces
- **Secret injection** — Decrypted secrets passed as env vars to task workers
- **RBAC enforcement** — Middleware-level role checks on all API endpoints

## Getting Started

### Prerequisites

- **Rust** — Latest stable (1.70+)
- **Python** — 3.13+ or 3.14+
- **protoc** — Protocol Buffers compiler (for gRPC)

### Build

```bash
git clone https://github.com/kiragawd/vortex.git
cd vortex

# Python 3.14+ requires this env var
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

cargo build
```

### Run Controller + Swarm

```bash
# Terminal 1: Start server with Swarm enabled
./target/debug/vortex server --swarm

# Terminal 2: Start a worker
./target/debug/vortex worker --controller http://localhost:50051 --capacity 4
```

### Access Dashboard

Open **http://localhost:3000** in your browser.

**Default credentials:** `admin` / `admin`

### Create a DAG

Create `dags/my_pipeline.py`:

```python
from vortex import DAG, BashOperator

with DAG("my_pipeline", schedule_interval="@daily") as dag:
    start = BashOperator(task_id="start", bash_command="echo 'Pipeline started'")
    process = BashOperator(task_id="process", bash_command="echo 'Processing data...'")
    finish = BashOperator(task_id="finish", bash_command="echo 'Done!'")

    start >> process >> finish
```

The DAG is automatically loaded on server startup or can be uploaded via the web UI.

## CLI Reference

```
vortex server [--swarm] [--swarm-port 50051]
    Start the VORTEX controller with REST API on port 3000.
    --swarm           Enable gRPC Swarm controller for distributed workers
    --swarm-port      gRPC listen port (default: 50051)

vortex worker --controller <URL> [--capacity N] [--id <ID>] [--labels <L1,L2>]
    Start a Swarm worker that connects to a controller.
    --controller      gRPC controller address (e.g., http://localhost:50051)
    --capacity        Max concurrent tasks (default: 4)
    --id              Worker ID (auto-generated if omitted)
    --labels          Comma-separated labels for task affinity
```

## REST API

All endpoints require `Authorization: <api_key>` header (except `/api/login`).

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/login` | Authenticate and get API key |
| `GET` | `/api/dags` | List all DAGs |
| `POST` | `/api/dags/upload` | Upload a `.py` DAG file |
| `GET` | `/api/dags/:id/tasks` | Get DAG tasks, instances, and dependencies |
| `GET` | `/api/dags/:id/runs` | Get run history |
| `POST` | `/api/dags/:id/trigger` | Trigger a manual run |
| `POST` | `/api/dags/:id/retry` | Retry failed tasks |
| `PATCH` | `/api/dags/:id/pause` | Pause DAG |
| `PATCH` | `/api/dags/:id/unpause` | Unpause DAG |
| `GET` | `/api/dags/:id/source` | Get DAG source code |
| `PATCH` | `/api/dags/:id/source` | Update DAG source and re-parse |
| `GET` | `/api/tasks/:id/logs` | Get task execution logs |
| `GET` | `/api/swarm/status` | Swarm status (workers, queue depth) |
| `GET` | `/api/swarm/workers` | List all workers with state |
| `POST` | `/api/swarm/workers/:id/drain` | Drain a worker |
| `DELETE` | `/api/swarm/workers/:id` | Remove a worker |
| `GET` | `/api/secrets` | List secret keys |
| `POST` | `/api/secrets` | Store an encrypted secret |
| `DELETE` | `/api/secrets/:key` | Delete a secret |
| `GET` | `/api/users` | List users (Admin only) |
| `POST` | `/api/users` | Create user (Admin only) |
| `DELETE` | `/api/users/:username` | Delete user (Admin only) |

See [docs/API_REFERENCE.md](./docs/API_REFERENCE.md) for full request/response examples.

## Database Schema

SQLite database (`vortex.db`) with the following tables:

- **`dags`** — DAG definitions, schedule, pause state
- **`tasks`** — Task definitions (id, command, type, config, retry settings)
- **`task_instances`** — Execution records with state, logs, duration, run_id, worker_id
- **`dag_runs`** — Run records with state, triggered_by, timestamps
- **`dag_versions`** — Version history linking DAGs to filesystem paths
- **`workers`** — Worker registrations, heartbeats, capacity
- **`users`** — RBAC user accounts with API keys
- **`secrets`** — AES-256-GCM encrypted key-value secrets

## Project Structure

```
vortex/
├── src/
│   ├── main.rs           # Entry point, CLI parsing, orchestration loop
│   ├── scheduler.rs      # DAG/Task structs, dependency-aware scheduler
│   ├── db.rs             # SQLite database layer (all table ops)
│   ├── web.rs            # Axum REST API + static asset serving
│   ├── swarm.rs          # gRPC Swarm controller + worker management
│   ├── worker.rs         # gRPC Swarm worker (poll/execute/report)
│   ├── executor.rs       # Task execution (bash + python) with timeouts
│   ├── vault.rs          # AES-256-GCM encryption for secrets
│   ├── python_parser.rs  # PyO3 + regex DAG parser
│   └── lib.rs            # Library exports for testing
├── python/vortex/        # Python Airflow-compatibility shim
├── assets/index.html     # Single-file Web Dashboard (Tailwind + D3 + Dagre)
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
