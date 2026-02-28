# TODO.md — VORTEX Enterprise Roadmap

> Gap analysis vs Apache Airflow at enterprise scale.
> Prioritized by severity and impact on production readiness.

---

## 🟢 Already Implemented (Airflow Parity)

- [x] Python DAG definitions (PyO3 + Airflow-compatible shim)
- [x] Dependency-aware topological sort with fan-out/fan-in
- [x] Built-in Web Dashboard (D3 + Dagre visual graphs)
- [x] Task retry with configurable backoff
- [x] Secrets management (AES-256-GCM encrypted vault)
- [x] RBAC (Admin / Operator / Viewer)
- [x] REST API (25 endpoints)
- [x] Distributed workers (gRPC Swarm)
- [x] Unified Async Database Trait (`Arc<dyn DatabaseBackend>`)
- [x] SQLx-based SQLite backend (100% async)
- [x] Auto-recovery on worker failure (health check loop)
- [x] Task logs (FS-based with DB metadata)
- [x] Visual DAG dependency graph
- [x] Gantt/Timeline execution visualization
- [x] Calendar scheduling view
- [x] Comprehensive Audit Logging
- [x] DAG pause / unpause
- [x] Manual trigger + retry from failure
- [x] Run history with per-run task breakdown
- [x] In-browser DAG code editor with live re-parse
- [x] DAG file upload (multipart)

---

## Phase 1 — Security & Scheduling ✅ COMPLETE

> Shipped 2026-02-26. Commit `4cd8550`.

### 1. Cron Scheduler Loop (Auto-Trigger DAGs)
- [x] Tokio task that wakes every 60 seconds
- [x] Evaluate all DAGs with `schedule_interval` (cron expressions)
- [x] Compare `last_run` vs cron expression to decide if a run is due
- [x] Respect `is_paused`, `max_active_runs`, and `catchup` flags
- [x] Update `last_run` and `next_run` columns after each trigger
- [x] Support standard cron syntax + Airflow presets (`@daily`, `@hourly`, `@weekly`)

**Why:** Airflow's entire value prop is *automated* scheduling. Without this, VORTEX is trigger-only — nobody will take it seriously.

### 2. Password Hashing (Security Critical) ✅
- [x] Replace plaintext `password_hash` storage with bcrypt or argon2
- [x] Hash password on user creation (`POST /api/users`)
- [x] Hash + compare on login (`POST /api/login`)
- [x] Migrate existing plaintext passwords on first startup

**Why:** Storing passwords in plaintext is a CVE waiting to happen. Non-negotiable for any production system.

### 3. Structured Logging ✅
- [x] Replace all `println!()` / `eprintln!()` with `tracing` crate
- [x] Add log levels: DEBUG, INFO, WARN, ERROR
- [x] JSON output format for log aggregation (ELK, Splunk, Datadog)
- [x] Configurable log level via CLI flag (`--log-level info`)

**Why:** Enterprise ops teams need searchable, structured logs. `println!` doesn't cut it.

### 4. TLS / HTTPS Support ✅
- [x] Accept `--tls-cert` and `--tls-key` CLI flags
- [x] Serve Axum over HTTPS when certs are provided

**Why:** API keys are currently transmitted in plaintext over HTTP. Unacceptable for production.

### Phase 1 — Remaining Polish ✅
- [x] Add password strength validation (min length, complexity)
- [x] File rotation via `tracing-appender`
- [x] Request-level tracing with correlation IDs
- [x] gRPC TLS for controller ↔ worker communication
- [x] Document cert generation with `openssl` / Let's Encrypt

---

## Phase 2 — Data Pipeline Essentials ✅ COMPLETE

> Unified as 100% async under the `sqlx` backend trait in **Wave 1** (Feb 2026).

