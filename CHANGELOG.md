# Changelog

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
- **Cloud-Native Distribution** — Dockerfile, docker-compose.yml, Helm chart (`helm/vortex/`)
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
- **TASK-4:** Fixed Prometheus scrape target from `localhost:3000` (Grafana) to `controller:8080` (Vortex metrics)
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
- **TASK-29:** Removed hardcoded default API key — now requires `VORTEX_API_KEY` env var (`pools.py`)
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
- **BUG-H14 / CORS:** `VORTEX_CORS_ORIGINS` env var enforces specific origin allowlist; `allow_origin(Any)` removed from production paths.
- **BUG-H13 / Rate Limiting:** Login rate limit key is now `(IP, username)` tuple — prevents bypass via IP rotation.
- **BUG-C7 / gRPC Auth:** Workers must supply `VORTEX_GRPC_AUTH_TOKEN` bearer token; unauthenticated gRPC connections rejected in non-dev mode.
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
- gRPC secret transmission requires TLS when `VORTEX_GRPC_TLS_CERT` is set.

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
- **DOC-2:** Standardized `VORTEX_BASE_URL` env var name (was `VORTEX_SERVER_URL` in CLI examples).
- **DOC-3:** Updated Kubernetes Executor status — pod spec generation and namespace validation implemented; pod API submission pending ENT-16.
- **DOC-4:** Clarified gRPC worker `--controller` URL format: `http://` for plaintext (Tonic HTTP/2), `https://` for TLS.
- **DOC-5:** Added API reference for approval workflows, API token management, data lineage, incident management, RBAC fine-grained roles, and IP allowlist endpoints.
- **DOC-6:** Added comprehensive environment variables reference table to `CONFIGURATION.md`.
- **DOC-7:** Added Python SDK connector examples (BigQuery, Snowflake, S3, PostgreSQL) and YAML DAG connector configuration examples.
- **DOC-8:** Added Glossary to `ARCHITECTURE.md` defining Controller, Worker, Swarm, DAG, Task Instance, DAG Run, XCom, Vault, Sensor, Pool, Backfill, Team, and Approval Workflow.
- **DOC-9:** Expanded Python SDK API reference in `PYTHON_INTEGRATION.md` — added module descriptions for `vortex.dag`, `vortex.task`, `vortex.xcom`, `vortex.secrets`, and `vortex.notifications`.
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
