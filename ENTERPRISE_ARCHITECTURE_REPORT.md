# Vortex Architecture Report

## 1. Architectural Overview & Airflow Comparison

Vortex fundamentally rethinks data orchestration by replacing Python's heavy, process-based, GIL-bound architecture with Rust's high-performance, async-first paradigm.

### Key Architectural Advantages

1. **Concurrency Model (Tokio vs Python processes):** Airflow spawns multiple heavyweight OS processes to achieve parallel scheduling, placing an enormous burden on PostgreSQL due to heavy lock contention. Vortex uses Rust's `tokio` async runtime, evaluating dependencies and scheduling tasks using massively parallel, lightweight async tasks (~2KB memory footprint). Tasks yield instead of blocking, enabling orders of magnitude more parallel task executions per core.

2. **Native Connectors & Automated Conversion:** An automated AST-parsing and AI-agentic layer transpiles Airflow Python DAGs into native Rust code. Databricks, Snowflake, and PostgreSQL connectors execute directly in Rust using memory-efficient frameworks like `arrow-rs`, achieving near zero-copy memory deserialization.

3. **Single Binary Deployment:** Vortex compiles to a single ~15MB binary (containing the web UI, REST API, scheduler, and worker executor). This replaces Airflow's complex infrastructure footprint (Webserver, Schedulers, Triggerer, Celery/Redis, Workers).

4. **Distributed gRPC Swarm:** Vortex handles horizontal scaling out-of-the-box via its lightweight gRPC Swarm (Worker/Controller) architecture, gracefully managing heartbeats, node loss, and task requeuing.

5. **Built-in Security:** Includes an AES-256-GCM encrypted built-in vault, team-based multi-tenancy quotas, strict path traversal protections, and role-based access controls (RBAC) at the middleware level.

---

## 2. Feature Inventory

The following sections describe the capabilities implemented in the Vortex platform, organized by domain.

### Identity & Access Management (IAM)
**Module:** `src/auth.rs`, `src/rbac.rs`

- **Local Authentication** — Username/password with bcrypt hashing and rate-limited login (10 attempts/60s)
- **OIDC Integration** — OpenID Connect flow with discovery document, token exchange, and userinfo fetching (Okta, Azure AD, PingIdentity)
- **SAML/LDAP** — Configuration types and session management defined; provider implementations pending
- **Fine-Grained RBAC** — Role-based permissions with wildcard scope matching (`dag.*`, `secret.read`, etc.)
- **API Token Scoping** — Tokens with bcrypt-hashed verification, action/resource scoping, and automatic expiry
- **IP Allowlisting** — CIDR-based network access control supporting IPv4 and IPv6 subnets
- **Team Isolation** — Multi-tenant resource partitioning with per-team quotas

### Cloud-Native Infrastructure
**Module:** `src/k8s_executor.rs`, `Dockerfile`, `docker-compose.yml`, `helm/vortex/`

- **Dockerfile** — Multi-stage production build with minimal runtime image
- **Docker Compose** — Local development stack: Vortex controller + worker + PostgreSQL + Prometheus
- **Helm Chart** — Kubernetes deployment with controller StatefulSet, worker Deployment (HPA-ready), PVC for DAG storage, ConfigMap/Secret injection, readiness/liveness probes
- **Kubernetes Executor** — Pod spec generation implemented; full `kube-rs` client integration (pod submission and status polling) pending

### Scheduling & Data-Aware Orchestration
**Module:** `src/advanced_scheduler.rs`, `src/scheduler.rs`

- **Cron-based Scheduling** — Standard cron expressions and presets (`@daily`, `@hourly`, etc.)
- **Dataset-Triggered Scheduling** — Dataset and trigger types defined with DB schema; evaluation logic implemented
- **Cross-DAG Dependencies** — Upstream completion checking and dependency management
- **Dynamic Task Mapping** — Expand/reduce logic for runtime fan-out; full scheduler integration pending

### Observability & Data Governance
**Module:** `src/lineage.rs`, `src/incident.rs`, `src/telemetry.rs`, `src/metrics.rs`

- **Data Lineage** — OpenLineage-compliant event emission via HTTP and structured log emitters
- **PagerDuty Integration** — Full incident lifecycle (trigger/acknowledge/resolve) with HTTP API calls
- **Opsgenie/Datadog** — Configuration types defined; HTTP implementations pending
- **OpenTelemetry** — W3C TraceContext parsing/serialization and span builders; OTLP exporter pending
- **Prometheus Metrics** — Built-in `/metrics` endpoint with task/worker/queue gauges and histograms

### Developer Experience & CI/CD
**Module:** `src/devops.rs`

- **Git-Sync** — DAG repository synchronization via `git pull`/`git clone` with URL validation and branch name sanitization
- **CI/CD Pipeline Definitions** — Configuration types for pipeline generation

### Web Dashboard
**Module:** `ui/`, `assets/`

- **React SPA** — React 18 + TypeScript + Vite 5 with Tailwind CSS
- **14 Pages** — Dashboard, DAGs, Runs, Compliance, RBAC, Monitoring, Settings, Swarm, Lineage, Connectors, Events, and more
- **Dark/Light Mode** — Full theme toggle with persistence
- **State Management** — Zustand stores with React Query for API integration
- **Charts** — Recharts for Gantt visualization, Dagre for DAG graph rendering
- **Embedded via rust-embed** — Compiled assets bundled into the single Rust binary

