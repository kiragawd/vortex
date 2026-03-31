# VORTEX 🌪️

**VORTEX** is a high-performance, single-binary enterprise orchestration engine designed to replace Apache Airflow with disruptive speed and simplicity.

Built in **Rust** with native **Python** DAG support via PyO3, VORTEX delivers sub-second scheduling, visual DAG monitoring, encrypted secret management, and distributed task execution — all from a single binary.

## Why VORTEX?

Because your data pipelines shouldn't spend more time scheduling tasks than executing them.

| Feature | Airflow | VORTEX |
|---------|---------|--------|
| **Startup** | Minutes (webserver + scheduler + workers + Redis + DB) | Seconds (single binary) |
| **Scheduling** | Python-based, GIL-bound | Lock-free Rust async (Tokio) |
| **Dependencies** | Python, Redis, Celery, PostgreSQL | Rust + Python runtime + PostgreSQL |
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

### Enterprise Connectors
- **Unified Connector Trait** — `EnterpriseConnector` contract in `src/enterprise_connector.rs` with config validation, health checks, query execution, streaming, and introspection
- **Connector Registry** — Dynamic registration and lookup of connectors by name
- **PostgreSQL** — Native async connector via `sqlx` with connection pooling, streaming fetch, and query instrumentation
- **Snowflake** — REST API connector with key-pair / OAuth auth, async query polling, and Arrow result format support
- **Databricks** — Dual-mode connector: SQL Warehouse for direct queries and Jobs API for workflow triggers
- **MySQL** — Async connector scaffold via `sqlx` MySQL driver with type normalization
- **MS SQL Server** — Async connector scaffold via TDS (`tiberius`) with type normalization
- **dbt** — Shell controller connector: runs `dbt compile/run/test`, captures JSON logs, maps exit codes to task status
- **Retry & Timeout** — Cross-cutting retry policy with configurable backoff on all connectors
- **Capability Flags** — Connectors declare capabilities (Transactions, BatchRead, StreamingRead, AsyncJobs, ArrowZeroCopy, etc.)

### Airflow Migration Pipeline
- **Static AST Parser** — Rust-native Python AST parser (`src/airflow_ast_parser.rs`) extracts DAGs, tasks, dependencies, and schedules without executing Python
- **Rust DAG Code Generator** — Generates native Rust DAG modules from parsed AST IR (`src/dag_codegen.rs`), with `todo!()` placeholders for unsupported constructs
- **CLI `migrate` Command** — `vortex-cli migrate <path>` transpiles Airflow DAGs to Rust with `--strict`, `--report-format`, and `--use-shim-fallback` options
- **Migration Reports** — JSON/Markdown reports listing converted tasks, placeholder tasks, and required manual actions
- **Graph Equivalence Validation** — Automated checks that generated DAG dependency topology matches the source Airflow DAG

### Agentic Migration (AI-Assisted)
- **LLM Provider Integration** — Provider-agnostic abstraction supporting OpenAI and Anthropic (`src/agentic.rs`)
- **Python-to-Rust Agent** — Iterative translation loop: analyze → plan → generate → compile-check → lint → repair until passing or budget exhausted
- **dbt-to-Rust Agent** — Parses dbt manifest, expands Jinja SQL, builds dependency graph, and generates Rust pipeline modules
- **Safety Guardrails** — Policy-based blocking of dangerous APIs, forced explicit error handling, and compile validation on all generated code
- **Token/Cost Telemetry** — Tracks LLM usage and cost per agentic conversion

### Extensibility & Power
- **Plugin System** — Trait-based custom operators (e.g., HTTP, SQL, Slack)
- **Dynamic Loading** — Load `.so` / `.dylib` plugins from `plugins/` at runtime
- **Task Groups** — Logical and visual nesting of tasks for complex pipelines
- **DAG Factory** — Generate DAGs from YAML/JSON configs for non-Python users
- **XCom** — Cross-task communication via push/pull key-value store
- **Task Pools** — Concurrency-limiting resource pools for shared resources
- **Webhook Callbacks** — Configurable notifications on success/failure/retry/SLA miss (Webhook, Slack, Email)

### Web Dashboard
- **React SPA** — React 18 + TypeScript + Vite 5 with Tailwind CSS, dark/light mode
- **Visual DAG graphs** — Interactive D3.js + Dagre dependency visualization
- **14 pages** — Dashboard, DAGs, Runs, Compliance, RBAC, Monitoring, Settings, Swarm, Lineage, Connectors, Events, and more
- **Status Aggregation** — Real-time state coloring for Task Groups and DAGs
- **Run History** — Collapsible accordion with per-run graph snapshots
- **Code Editor** — In-browser DAG source editing with live re-parse
- **Audit Log** — Comprehensive trail of user actions (logins, triggers, DAG updates)
- **Temporal Analysis** — Gantt charts for execution bottlenecks and Calendar for scheduling

