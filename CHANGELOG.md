# Changelog

## [0.8.1] - 2026-04-19 — Security & Bug Fix Release

### Security Fixes
- SAML authentication: signature validation now rejects unsigned assertions by default. Set `RYUO_SAML_ALLOW_UNVERIFIED=true` to allow (dev only).
- OIDC auto-provisioned users now have `password_change_required=true` — must change password on first local login.
- PKCE store now has 10-minute TTL with automatic pruning.
- All RBAC, token, IP allowlist, pool, retention, incident, and compliance endpoints now require Admin role.
- API token hashing upgraded from SipHash to SHA-256.
- Webhook HMAC validation uses constant-time comparison (prevents timing attacks).
- SQL injection prevention: all connector SQL is validated via `sqlparser` (SELECT-only whitelist).
- SSRF protection: webhook/notification URLs validated against private IP ranges.
- CI pipeline shell commands validated against dangerous patterns.
- Secret masking in compliance module now always returns `****`.

### Bug Fixes
- Fixed scheduler deadlock when >100 downstream tasks need skipping (channel buffer sized to task count).
- Fixed pool slot enforcement in local execution path.
- Fixed `cross_dag_check_upstream_completed` querying wrong column (`status` → `state`).
- Fixed `find_api_token_by_hash` to filter by token hash instead of scanning all tokens.
- Fixed `truncate_log` panic on multi-byte UTF-8 characters.
- Fixed `HttpOperator` missing 30s request timeout.
- Fixed `mark_stale_workers_offline` TOCTOU with single `UPDATE RETURNING`.
- Fixed schema version increment to use atomic operation.
- Fixed N+1 query patterns in dataset checks.
- Added LIMIT 1000 to unbounded queries.
- Fixed disaster recovery stubs to return `Err("not implemented")`.
- Fixed `export_environment` to walk full inheritance chain with cycle detection.
- Fixed `FileWatchSensor` false positive on first poll.
- Fixed notification dispatch to cap at 10 concurrent requests.
- Fixed Redshift connector to cache connection pool via LRU.
- Fixed incident provider HTTP clients to use 30s timeout.
- Fixed lineage emitter to use 10s timeout and error on non-2xx responses.
- Fixed Opsgenie dedup_key URL encoding.
- Fixed Snowflake REST pagination bounded to 100 pages.
- Fixed `pause_dag` to propagate DB errors.
- Fixed rate limiter key to use username.
- Added rate limiter HashMap pruning.

### Database
- Migration 019: Schema hardening — UNIQUE on `dag_versions(dag_id,version)`, FK constraints, CHECK constraints on state/role columns, indexes.
- CHECK constraints include `Active` for `workers.state` and `Queued` for `dag_runs.state`.

### CLI
- Added `validate_identifier()` on key CLI commands.
- Added audit logging on secret/user/team operations.
- Exit code 1 for not-found results.

### Breaking Changes
- SAML auth rejected by default without `RYUO_SAML_ALLOW_UNVERIFIED=true`.
- `get_interrupted_tasks` return type changed to 4-tuple (includes `run_id`).
- `token_has_scope` wildcard matching tightened.
- Snowflake `with_keypair_auth` now takes 3 args (user, key, passphrase: `Option<&str>`).

---

## [0.8.0] - Agentic Data-Aware Orchestration Release

### Added

#### Agentic CLI (38 new CLI command groups)
- **XCom CLI** — `ryuo xcom push/pull/list` for inter-task data exchange
- **Dataset Event CLI** — `ryuo dataset event emit` with downstream trigger reporting
- **DAG Runs CLI** — `ryuo dag runs <id>` with `--state` filter
- **DAG Create YAML** — `ryuo dag create --from-yaml` with `--dry-run` validation
- **JSON Output Mode** — Global `--output json` flag on all commands
- **Config Overrides** — `ryuo dag trigger <id> --config '{"key":"val"}'`
- **DAG Backfill** — `ryuo dag backfill` with `--interval`, `--dry-run`, 10K safety cap
- **Task Logs** — `ryuo task logs <id> --tail N`
- **Event Trigger CRUD** — `ryuo event trigger create/list/delete`
- **Sensor Status** — `ryuo sensor list` for sensor task instances
- **Connector Query** — `ryuo connector query <name> --sql "..."` (SELECT-only via sqlparser)

