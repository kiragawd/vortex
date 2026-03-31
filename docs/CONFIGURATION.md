# Configuration Management & Operational Tooling

## Overview

Vortex provides environment-scoped configuration management, feature flags, health checks, and Git-Sync for DAG repository synchronization.

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
vortex-cli config list --environment prod

# Get a specific config value
vortex-cli config get --key "scheduler.max_parallel_tasks" --environment prod

# Set a config value
vortex-cli config set --key "scheduler.max_parallel_tasks" --value "32" --environment prod
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
2. Vortex periodically pulls the repository to the `dags/` directory
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