### Authentication & Access Control
- **Local auth** — Username/password with bcrypt hashing
- **OIDC** — OpenID Connect integration (Okta, Azure AD, PingIdentity)
- **Fine-grained RBAC** — Role-based permissions with resource-level scoping
- **API token scoping** — Tokens restricted by action/resource with wildcard matching
- **IP allowlisting** — CIDR-based network access control (IPv4/IPv6)
- **Team isolation** — Multi-tenant support with per-team quotas and resource partitioning

### Compliance & Governance
- **Audit logging** — Detailed event tracking for all user and system actions
- **Approval workflows** — DAG change approval gates for change management
- **Retention policies** — Configurable time/count-based retention for logs and history
- **Compliance tracking** — SOC 2, GDPR, HIPAA control mapping

### Observability
- **Data lineage** — OpenLineage-compliant event emission (HTTP and log emitters)
- **Incident management** — PagerDuty integration with trigger/acknowledge/resolve
- **OpenTelemetry** — W3C TraceContext propagation and span builders
- **Prometheus metrics** — Built-in `/metrics` endpoint for Grafana dashboards

### Distributed Execution (Swarm)
- **gRPC worker protocol** — Workers register, poll, execute, and report via Protobuf
- **Auto-recovery** — Dead worker detection, task re-queuing, health check loop
- **Worker re-registration** — Workers automatically re-register after controller restart without manual intervention
- **Worker draining** — Graceful shutdown with task completion

### Security & Reliability
- **AES-256-GCM encrypted vault** — Secrets encrypted at rest with unique nonces
- **Login rate-limiting** — Max 10 attempts per 60 s per username, returns `429 Too Many Requests`
- **Schedule validation** — `normalize_schedule` validates cron expressions at DAG registration time, rejecting garbage expressions before they can crash the cron loop
- **Execution Sandboxing** — Python DAG execution (`--allow-unsafe-dag-exec`) and dynamic `.so` plugins (`--allow-unsafe-plugins`) are disabled by default and require explicit CLI opt-in.
- **Path traversal protection** — DAG source updates validate against the canonical `dags/` directory using strict resolution guards.
- **Security headers** — All responses include `Content-Security-Policy`, `X-Frame-Options: DENY`, and `X-Content-Type-Options: nosniff`
- **Request body limits** — Bodies > 10 MB are rejected with `413 Payload Too Large`
- **Health check endpoint** — `GET /health` verifies DB connectivity; ready for load-balancer probes and K8s readiness checks
- **Graceful shutdown** — On `SIGINT`/`SIGTERM`, marks all `Running` tasks as `Failed` and releases the HA leader lock before exiting
- **Team isolation** — Non-admin users can only see and trigger their own team's DAGs
- **One-Click Rollbacks** — Side-by-side version diffing and immediate rollback
- **Task Timeouts** — Configurable execution limits with auto-kill enforcement
- **RBAC enforcement** — Middleware-level role checks on all API endpoints
- **PostgreSQL backend** — Connection pooling and production-grade migrations
- **Configurable bind addresses** — Use `--port` and `--grpc-bind` to restrict network exposure

## ⚠️ Production Considerations

By default, VORTEX runs as a single-node controller, which introduces a Single Point of Failure (SPOF). For production environments, it is strongly recommended to run VORTEX behind a supervisor (like `systemd` or Kubernetes deployments) configured to automatically restart the process on failure.

For true active-standby High Availability (HA) across multiple machines, VORTEX supports a leader election mode using PostgreSQL advisory locks.

See the [High Availability Guide](./docs/high-availability.md) for full setup instructions and architectural details.

## Current Limitations & Future Enhancements

While VORTEX provides a comprehensive orchestration platform, some features are scaffolded for future completion:

- **Kubernetes Executor:** Pod spec generation is implemented, but full `kube-rs` client integration for pod submission and status polling is pending. VORTEX scales horizontally via its built-in gRPC Swarm in the meantime.
- **SSO (SAML/LDAP):** Local and OIDC authentication are functional. SAML and LDAP providers have configuration types defined but lack full provider implementations.
- **Disaster Recovery:** Backup metadata tracking and failover types exist, but end-to-end backup I/O and automated restore are not yet operational.
- **OpenTelemetry Export:** W3C TraceContext propagation and span types are complete but the OTLP exporter (HTTP/gRPC sender) is not yet wired.
- **Dynamic Task Mapping:** Runtime task fan-out (e.g., `task.expand()`) has expand/reduce logic but is not fully integrated with the scheduler loop.
- **Connection UI:** Connection management is scoped to the Secrets Vault; named connections with a UI builder are not implemented.
- **Custom Timetables:** Schedules rely on cron and standard presets rather than custom timetable classes.

