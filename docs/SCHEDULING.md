# Scheduling & Data-Aware Orchestration

## Overview

Vortex provides a multi-strategy scheduling engine covering cron-based schedules, dataset-triggered execution, cross-DAG dependencies, and dynamic task mapping.

**Modules:** `src/scheduler.rs`, `src/advanced_scheduler.rs`

---

## Cron-based Scheduling

Standard cron expressions and presets for time-based DAG execution:

| Preset | Cron Expression | Description |
|--------|----------------|-------------|
| `@daily` | `0 0 * * *` | Once per day at midnight |
| `@hourly` | `0 * * * *` | Once per hour |
| `@weekly` | `0 0 * * 0` | Once per week on Sunday |
| `@monthly` | `0 0 1 * *` | First of each month |
| `@yearly` | `0 0 1 1 *` | January 1st each year |
| Custom | `*/15 * * * *` | Any valid cron expression |

**Validation:** `normalize_schedule` validates cron expressions at DAG registration time, rejecting invalid expressions before they enter the scheduler loop.

### Example

```python
from vortex import DAG, BashOperator

with DAG("etl_daily", schedule_interval="@daily") as dag:
    extract = BashOperator(task_id="extract", bash_command="echo 'extracting'")
    transform = BashOperator(task_id="transform", bash_command="echo 'transforming'")
    extract >> transform
```

---

## Dataset-Triggered Scheduling

DAGs can be triggered by upstream dataset updates instead of (or in addition to) cron schedules.

**Key types:**
- `Dataset` — Named data asset with a URI identifier
- `DatasetEvent` — Records when a dataset was last updated
- `DatasetTrigger` — Links a DAG to one or more datasets with trigger conditions

### Trigger Modes

| Mode | Behavior |
|------|----------|
| **All** | DAG triggers only when **all** referenced datasets have been updated |
| **Any** | DAG triggers when **any** referenced dataset is updated |

### Database Schema

| Table | Purpose |
|-------|---------|
| `datasets` | Dataset definitions (name, URI, metadata) |
| `dataset_events` | Event log of dataset updates |
| `dataset_triggers` | DAG-to-dataset trigger mappings |

### How It Works

1. A task completes and reports a dataset update event
2. `DatasetScheduler` evaluates all triggers referencing that dataset
3. In **All** mode, checks whether all required datasets have fresh events
4. In **Any** mode, triggers immediately on any matching event
5. Matching DAGs are automatically triggered for execution

---

## Cross-DAG Dependencies

DAGs can declare dependencies on upstream DAGs, preventing execution until dependencies complete.

| Table | Purpose |
|-------|---------|
| `cross_dag_dependencies` | Upstream/downstream DAG dependency mappings |

**Behavior:**
- Before scheduling a downstream DAG run, the scheduler checks that all upstream DAGs have completed their most recent run
- Supports multiple upstream dependencies per DAG
- Integrates with the External Task Sensor for real-time polling

---

## Dynamic Task Mapping

Runtime task fan-out where a single task template expands into multiple parallel task instances based on dynamic input.

| Table | Purpose |
|-------|---------|
| `task_map_templates` | Dynamic task mapping configuration |

**Expand/Reduce logic:**
- **Expand** — A task template generates N parallel task instances at runtime (e.g., one per partition, file, or input record)
- **Reduce** — Downstream tasks wait for all expanded instances to complete before proceeding

**Status:** Expand/reduce logic is implemented. Full scheduler integration is pending.

---

## Dependency Orchestration

The core scheduler uses topological sorting with in-degree tracking for dependency-aware execution:

1. Build in-degree map from DAG task dependencies
2. Enqueue all tasks with in-degree 0 (no dependencies)
3. When a task completes, decrement downstream in-degrees
4. Tasks reaching in-degree 0 are enqueued for execution
5. Continue until all tasks finish or a failure is detected

> **Concurrency note:** The in-degree map is protected by a `tokio::sync::Mutex` (not `std::sync::Mutex`). This is intentional — the guard is held briefly across `.await` points, and using a sync mutex would block the Tokio worker thread.

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) — Execution flow and failure scenarios
- [Events & Sensors](./EVENTS_SENSORS.md) — Event-triggered DAGs and external task sensors
- [Python Integration](./PYTHON_INTEGRATION.md) — DAG authoring with Python