#### Data-Aware Operations
- **Queue Management** — `ryuo queue list/reprioritize` for priority-based task scheduling
- **Dataset Freshness** — `ryuo dataset freshness --uri/--stale-after`
- **Schema Change Detection** — `ryuo dataset schema store/diff` with automatic diff
- **Data Volume Stats** — `ryuo dataset stats --uri`
- **Dynamic Task Mapping** — Fan-out scheduler with 1000-task safety cap
- **Data Profiling** — `ryuo profile postgres --table <name>` (row count, null %, distinct, min/max)
- **Anomaly Detection** — `ryuo sensor check-anomaly --sql --baseline --sigma`

#### Safety & Governance
- **DAG Validation** — `ryuo validate <file>` for YAML/JSON cycle+structure checks
- **Approval Gates** — `ryuo approval request/list/approve/reject`
- **Rate Limiting** — `ryuo rate-limit check/status` with sliding window
- **Mutation Audit** — `--reason` flag on all state-changing commands
- **DAG Versioning** — `ryuo dag versions/rollback` for version history
- **Input Hardening** — Command injection prevention, identifier validation, path sanitization

#### Agent Integration
- **Agent State Store** — `ryuo agent state get/set/list/delete` with TTL
- **Agent Decision Log** — `ryuo agent log insert/query` with structured context
- **Event Watch** — `ryuo event recent/watch` with poll-based event stream
- **Inter-Agent Events** — `ryuo event publish/custom` for agent-to-agent communication
- **MCP Tool Server** — `ryuo mcp tools/describe` exposing 12 operations as LLM-callable tools
- **Agent-Scoped Tokens** — `ryuo token create --scope-rule "dag:etl_*:trigger,read"`

#### Infrastructure Connectors
- **K8s Executor CLI** — `ryuo k8s status/pods/logs/config` via REST API
- **Kafka Connector** — `ryuo kafka topics/produce/consume` via REST Proxy
- **S3/GCS Storage** — `ryuo storage ls/stat/freshness` for object storage
- **Delta Lake** — `ryuo delta-lake info/schema/history` via _delta_log parsing

#### Production Hardening
- **Health Endpoint** — `ryuo health` deep check (DB, workers, queue, datasets)
- **DR Backup** — `ryuo backup create/list/info` with real pg_dump
- **Swarm Status (real)** — `ryuo swarm status/workers` queries worker table
- **Connector Health (real)** — `ryuo connector health <name>` tests connectivity

### Database Migrations
- `011_event_triggers.sql` — Event triggers table
- `012_task_priority.sql` — Task priority + scheduler state
- `013_dataset_schemas.sql` — Dataset schema tracking
- `014_approval_gates.sql` — Approval request workflow
- `015_rate_limits.sql` — Rate limit counters
- `016_agent_state.sql` — Agent state + decision logs
- `017_custom_events.sql` — Custom events for inter-agent communication
- `018_token_scopes.sql` — Token scope rules + expiry

### New Modules
- `src/mcp_server.rs` — MCP tool definitions and server

---

## [0.7.0] - Platform Release

### Added

#### Security & Access Control
- **IAM** — SSO/OIDC/SAML/LDAP authentication middleware (`src/auth.rs`)
- **RBAC** — Role-based access control, API token scoping, IP allowlisting (`src/rbac.rs`)
- **Compliance** — Audit logging, approval workflows, retention engine, compliance tracker (`src/compliance.rs`)

#### Observability & Governance
- **Data Lineage** — OpenLineage-compliant data lineage tracking (`src/lineage.rs`)
- **Incident Management** — PagerDuty/Opsgenie/Datadog integration (`src/incident.rs`)
- **OpenTelemetry** — W3C TraceContext propagation, OTLP export, APM metrics (`src/telemetry.rs`)

