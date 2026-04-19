# Configuration Management & Operational Tooling

## Overview

Ryuo provides environment-scoped configuration management, feature flags, health checks, and Git-Sync for DAG repository synchronization.

**Modules:** `src/config_ops.rs`, `src/devops.rs`

---

## Configuration Manager

Environment-scoped configuration with inheritance resolution.

### Environments

| Environment | Description |
|------------|-------------|
| `dev` | Local development configuration |
| `staging` | Pre-production environment |
| `prod` | Production environment |

### Inheritance

Configuration values cascade from less specific to more specific environments. A `prod` config inherits from `staging`, which inherits from `dev`. More specific environments override inherited values.

### CLI

```bash
# List configuration values
ryuo-cli config list --environment prod

# Get a specific config value
ryuo-cli config get --key "scheduler.max_parallel_tasks" --environment prod

# Set a config value
ryuo-cli config set --key "scheduler.max_parallel_tasks" --value "32" --environment prod
```

---

## Feature Flags

Boolean feature flag management with `RwLock`-protected in-memory store.

Feature flags allow enabling/disabling capabilities at runtime without restarts:

| Flag | Description |
|------|-------------|
| `dataset_triggers` | Enable dataset-triggered scheduling |
| `dynamic_task_mapping` | Enable runtime task fan-out |
| `lineage_emission` | Enable OpenLineage event emission |
| `approval_workflows` | Require approval gates for DAG changes |

Feature flags are stored in-memory with read-write lock protection for concurrent access.

---

## Health Checks

Operational health check types and maintenance window definitions.

### Health Endpoint

```bash
curl http://localhost:3000/health
```

```json
{"status": "ok", "version": "v0.6.0", "db": "connected"}
```

| Status | Meaning |
|--------|---------|
| `ok` | All systems operational |
| `degraded` | Partial functionality (e.g., DB disconnected) |

### Maintenance Windows

Define scheduled maintenance periods during which alerts are suppressed and DAG scheduling may be paused.

---

## Git-Sync

DAG repository synchronization for team-based DAG management.

### How It Works

1. Configure a Git repository URL and branch
2. Ryuo periodically pulls the repository to the `dags/` directory
3. New or updated DAG files are automatically loaded by the scheduler

### Authentication

| Method | Description |
|--------|-------------|
| SSH key | Private key for SSH-based repository access |
| HTTP basic | Username/password for HTTPS repositories |
| Token | Personal access token or deploy token |

### Sync States

| State | Description |
|-------|-------------|
| `Idle` | No sync in progress |
| `Syncing` | Repository pull in progress |
| `Success` | Last sync completed successfully |
| `Failed` | Last sync encountered an error |
| `Disabled` | Git-Sync is turned off |

### Safety

- Repository URLs are validated before use
- Branch names are sanitized to prevent injection
- Pull operations use `--depth 1` for shallow clones when possible

---

## Related Documentation

- [Deployment](./DEPLOYMENT.md) — Environment variables and server configuration
- [Scheduling](./SCHEDULING.md) — Scheduler configuration and dataset triggers
- [Architecture](./ARCHITECTURE.md) — System design overview

---

## Environment Variables Reference

Complete reference for all supported environment variables.

### Required

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | required | PostgreSQL connection URL (`postgres://user:pass@host/db`) |

### Security & Vault

| Variable | Default | Description |
|----------|---------|-------------|
| `RYUO_SECRET_KEY` | none (vault disabled) | AES-256-GCM vault master key — must be exactly 32 bytes. Without it, secret storage is disabled. |

### Server & API

| Variable | Default | Description |
|----------|---------|-------------|
| `RYUO_BASE_URL` | `http://localhost:3000` | Base URL injected into task processes as `RYUO_BASE_URL` for API callbacks |
| `RYUO_TASK_API_KEY` | none | Scoped API key injected into task processes for task-to-server API access |
| `RYUO_NODE_ID` | auto-generated UUID | Unique identifier for this controller node (used in HA mode) |

### gRPC / Swarm

| Variable | Default | Description |
|----------|---------|-------------|
| `RYUO_GRPC_AUTH_TOKEN` | none | Bearer token workers must supply when connecting to the controller gRPC endpoint |
| `RYUO_GRPC_TLS_CERT` | none | Path to PEM TLS certificate for gRPC mTLS |
| `RYUO_GRPC_TLS_KEY` | none | Path to PEM TLS private key for gRPC mTLS |
| `RYUO_GRPC_TLS_CA` | none | Path to PEM CA certificate — when set, workers enable mTLS validation against this CA |

### CORS & Rate Limiting

| Variable | Default | Description |
|----------|---------|-------------|
| `RYUO_CORS_ORIGINS` | none (same-origin only) | Comma-separated list of allowed CORS origins. Set to `*` for open access (dev only — logs a warning). |
| `RYUO_RATE_LIMIT_MAX` | `100` | Maximum requests per rate-limit window per `(IP, username)` key |
| `RYUO_RATE_LIMIT_WINDOW` | `60` | Rate-limit window in seconds |

### Authentication

| Variable | Default | Description |
|----------|---------|-------------|
| `RYUO_SAML_ALLOW_UNVERIFIED` | `false` | Allow unverified SAML signatures. **Dev only** — do not enable in production. |

### Python Execution

| Variable | Default | Description |
|----------|---------|-------------|
| `RYUO_PYTHON_TIMEOUT` | `30` | Python task execution timeout in seconds |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | none | Set to `1` when using Python 3.14+ for PyO3 ABI compatibility |
| `RYUO_ALLOW_PYTHON_DAGS` | `false` | Allow Python DAG loading via PyO3. When disabled, only YAML/JSON DAGs are loaded. |

### Observability

| Variable | Default | Description |
|----------|---------|-------------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | none | OTLP HTTP/gRPC endpoint for distributed tracing export (e.g., `http://jaeger:4318`) |

### Agentic Migration

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_KEY` | none | API key for OpenAI LLM provider (used by `ryuo-cli migrate --agentic --llm-provider openai`) |
| `OPENAI_ENDPOINT` | OpenAI default | Custom OpenAI-compatible API base URL |
| `ANTHROPIC_API_KEY` | none | API key for Anthropic LLM provider |
| `ANTHROPIC_ENDPOINT` | Anthropic default | Custom Anthropic-compatible API base URL |
