# Events & Sensors

## Overview

Vortex provides an event-driven architecture with an in-memory event bus, webhook ingestion, event-triggered DAG execution, and a sensor framework for polling external conditions.

**Modules:** `src/event_framework.rs`, `src/sensors.rs`

---

## Event Bus

Broadcast channel-based in-memory event log with flexible filter matching.

### Event Structure

Each event contains:
- **Source** — Origin identifier (e.g., `webhook.github`, `sensor.s3`, `dag.etl_pipeline`)
- **Event type** — Categorization string
- **Payload** — JSON data
- **Metadata** — Key-value tags for routing and filtering
- **Timestamp** — Event creation time

### Event Filters

Events can be matched using filter rules:

| Filter Type | Description | Example |
|------------|-------------|---------|
| Source glob | Glob pattern on event source | `webhook.*`, `sensor.s3.*` |
| JSON path | Condition on payload fields | `$.status == "completed"` |
| Metadata match | Key-value match on metadata tags | `environment: production` |

Filters are composable — multiple conditions are combined with AND logic.

---

## Webhook Receiver

HTTP endpoint for ingesting external events into the event bus.

External systems (GitHub, CI/CD pipelines, monitoring tools) can push events to Vortex via webhook:

```bash
curl -X POST http://localhost:3000/api/events/webhook \
  -H "Authorization: Bearer <api_key>" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "github.push",
    "event_type": "repository.push",
    "payload": {"ref": "refs/heads/main", "commits": 3},
    "metadata": {"repo": "data-pipelines"}
  }'
```

---

## Event-Triggered DAGs

DAGs can be configured to execute when incoming events match specified patterns.

**Flow:**
1. External event arrives via webhook or is emitted internally
2. Event bus routes the event to all registered subscribers
3. Subscribers evaluate filter rules against the event
4. Matching DAGs are triggered for execution

---

## Sensor Framework

Sensors are lightweight polling tasks that monitor external conditions and trigger downstream work when conditions are met.

### Sensor Modes

| Mode | Behavior |
|------|----------|
| **Poke** | Tight polling loop — sensor holds its slot and checks repeatedly at a configured interval |
| **Reschedule** | Release slot between checks — sensor frees its execution slot and is re-scheduled by the scheduler |

### Sensor Types

#### File Sensor

Monitor filesystem paths for existence or modification.

| Parameter | Description |
|-----------|-------------|
| `path` | File or directory path to monitor |
| `check_type` | `exists` or `modified_since` |
| `poke_interval` | Seconds between checks |

#### HTTP Sensor

Poll HTTP endpoints and match response conditions.

| Parameter | Description |
|-----------|-------------|
| `url` | HTTP endpoint to poll |
| `method` | HTTP method (default: `GET`) |
| `expected_status` | Expected HTTP status code |
| `response_pattern` | Optional regex match on response body |
| `poke_interval` | Seconds between checks |

#### SQL Sensor

Execute database queries and evaluate result conditions.

| Parameter | Description |
|-----------|-------------|
| `connection` | Database connection reference |
| `sql` | SQL query to execute |
| `condition` | `row_count > 0`, value match, etc. |
| `poke_interval` | Seconds between checks |

#### External Task Sensor

Wait for upstream DAG or task completion across DAG boundaries.

| Parameter | Description |
|-----------|-------------|
| `dag_id` | Upstream DAG identifier |
| `task_id` | Specific task to wait for (optional — waits for full DAG if omitted) |
| `execution_date` | Target execution date |
| `poke_interval` | Seconds between checks |

---

## Related Documentation

- [Scheduling](./SCHEDULING.md) — Dataset-triggered scheduling and cross-DAG dependencies
- [Architecture](./ARCHITECTURE.md) — System design and execution flow
- [Observability](./OBSERVABILITY.md) — Metrics and tracing