#### Scheduling & Orchestration
- **Advanced Scheduling** — Dataset-triggered scheduling, cross-DAG dependencies, dynamic task mapping (`src/advanced_scheduler.rs`)
- **Event-Driven Architecture** — Event bus, webhook receiver, sensor registry (`src/event_framework.rs`)
- **Kubernetes Executor** — Pod-per-task isolation (`src/k8s_executor.rs`)

#### Connectors & Integrations
- **Cloud Connectors** — BigQuery, Redshift, Kafka, S3, GCS, Delta Lake (`src/cloud_connectors.rs`)
- **OpenAPI** — OpenAPI 3.1 spec generation with utoipa annotations (`src/openapi.rs`)
- **Developer SDK** — Plugin SDK scaffold CLI, marketplace, DAG test harness (`src/sdk.rs`)

#### Infrastructure & Operations
- **Cloud-Native Distribution** — Dockerfile, docker-compose.yml, Helm chart (`helm/ryuo/`)
- **DevOps & CI/CD** — Git-sync, CI pipeline generation, workspace federation (`src/devops.rs`)
- **Config Management** — Environment-scoped config with inheritance, feature flags, health checks, maintenance windows (`src/config_ops.rs`)
- **Disaster Recovery** — Backup/restore, failover orchestration, chaos testing engine, recovery automation (`src/disaster_recovery.rs`)

#### UI & Frontend
- **Web UI** — React 18 + TypeScript + Vite 5 SPA with dark/light mode, 8 pages (`ui/`)

#### Migration
- **Legacy Migration** — TWS and Autosys JIL parsers, migration converter, Rust/Python code generators (`src/migration.rs`)

### Changed
- **UI rewrite:** Migrated from Vanilla JS to React 18/TypeScript/Vite 5 with Tailwind CSS
- **Dark mode:** Full dark/light theme toggle with persistence, applied across all components and pages
- **SPA routing:** `static_handler` in `web.rs` now serves `index.html` for client-side routes (SPA fallback)
- **TWS parser fix:** Fixed indentation detection bug where continuation lines were misidentified as new jobs

### Tests
- 131 unit tests (inline `#[cfg(test)]` modules) — all passing
- 38 new integration tests across 3 test files:
  - `tests/migration_tests.rs` — 10 tests for TWS/Autosys parsing, conversion, code generation
  - `tests/disaster_recovery_tests.rs` — 10 tests for backup, failover, chaos, recovery
  - `tests/config_ops_tests.rs` — 18 tests for config, feature flags, health, maintenance
- 10 Playwright E2E test suites (existing) + 1 new dark mode/routing test suite
- Total: 269+ Rust tests, 0 failures

## [0.7.1] - 2026-03-27 — Security & Reliability Audit

### Fixed — Critical
- **TASK-1:** Clarified W3C traceparent span_id handling in `telemetry.rs` — renamed shadowed variable for clarity
- **TASK-2:** Added input validation for git repo URLs and branch names in `devops.rs` — prevents command injection, strips credentials from error messages
- **TASK-3:** Added SQL injection protection in `sensors.rs` — rejects multi-statement queries in SQL sensor
- **TASK-4:** Fixed Prometheus scrape target from `localhost:3000` (Grafana) to `controller:8080` (Ryuo metrics)
- **TASK-5:** LDAP auth provider now returns error instead of silently granting "Viewer" access (`auth.rs`)

### Fixed — High Severity
- **TASK-6:** Required secrets now validated before task dispatch — missing/failed secrets fail the task immediately (`swarm.rs`)
- **TASK-7:** Added documentation warning about regex-based SAML attribute extraction limitations (`auth.rs`)
- **TASK-8:** Added debug logging when IP allowlist is empty (open-by-default behavior documented) (`rbac.rs`)
- **TASK-9:** Fixed TOCTOU race condition in metrics gauge decrements — removed get-then-dec pattern (`metrics.rs`)
- **TASK-10:** Added bounds-checked JSON access for OpenAI and Anthropic API responses (`agentic.rs`)
- **TASK-11:** Added stub warnings to unimplemented cloud connectors (Kafka, S3) (`cloud_connectors.rs`)
- **TASK-12:** Fixed K8s pod name sanitization — uses `to_ascii_lowercase()`, ensures valid K8s name (`k8s_executor.rs`)
- **TASK-13:** Added stub documentation warnings to K8s executor submit/status methods (`k8s_executor.rs`)