## Getting Started

### Prerequisites

- **Rust** — Latest stable (1.70+)
- **Python** — 3.13+ or 3.14+
- **PostgreSQL** — 14+ (required)
- **protoc** — Protocol Buffers compiler (for gRPC)

### Build

```bash
git clone https://github.com/kiragawd/vortex.git
cd vortex

# Python 3.14+ requires this env var
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

cargo build --release
```

**Note:** By default, VORTEX runs in a secure sandbox mode. To execute Python DAGs or load dynamic plugins, you must pass the corresponding explicit opt-in flags.

### Run Controller + Swarm

```bash
# Terminal 1: Start server with PostgreSQL (and Python DAG support enabled)
./target/release/vortex server --swarm --database-url "postgres://user:pass@localhost/vortex" --allow-unsafe-dag-exec

# Optional: custom web port (default 3000) and restrict gRPC to localhost
./target/release/vortex server --swarm --database-url "postgres://..." --port 8080 --grpc-bind 127.0.0.1 --allow-unsafe-dag-exec

# Optional: register the built-in benchmark DAG
./target/release/vortex server --swarm --database-url "postgres://..." --benchmark

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

VORTEX comes with a dedicated CLI (`vortex-cli`) for automation.

```bash
vortex-cli dags list
vortex-cli dags trigger <dag_id>
vortex-cli dags pause <dag_id>
vortex-cli dags unpause <dag_id>
vortex-cli dags backfill <dag_id> --start 2026-01-01 --end 2026-02-01 --parallel 4
vortex-cli migrate ./dags --output-dir ./generated_dags --strict
vortex-cli migrate ./dags --agentic --llm-provider openai --model gpt-4o-mini
vortex-cli tasks logs <task_instance_id>
vortex-cli secrets set MY_KEY MY_VAL
vortex-cli users create new_user --role Operator
```

Run `vortex-cli --help` for full command reference. See [CLI Reference](./docs/CLI_REFERENCE.md) for details on all supported flags.

## Database Schema

VORTEX uses PostgreSQL with the following tables:

**Core:**
- **`dags`** — DAG definitions, schedule, team assignment, pause state
- **`tasks`** — Task definitions (id, command, type, config, group, timeout, retry)
- **`task_instances`** — Execution records with state, logs, duration, run_id, worker_id
- **`dag_runs`** — Run records with state, triggered_by, timestamps
- **`dag_versions`** — Snapshots linking DAGs to source files for rollbacks
- **`workers`** — Worker registrations, heartbeats, capacity
- **`secrets`** — AES-256-GCM encrypted key-value secrets
- **`task_xcom`** — Cross-task communication key-value store
- **`pools`** / **`pool_slots`** — Concurrency-limiting resource pools

**IAM & Access Control:**
- **`users`** — User accounts with bcrypt-hashed passwords and team IDs
- **`teams`** — Multi-tenancy isolation with resource quotas
- **`auth_providers`** — OIDC/SAML/LDAP/Local provider configurations
- **`user_sessions`** — SSO session tracking with token storage
- **`rbac_permissions`** / **`rbac_roles`** / **`rbac_role_permissions`** / **`rbac_user_roles`** — Permission matrix
- **`api_tokens`** — Scoped API tokens with hash verification and expiry
- **`ip_allowlist`** — CIDR-based network access rules

**Compliance & Governance:**
- **`audit_log`** — Permanent trail of security and operational events
- **`approval_gates`** / **`approval_requests`** — Change management workflow
- **`retention_policies`** — Data retention configuration
- **`compliance_controls`** — Regulatory control mapping (SOC 2, GDPR, etc.)
- **`dag_callbacks`** — Per-DAG webhook/notification configuration

**Observability:**
- **`lineage_events`** / **`lineage_datasets`** — OpenLineage data tracking
- **`incident_configs`** — PagerDuty/Opsgenie/Datadog alert configuration

**Scheduling:**
- **`datasets`** / **`dataset_events`** / **`dataset_triggers`** — Data-aware scheduling
- **`cross_dag_dependencies`** — Cross-DAG dependency management
- **`task_map_templates`** — Dynamic task mapping configuration

## Project Structure

```
vortex/
├── src/
│   ├── main.rs               # Entry point, CLI parsing, orchestration loop
│   ├── lib.rs                 # Library exports
│   ├── scheduler.rs           # DAG/Task structs, dependency-aware scheduler
│   ├── advanced_scheduler.rs  # Dataset triggers, cross-DAG deps, dynamic mapping
│   ├── db_trait.rs            # Unified database abstraction trait
│   ├── db_postgres.rs         # PostgreSQL implementation
│   ├── web.rs                 # Axum REST API + static asset serving
│   ├── swarm.rs               # gRPC Swarm controller
│   ├── worker.rs              # gRPC Swarm worker
│   ├── proto.rs               # Consolidated gRPC definitions
│   ├── executor.rs            # Plugin-based task execution (bash/python/http)
│   ├── vault.rs               # AES-256-GCM encryption for secrets
│   ├── python_parser.rs       # PyO3 + Dynamic DAG logic
│   ├── dag_factory.rs         # YAML/JSON DAG generation
│   ├── metrics.rs             # Prometheus instrumentation
│   ├── xcom.rs                # Cross-task communication (XCom)
│   ├── pools.rs               # Task pool management
│   ├── auth.rs                # SSO/OIDC/SAML/LDAP authentication
│   ├── rbac.rs                # Fine-grained RBAC, API tokens, IP allowlisting
│   ├── compliance.rs          # Audit logging, approval workflows, retention
│   ├── lineage.rs             # OpenLineage data lineage emission
│   ├── incident.rs            # PagerDuty/Opsgenie/Datadog incident triggers
│   ├── telemetry.rs           # OpenTelemetry tracing and APM
│   ├── openapi.rs             # OpenAPI 3.1 spec generation
│   ├── connectors.rs          # Core connector implementations (Postgres, Snowflake, Databricks, dbt, MySQL, MSSQL)
│   ├── cloud_connectors.rs    # Cloud connectors (BigQuery, Redshift, Kafka, S3, GCS)
│   ├── enterprise_connector.rs # Unified connector trait and registry
│   ├── airflow_ast_parser.rs  # Static Python AST parser for Airflow DAGs
│   ├── dag_codegen.rs         # Rust DAG code generator from parsed AST
│   ├── agentic.rs             # LLM-assisted migration (OpenAI/Anthropic)
│   ├── migration.rs           # TWS/Autosys JIL parsers and migration CLI
│   ├── sensors.rs             # SQL/HTTP sensor operators
│   ├── notifications.rs       # Webhook/Slack/Email callback notifications
│   ├── event_framework.rs     # Event bus, webhook receiver, sensor registry
│   ├── k8s_executor.rs        # Kubernetes pod-per-task executor
│   ├── disaster_recovery.rs   # Backup/restore, failover orchestration
│   ├── config_ops.rs          # Configuration management, feature flags
│   ├── devops.rs              # Git-sync, CI/CD pipeline tooling
│   ├── sdk.rs                 # Plugin SDK scaffolding and marketplace
│   └── bin/                   # CLI binary entry points
├── ui/                        # React 18 + TypeScript + Vite 5 SPA
│   ├── src/                   # Components, pages, stores, API clients
│   ├── package.json           # Node dependencies
│   └── vite.config.ts         # Build configuration
├── python/vortex/             # Python Airflow-compatibility shim
├── assets/                    # Compiled static assets (embedded via rust-embed)
├── plugins/                   # Dynamic .so/.dylib operator plugins
├── migrations/                # PostgreSQL migration scripts
├── dags/                      # DAG files (auto-loaded on startup)
├── proto/                     # gRPC Protobuf definitions
├── helm/vortex/               # Helm chart for Kubernetes deployment
├── tests/                     # Unit + integration + E2E tests
├── docs/                      # Documentation
├── Dockerfile                 # Multi-stage production build
├── docker-compose.yml         # Local dev stack (Vortex + PostgreSQL + Prometheus)
└── prometheus.yml             # Prometheus scrape configuration
```

## Documentation

- **[Architecture](./docs/ARCHITECTURE.md)** — System design and data flow
- **[API Reference](./docs/API_REFERENCE.md)** — Complete REST API with examples
- **[CLI Reference](./docs/CLI_REFERENCE.md)** — CLI command reference
- **[Deployment Guide](./docs/DEPLOYMENT.md)** — Build, configure, and run in production
- **[Python Integration](./docs/PYTHON_INTEGRATION.md)** — DAG authoring with Python
- **[Secrets Vault](./docs/SECRETS_VAULT.md)** — Encrypted secret management
- **[Resilience](./docs/RESILIENCE.md)** — Auto-recovery and health monitoring
- **[Plugins](./docs/PLUGINS.md)** — Custom operator development
- **[High Availability](./docs/high-availability.md)** — HA deployment with leader election
- **[Migration Guide](./docs/MIGRATION_GUIDE.md)** — Airflow-to-Vortex DAG migration
- **[Connector API](./docs/CONNECTOR_API.md)** — Connector trait and implementations

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