### 5. XCom (Inter-Task Data Passing) ✅
- [x] `task_xcom` table: `(dag_id, task_id, run_id, key, value, timestamp)`
- [x] `xcom_push(key, value)` — callable from Python tasks via context
- [x] `xcom_pull(task_id, key)` — retrieve upstream task output
- [x] Inject upstream XCom values as env vars for bash tasks
- [x] REST API: `GET /api/dags/:id/runs/:run_id/xcom`
- [x] Size limit per XCom value (e.g., 64KB) to prevent abuse

**Why:** Real pipelines need data flow between tasks. "Extract" passes file paths to "Transform", "Validate" passes row counts to "Alert", etc.

### 6. Sensor Operators ✅
- [x] New task type: `sensor` (polls a condition at intervals)
- [x] `FileSensor` — wait for a file to appear at a path
- [x] `HttpSensor` — wait for an HTTP endpoint to return 200
- [x] `ExternalTaskSensor` — wait for another DAG's task to succeed
- [x] `SqlSensor` — wait for a SQL query to return rows
- [x] Configurable: `poke_interval`, `timeout`, `mode` (poke vs reschedule)

**Why:** Extremely common pattern. "Wait for upstream data to land, then process."

### 7. Webhook Notifications / Callbacks ✅
- [x] DAG-level callbacks: `on_success`, `on_failure`, `on_sla_miss`
- [x] Task-level callbacks: `on_failure`, `on_retry`
- [x] Webhook delivery (POST JSON to a URL)
- [x] Built-in Slack integration (incoming webhook URL)
- [x] Email integration (SMTP config)
- [x] Callback config stored in DAG metadata or global config

**Why:** If a DAG fails at 3 AM, somebody needs to know. Zero alerting = no enterprise adoption.

### 8. Task Pools / Concurrency Limits ✅
- [x] `pools` table: `(name, slots, description)`
- [x] Tasks can declare `pool = "database"` to share limited resources
- [x] Scheduler respects pool slot limits across all DAGs
- [x] REST API: CRUD for pools
- [x] UI: Pool management page
- [x] Default pool with configurable global limit

**Why:** Without this, 50 DAGs can all hammer the same database simultaneously. Pools prevent resource exhaustion.

---

## Phase 3 — Scale & Observability ✅ COMPLETE

> Wave 2 (Feb 2026) complete: Gantt UI, Calendar UI, and Audit Logging.
> Phase 3 completion (Feb 2026): Production PostgreSQL migrations, connection tuning, Prometheus metrics, and Grafana dashboard.

### 9. PostgreSQL Backend ✅
- [x] Abstract DB layer behind a trait (SQLite vs PostgreSQL) ✅ Done
- [x] `sqlx` or `diesel` with connection pooling ✅ Done (sqlx async unification)
- [x] Migration system (sqlx-migrate or refinery)
- [x] `--database-url` CLI flag for PostgreSQL connection string
- [x] Keep SQLite as default for dev/small deployments
- [x] Connection pool tuning (min/max connections, idle timeout)

**Why:** SQLite is single-writer. Past ~10 concurrent workers reporting results, you hit lock contention and dropped writes.

### 10. Prometheus Metrics Endpoint ✅
- [x] `GET /metrics` endpoint (Prometheus exposition format)
- [x] Metrics: `vortex_dags_total`, `vortex_tasks_running`, `vortex_tasks_failed_total`
- [x] Metrics: `vortex_task_duration_seconds` (histogram)
- [x] Metrics: `vortex_workers_active`, `vortex_queue_depth`
- [x] Metrics: `vortex_scheduler_heartbeat_timestamp`
- [x] Grafana dashboard template in `docs/grafana/`

**Why:** Every enterprise has Prometheus + Grafana. No metrics = invisible system = no trust.

### 11. Calendar + Gantt Views in UI ✅
- [x] Calendar heatmap — daily run states (green/red/yellow cells)
- [x] Gantt chart — horizontal timeline of task durations within a run
- [x] Date range picker for historical views
- [x] Click-through from calendar → run details

**Why:** Airflow's calendar and Gantt views are genuinely useful for spotting patterns (slow Tuesdays, frequent Friday failures, etc.).