### Fixed — Medium Severity
- **TASK-14:** Added stub warning to backup manager create_backup (`disaster_recovery.rs`)
- **TASK-15:** Fixed failover manager RwLock race condition — scoped write locks properly (`disaster_recovery.rs`)
- **TASK-16:** Email notification failures now propagate to callers instead of being swallowed (`notifications.rs`)
- **TASK-17:** Added warning log when task timeout is lost during re-queue (`swarm.rs`)
- **TASK-18:** Added warning log when config inheritance depth limit is exceeded (`config_ops.rs`)
- **TASK-19:** Renamed `_libraries` → `loaded_libraries` in PluginRegistry to prevent accidental removal (`executor.rs`)
- **TASK-20:** Documented Autosys negation operator limitation in migration parser (`migration.rs`)

### Fixed — Infrastructure
- **TASK-21:** Added worker healthcheck to docker-compose.yml
- **TASK-22:** Added readiness/liveness probes to Helm worker deployment
- **TASK-23:** Added plugins volume mount to Helm worker deployment
- **TASK-24:** Added startupProbe to Helm controller StatefulSet for migration tolerance
- **TASK-25/26:** New migration adding indexes on `api_tokens.expires_at`, `task_instances.execution_date`, and UNIQUE on `retention_policies.target_table`

### Fixed — Python SDK
- **TASK-29:** Removed hardcoded default API key — now requires `RYUO_API_KEY` env var (`pools.py`)
- **TASK-30:** Added `timeout=30` to all urllib HTTP calls (`xcom.py`, `pools.py`, `notifications.py`)
- **TASK-31:** Added thread lock around DAG registry mutations (`airflow_shim.py`)

### Fixed — Tests
- **TASK-28:** DB tests now print explicit skip message when `DATABASE_URL` not set (`db_tests.rs`)

## [Unreleased]

### Added
- Static Airflow parser module for DAG/task/dependency extraction.

---

## [0.7.2] - 2026-04-01 — Documentation & Security Hardening

### Security
- **SEC-1 / Vault KDF:** Vault master key now processed via Argon2id KDF before use as AES-256 key material — replaces direct raw-byte usage.
- **BUG-H8 / PKCE:** OIDC authentication flow now requires PKCE (Proof Key for Code Exchange) for all authorization code exchanges — mitigates auth code interception attacks.
- **BUG-C2 / SAML Signatures:** SAML response processing now validates XML digital signatures; regex-only attribute extraction replaced with signature-verified assertion parsing.
- **SEC-11 / Timing Attack:** Login endpoint uses constant-time comparison and performs a dummy bcrypt hash for non-existent usernames — prevents user enumeration via timing differences.
- **BUG-H14 / CORS:** `RYUO_CORS_ORIGINS` env var enforces specific origin allowlist; `allow_origin(Any)` removed from production paths.
- **BUG-H13 / Rate Limiting:** Login rate limit key is now `(IP, username)` tuple — prevents bypass via IP rotation.
- **BUG-C7 / gRPC Auth:** Workers must supply `RYUO_GRPC_AUTH_TOKEN` bearer token; unauthenticated gRPC connections rejected in non-dev mode.
- **BUG-C5 / Multi-Tenant Isolation:** All data-returning endpoints filter by `auth_user.team_id`; XCom, secrets, runs, tasks, and DAGs are team-scoped.

### Fixed — Critical Transactions (BUG-C3, BUG-M1, BUG-M3, BUG-M12)
- Multi-step database writes wrapped in `pool.begin()` / `tx.commit()` transactions.
- DELETE+INSERT patterns migrated to UPSERT or explicit transactions.
- TOCTOU race in count-then-INSERT patterns resolved with INSERT-first + uniqueness constraints.

