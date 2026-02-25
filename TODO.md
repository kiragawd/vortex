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
- [x] REST API (22 endpoints)
- [x] Distributed workers (gRPC Swarm)
- [x] Auto-recovery on worker failure (health check loop)
- [x] Task logs (DB + filesystem)
- [x] Visual DAG dependency graph
- [x] DAG pause / unpause
- [x] Manual trigger + retry from failure
- [x] Run history with per-run task breakdown
- [x] In-browser DAG code editor with live re-parse
- [x] DAG file upload (multipart)

---

## Phase 1 — Security & Scheduling 🔥🔥🔥

> Must ship before any enterprise eval. Without these, VORTEX is a demo.

### 1. Cron Scheduler Loop (Auto-Trigger DAGs)
- [x] Tokio task that wakes every 60 seconds
- [x] Evaluate all DAGs with `schedule_interval` (cron expressions)
- [x] Compare `last_run` vs cron expression to decide if a run is due
- [x] Respect `is_paused`, `max_active_runs`, and `catchup` flags
- [x] Update `last_run` and `next_run` columns after each trigger
- [x] Support standard cron syntax + Airflow presets (`@daily`, `@hourly`, `@weekly`)

**Why:** Airflow's entire value prop is *automated* scheduling. Without this, VORTEX is trigger-only — nobody will take it seriously.

### 2. Password Hashing (Security Critical)
- [x] Replace plaintext `password_hash` storage with bcrypt or argon2
- [x] Hash password on user creation (`POST /api/users`)
- [x] Hash + compare on login (`POST /api/login`)
- [x] Migrate existing plaintext passwords on first startup
- [ ] Add password strength validation (min length, complexity)

**Why:** Storing passwords in plaintext is a CVE waiting to happen. Non-negotiable for any production system.

### 3. Structured Logging
- [x] Replace all `println!()` / `eprintln!()` with `tracing` crate
- [x] Add log levels: DEBUG, INFO, WARN, ERROR
- [x] JSON output format for log aggregation (ELK, Splunk, Datadog)
- [ ] File rotation via `tracing-appender`
- [ ] Request-level tracing with correlation IDs
- [x] Configurable log level via CLI flag (`--log-level info`)

**Why:** Enterprise ops teams need searchable, structured logs. `println!` doesn't cut it.

### 4. TLS / HTTPS Support
- [x] Accept `--tls-cert` and `--tls-key` CLI flags
- [x] Serve Axum over HTTPS when certs are provided
- [ ] gRPC TLS for controller ↔ worker communication
- [ ] Document cert generation with `openssl` / Let's Encrypt

**Why:** API keys are currently transmitted in plaintext over HTTP. Unacceptable for production.

---

## Phase 2 — Data Pipeline Essentials 🔥🔥

> Features required for real-world data pipelines (ETL, ML, analytics).

### 5. XCom (Inter-Task Data Passing)
- [ ] `task_xcom` table: `(dag_id, task_id, run_id, key, value, timestamp)`
- [ ] `xcom_push(key, value)` — callable from Python tasks via context
- [ ] `xcom_pull(task_id, key)` — retrieve upstream task output
- [ ] Inject upstream XCom values as env vars for bash tasks
- [ ] REST API: `GET /api/dags/:id/runs/:run_id/xcom`
- [ ] Size limit per XCom value (e.g., 64KB) to prevent abuse

**Why:** Real pipelines need data flow between tasks. "Extract" passes file paths to "Transform", "Validate" passes row counts to "Alert", etc.

### 6. Sensor Operators
- [ ] New task type: `sensor` (polls a condition at intervals)
- [ ] `FileSensor` — wait for a file to appear at a path
- [ ] `HttpSensor` — wait for an HTTP endpoint to return 200
- [ ] `ExternalTaskSensor` — wait for another DAG's task to succeed
- [ ] `SqlSensor` — wait for a SQL query to return rows
- [ ] Configurable: `poke_interval`, `timeout`, `mode` (poke vs reschedule)

**Why:** Extremely common pattern. "Wait for upstream data to land, then process."

### 7. Webhook Notifications / Callbacks
- [ ] DAG-level callbacks: `on_success`, `on_failure`, `on_sla_miss`
- [ ] Task-level callbacks: `on_failure`, `on_retry`
- [ ] Webhook delivery (POST JSON to a URL)
- [ ] Built-in Slack integration (incoming webhook URL)
- [ ] Email integration (SMTP config)
- [ ] Callback config stored in DAG metadata or global config

**Why:** If a DAG fails at 3 AM, somebody needs to know. Zero alerting = no enterprise adoption.

### 8. Task Pools / Concurrency Limits
- [ ] `pools` table: `(name, slots, description)`
- [ ] Tasks can declare `pool = "database"` to share limited resources
- [ ] Scheduler respects pool slot limits across all DAGs
- [ ] REST API: CRUD for pools
- [ ] UI: Pool management page
- [ ] Default pool with configurable global limit

**Why:** Without this, 50 DAGs can all hammer the same database simultaneously. Pools prevent resource exhaustion.

---

## Phase 3 — Scale & Observability 🔥

> Required for scaling beyond a single team / 100+ DAGs.

### 9. PostgreSQL Backend
- [ ] Abstract DB layer behind a trait (SQLite vs PostgreSQL)
- [ ] `sqlx` or `diesel` with connection pooling
- [ ] Migration system (sqlx-migrate or refinery)
- [ ] `--database-url` CLI flag for PostgreSQL connection string
- [ ] Keep SQLite as default for dev/small deployments
- [ ] Connection pool tuning (min/max connections, idle timeout)