### Legacy Scheduler Migration
**Module:** `src/migration.rs`, `src/airflow_ast_parser.rs`, `src/dag_codegen.rs`, `src/agentic.rs`

- **TWS Parser** — Parses IBM Tivoli Workload Scheduler definitions (job extraction, FOLLOWS/AT/DESCRIPTION)
- **Autosys JIL Parser** — Parses Autosys JIL definitions with insert_job detection
- **Airflow AST Parser** — Static Python AST extraction of DAGs, tasks, dependencies, and schedules
- **Rust Code Generator** — Native Rust DAG module generation from parsed AST IR
- **Agentic Migration** — LLM-assisted conversion (OpenAI/Anthropic) with compile-check and lint validation

### OpenAPI & API Governance
**Module:** `src/openapi.rs`

- **OpenAPI 3.1 Spec** — JSON spec served at `/api/openapi.json`
- **Rate Limiting** — Login rate limiting implemented; per-endpoint rate limiting middleware defined

### Compliance, Governance & Change Management
**Module:** `src/compliance.rs`

- **Audit Logging** — Detailed event tracking for all user and system actions
- **Approval Workflows** — DAG change approval gates with request/approve/reject lifecycle
- **Retention Policies** — Configurable time/count-based retention for logs and run history
- **Compliance Tracking** — SOC 2, GDPR, HIPAA control mapping and assessment

### Connector Ecosystem
**Module:** `src/connectors.rs`, `src/cloud_connectors.rs`, `src/enterprise_connector.rs`

| Connector | Status | Notes |
|-----------|--------|-------|
| PostgreSQL | Functional | Native async via `sqlx` with connection pooling and streaming |
| Snowflake | Functional | REST API with key-pair/OAuth auth and async query polling |
| Databricks | Functional | Dual-mode: SQL Warehouse + Jobs API |
| BigQuery | Functional | HTTP API with OAuth token auth and query execution |
| Redshift | Functional | SQLx PostgreSQL driver with real SQL execution |
| MySQL | Scaffolded | Async connector scaffold via `sqlx` MySQL driver |
| MS SQL | Scaffolded | Async connector scaffold via TDS (`tiberius`) |
| dbt | Functional | Shell controller: run/compile/test with JSON log capture |
| Kafka | Scaffolded | Configuration types defined |
| S3/GCS | Scaffolded | Configuration types defined |

### Disaster Recovery & Resilience
**Module:** `src/disaster_recovery.rs`

- **Backup Metadata** — In-memory backup tracking and metadata management
- **Failover Types** — Cluster node and health state type definitions
- **Implementation Status** — Backup I/O and automated restore are not yet operational

### Event-Driven Architecture
**Module:** `src/event_framework.rs`, `src/sensors.rs`

- **Event Bus** — Broadcast channel-based in-memory event log with filter matching
- **Event Filters** — Source glob matching, JSON path conditions, metadata checks
- **SQL/HTTP Sensors** — Lightweight polling tasks for external condition monitoring

### Configuration Management
**Module:** `src/config_ops.rs`

- **Config Manager** — Environment-scoped configuration with inheritance resolution (in-memory)
- **Feature Flags** — Boolean feature flag management with RwLock-protected store
- **Health Checks** — Operational health check types and maintenance window definitions

### Developer SDK & Plugin Ecosystem
**Module:** `src/sdk.rs`, `src/executor.rs`

- **Plugin Scaffold CLI** — Generates Cargo project structure (`Cargo.toml`, `src/lib.rs`, tests, manifest)
- **Dynamic Plugin Loading** — `.so`/`.dylib` plugins loaded from `plugins/` at runtime
- **Manifest Validation** — Plugin metadata validation and compatibility checking

---

## 3. Deployment Architecture

### Docker Compose (Development)

```
docker-compose up -d
```

Runs: Vortex controller + worker + PostgreSQL + Prometheus

### Kubernetes (Production)

The Helm chart at `helm/vortex/` provides:
- Controller StatefulSet with startup/readiness/liveness probes
- Worker Deployment with HPA-ready scaling and plugin volume mounts
- PVC for DAG storage
- ConfigMap/Secret injection
- Ingress configuration

### High Availability

Vortex supports active-standby HA using PostgreSQL advisory locks for leader election. See [High Availability Guide](./docs/high-availability.md).

---

## 4. Security Model

- **Authentication:** Local (bcrypt), OIDC (Okta/AzureAD), with SAML/LDAP types defined
- **Authorization:** Middleware-level RBAC with role-permission matrix and team scoping
- **Secrets:** AES-256-GCM encrypted vault with unique nonces per secret
- **Network:** IP allowlisting (CIDR), security headers (CSP, X-Frame-Options, X-Content-Type-Options)
- **API:** Scoped tokens with bcrypt hashing, rate limiting, request body size limits
- **Execution:** Python DAG sandboxing (opt-in `--allow-unsafe-dag-exec`), plugin sandboxing (opt-in `--allow-unsafe-plugins`)
- **Transport:** TLS support for REST API; mTLS for gRPC worker connections planned