### Fixed — Input Validation (BUG-C6, BUG-H7, BUG-H11)
- User-supplied SQL (sensor queries) validated with `sqlparser` — SELECT-only enforcement.
- SQL LIKE patterns escape `%` and `_` from user input before DB queries.
- File path parameters (`dag_id`, `task_id`) sanitized to `[a-zA-Z0-9_-]` pattern.

### Fixed — Secret Handling (BUG-M2, BUG-M6, SEC-10)
- Secrets no longer logged to task events or stdout.
- In-memory secrets use `secrecy::SecretString` with zeroize-on-drop.
- gRPC secret transmission requires TLS when `RYUO_GRPC_TLS_CERT` is set.

### Fixed — Event & Metric Completeness (BUG-H1, BUG-H4)
- `log_task_event()` now includes non-empty `run_id`, `dag_id`, and `task_id` fields.
- Prometheus gauge `dec()` guarded by `gauge.get() > 0` check — prevents negative gauge values.

### Fixed — Error Handling (BUG-H2, BUG-H3, BUG-M7)
- Failed tasks are re-queued or marked Failed — no silent drops.
- `execution_timeout_secs` propagated through retry paths (no longer reset to 0 on retry).
- DB error vs. no-result distinguished via `Result<Option<T>>` return types.

### Fixed — Performance (PERF-*)
- All list endpoints enforce LIMIT/pagination — no unbounded queries.
- Batch fetches for collections — N+1 query patterns eliminated.
- In-memory collections bounded by configurable max-size limits.
- DELETE operations use LIMIT in subqueries for large tables.

### Fixed — Database Schema (DB-*)
- Foreign key constraints added to all reference columns.
- Composite UNIQUE constraints added to identity tuples.
- Indexes added for common query patterns.
- NOT NULL and CHECK constraints added where appropriate.
- All new migrations are idempotent (IF NOT EXISTS, CREATE OR REPLACE).

### Documentation (DOC-1 through DOC-11)
- **DOC-1:** Clarified port assignments: port 3000 = REST API + web UI + `/metrics`; port 50051 = gRPC swarm; port 9090 = Prometheus server. Fixed incorrect port 8080 references.
- **DOC-2:** Standardized `RYUO_BASE_URL` env var name (was `RYUO_SERVER_URL` in CLI examples).
- **DOC-3:** Updated Kubernetes Executor status — pod spec generation and namespace validation implemented; pod API submission pending ENT-16.
- **DOC-4:** Clarified gRPC worker `--controller` URL format: `http://` for plaintext (Tonic HTTP/2), `https://` for TLS.
- **DOC-5:** Added API reference for approval workflows, API token management, data lineage, incident management, RBAC fine-grained roles, and IP allowlist endpoints.
- **DOC-6:** Added comprehensive environment variables reference table to `CONFIGURATION.md`.
- **DOC-7:** Added Python SDK connector examples (BigQuery, Snowflake, S3, PostgreSQL) and YAML DAG connector configuration examples.
- **DOC-8:** Added Glossary to `ARCHITECTURE.md` defining Controller, Worker, Swarm, DAG, Task Instance, DAG Run, XCom, Vault, Sensor, Pool, Backfill, Team, and Approval Workflow.
- **DOC-9:** Expanded Python SDK API reference in `PYTHON_INTEGRATION.md` — added module descriptions for `ryuo.dag`, `ryuo.task`, `ryuo.xcom`, `ryuo.secrets`, and `ryuo.notifications`.
- **DOC-10:** Added detailed troubleshooting subsections for DAG parsing errors, task timeouts, worker crash loops, and Prometheus scraping issues.
- **DOC-11:** Added this changelog entry documenting all 133+ audit fixes.
- DAG code generator and migration report writer.
- Enterprise connector abstraction and connector registry.
- Initial connector implementations for Postgres, Snowflake, Databricks, dbt, MySQL, and MS SQL.
- Agentic migration foundation (LLM provider interface, Python-to-Rust loop, dbt manifest conversion).
- CLI `migrate` command for Airflow-to-Rust conversion.
- Migration and connector API documentation.

## [0.6.0] - Existing baseline
- Existing scheduler, executor, web API, PostgreSQL backend, and Python compatibility layers.