### 12. Audit Logging ✅
- [x] `audit_log` table: `(timestamp, user, action, resource, details)`
- [x] Log: DAG triggers, pause/unpause, user creation/deletion
- [x] Log: Secret creation/deletion, source code edits
- [x] Log: Login attempts (success + failure)
- [x] REST API: `GET /api/audit` with filters (user, action, date range)
- [x] UI: Audit log viewer (Admin only)

**Why:** SOC2, HIPAA, and most compliance frameworks require audit trails. "Who did what, when?"

---

## Phase 4 — Ecosystem & Polish

> Nice-to-haves that differentiate VORTEX from a "toy" orchestrator.

### 13. Plugin / Provider System
- [x] Plugin trait: `VortexOperator { fn execute(&self, context: &TaskContext) -> Result<()> }`
- [x] Built-in providers: HTTP (`reqwest` base implementation)
- [x] Provider registry mapping task_type to operators
- [x] Dynamic loading via shared libraries (`.so` / `.dylib`) or Python plugins

### 14. Dynamic DAG Generation
- [x] Improve PyO3 parser to handle `for` loops generating tasks
- [x] Support parameterized DAGs (Jinja-like templating or Python f-strings)
- [x] `dag_factory` pattern: generate DAGs from YAML/JSON config
- [x] UI: Show "Dynamic" badge next to generated DAGs

### 15. Multi-Tenancy
- [x] `teams` table with DAG folder isolation
- [x] Per-team RBAC (team members only see their DAGs)
- [x] Resource quotas per team (max concurrent tasks, max DAGs)
- [x] Namespace-aware API (`/api/teams/:team/dags`)

### 16. DAG Version Diffing & Rollback
- [x] UI: Version history list with timestamps
- [x] Side-by-side diff view (old vs new source code)
- [x] One-click rollback to any previous version
- [x] Auto-snapshot on every source edit

### 17. Sub-DAGs / Task Groups
- [x] `TaskGroup` construct for organizing large DAGs
- [x] Collapsible groups in the visual graph
- [x] Group-level status aggregation (all green → group green)
- [x] Nested group support

### 18. CLI Tool (`vortex-cli`)
- [x] `vortex dags list` — list all DAGs
- [x] `vortex dags trigger <dag_id>` — trigger from terminal
- [x] `vortex dags pause/unpause <dag_id>`
- [x] `vortex tasks logs <instance_id>`
- [x] `vortex users create <username> --role Operator`
- [x] `vortex secrets set <key> <value>`
- [x] Auto-discovery of server URL from env or config file

### 19. Backfill Improvements
- [x] Date range backfill with parallel execution
- [x] Catchup mode (run all missed intervals since `start_date`)
- [x] Backfill progress tracking in UI
- [x] Dry-run mode (show what would be triggered)

### 20. Task Timeout Enforcement
- [x] Per-task `execution_timeout` config
- [x] Kill task process if timeout exceeded
- [x] Mark as `TimedOut` state (distinct from `Failed`)
- [x] Configurable default timeout (currently hardcoded 300s in executor)

---

## Tech Debt & Cleanup

- [x] Remove `#[allow(dead_code)]` annotations — either use or delete unused code
- [x] `BackfillRequest` struct fields are never read (web.rs)
- [x] `Scheduler::new()` and `Scheduler::run()` are never called (only `new_with_arc` + `run_with_trigger`)
- [x] `dag_to_json()` in python_parser.rs is never used
- [x] Several DB methods are unused: `get_dag_versions`, `worker_count`
- [x] Consolidate duplicate proto module definitions in `swarm.rs` and `worker.rs`
- [x] Add `#[cfg(test)]` module structure to all source files
- [x] CI/CD pipeline (GitHub Actions): build, test, lint, release binaries

---

*Last updated: 2026-02-28*
*Maintainer: Ashwin Vasireddy (@kiragawd)*