**Why:** SQLite is single-writer. Past ~10 concurrent workers reporting results, you hit lock contention and dropped writes.

### 10. Prometheus Metrics Endpoint
- [ ] `GET /metrics` endpoint (Prometheus exposition format)
- [ ] Metrics: `vortex_dags_total`, `vortex_tasks_running`, `vortex_tasks_failed_total`
- [ ] Metrics: `vortex_task_duration_seconds` (histogram)
- [ ] Metrics: `vortex_workers_active`, `vortex_queue_depth`
- [ ] Metrics: `vortex_scheduler_heartbeat_timestamp`
- [ ] Grafana dashboard template in `docs/grafana/`

**Why:** Every enterprise has Prometheus + Grafana. No metrics = invisible system = no trust.

### 11. Calendar + Gantt Views in UI
- [ ] Calendar heatmap — daily run states (green/red/yellow cells)
- [ ] Gantt chart — horizontal timeline of task durations within a run
- [ ] Date range picker for historical views
- [ ] Click-through from calendar → run details

**Why:** Airflow's calendar and Gantt views are genuinely useful for spotting patterns (slow Tuesdays, frequent Friday failures, etc.).

### 12. Audit Logging
- [ ] `audit_log` table: `(timestamp, user, action, resource, details)`
- [ ] Log: DAG triggers, pause/unpause, user creation/deletion
- [ ] Log: Secret creation/deletion, source code edits
- [ ] Log: Login attempts (success + failure)
- [ ] REST API: `GET /api/audit` with filters (user, action, date range)
- [ ] UI: Audit log viewer (Admin only)

**Why:** SOC2, HIPAA, and most compliance frameworks require audit trails. "Who did what, when?"

---

## Phase 4 — Ecosystem & Polish

> Nice-to-haves that differentiate VORTEX from a "toy" orchestrator.

### 13. Plugin / Provider System
- [ ] Plugin trait: `VortexOperator { fn execute(&self, context: &TaskContext) -> Result<()> }`
- [ ] Dynamic loading via shared libraries (`.so` / `.dylib`) or Python plugins
- [ ] Built-in providers: AWS S3, GCP GCS, PostgreSQL, MySQL, HTTP
- [ ] Provider registry (similar to Airflow's `apache-airflow-providers-*`)
- [ ] Plugin discovery via `plugins/` directory

### 14. Dynamic DAG Generation
- [ ] Improve PyO3 parser to handle `for` loops generating tasks
- [ ] Support parameterized DAGs (Jinja-like templating or Python f-strings)
- [ ] `dag_factory` pattern: generate DAGs from YAML/JSON config
- [ ] UI: Show generated task count vs static

### 15. Multi-Tenancy
- [ ] `teams` table with DAG folder isolation
- [ ] Per-team RBAC (team members only see their DAGs)
- [ ] Resource quotas per team (max concurrent tasks, max DAGs)
- [ ] Namespace-aware API (`/api/teams/:team/dags`)

### 16. DAG Version Diffing & Rollback
- [ ] UI: Version history list with timestamps
- [ ] Side-by-side diff view (old vs new source code)
- [ ] One-click rollback to any previous version
- [ ] Auto-snapshot on every source edit

### 17. Sub-DAGs / Task Groups
- [ ] `TaskGroup` construct for organizing large DAGs
- [ ] Collapsible groups in the visual graph
- [ ] Group-level status aggregation (all green → group green)
- [ ] Nested group support

### 18. CLI Tool (`vortex-cli`)
- [ ] `vortex dags list` — list all DAGs
- [ ] `vortex dags trigger <dag_id>` — trigger from terminal
- [ ] `vortex dags pause/unpause <dag_id>`
- [ ] `vortex tasks logs <instance_id>`
- [ ] `vortex users create <username> --role Operator`
- [ ] `vortex secrets set <key> <value>`
- [ ] Auto-discovery of server URL from env or config file

### 19. Backfill Improvements
- [ ] Date range backfill with parallel execution
- [ ] Catchup mode (run all missed intervals since `start_date`)
- [ ] Backfill progress tracking in UI
- [ ] Dry-run mode (show what would be triggered)

### 20. Task Timeout Enforcement
- [ ] Per-task `execution_timeout` config
- [ ] Kill task process if timeout exceeded
- [ ] Mark as `TimedOut` state (distinct from `Failed`)
- [ ] Configurable default timeout (currently hardcoded 300s in executor)

---

## Tech Debt & Cleanup

- [ ] Remove `#[allow(dead_code)]` annotations — either use or delete unused code
- [ ] `BackfillRequest` struct fields are never read (web.rs)
- [ ] `Scheduler::new()` and `Scheduler::run()` are never called (only `new_with_arc` + `run_with_trigger`)
- [ ] `dag_to_json()` in python_parser.rs is never used
- [ ] Several DB methods are unused: `get_dag_versions`, `update_dag_last_run`, `update_dag_next_run`, `get_scheduled_dags`, `get_active_dag_run_count`, `worker_count`
- [ ] Consolidate duplicate proto module definitions in `swarm.rs` and `worker.rs`
- [ ] Add `#[cfg(test)]` module structure to all source files
- [ ] CI/CD pipeline (GitHub Actions): build, test, lint, release binaries

---

*Last updated: 2026-02-26*
*Maintainer: Ashwin Vasireddy (@kiragawd)*
