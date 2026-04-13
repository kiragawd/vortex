# Observability & Data Governance

## Overview

Ryuo provides built-in observability through data lineage tracking, incident management integrations, distributed tracing, and Prometheus metrics.

**Modules:** `src/lineage.rs`, `src/incident.rs`, `src/telemetry.rs`, `src/metrics.rs`

---

## Data Lineage

OpenLineage-compliant event emission for tracking data flow across pipelines.

### Event Types

| Event | Description |
|-------|-------------|
| `START` | Task execution begins |
| `COMPLETE` | Task execution succeeds |
| `FAIL` | Task execution fails |
| `ABORT` | Task execution cancelled |

### Emitters

| Emitter | Transport | Description |
|---------|-----------|-------------|
| HTTP | `POST` to endpoint | Sends OpenLineage events to a lineage server (e.g., Marquez) |
| Log | Structured log | Emits events as structured JSON log entries |

### Event Structure

Each lineage event includes:
- **Run metadata** — Run ID, DAG ID, task ID, timestamps
- **Input datasets** — Source tables/files with schema facets
- **Output datasets** — Target tables/files with schema facets
- **Job facets** — SQL queries, processing details

### Database Schema

| Table | Purpose |
|-------|---------|
| `lineage_events` | Persisted lineage event records |
| `lineage_datasets` | Dataset definitions referenced by events |

### CLI

```bash
# Query lineage events
ryuo-cli lineage query --dag-id "etl_pipeline" --limit 50

# Export lineage data
ryuo-cli lineage export --format json --output lineage_export.json
```

---

## Incident Management

Integration with incident management platforms for automated alerting.

### PagerDuty

Full incident lifecycle support:

| Action | Description |
|--------|-------------|
| `trigger` | Create a new incident with severity and details |
| `acknowledge` | Mark incident as acknowledged |
| `resolve` | Close the incident |

**Configuration:**
- Routing key (integration key from PagerDuty service)
- Severity mapping from task failure types

### Opsgenie / Datadog

Configuration types are defined for both platforms. HTTP implementations are pending.

### Database Schema

| Table | Purpose |
|-------|---------|
| `incident_configs` | PagerDuty/Opsgenie/Datadog alert configuration per DAG |

---

## OpenTelemetry

W3C TraceContext propagation for distributed tracing across Ryuo components.

### Implemented

- **TraceContext parsing** — Extracts `traceparent` and `tracestate` headers from incoming requests
- **TraceContext serialization** — Propagates trace context to outgoing gRPC calls and HTTP requests
- **Span builders** — Create spans for scheduler, executor, and API operations

### Pending

- **OTLP Exporter** — HTTP/gRPC export to OpenTelemetry collectors (Jaeger, Tempo, Datadog APM)

### Trace Propagation Flow

```
HTTP Request (traceparent header)
  → Axum middleware extracts TraceContext
    → Scheduler creates child span
      → gRPC call to worker (traceparent injected)
        → Worker creates child span for task execution
```

---

## Prometheus Metrics

Built-in `/metrics` endpoint exposing Prometheus-format metrics.

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `ryuo_tasks_total` | Counter | Total tasks by state (queued, running, success, failed) |
| `ryuo_task_duration_seconds` | Histogram | Task execution duration |
| `ryuo_workers_active` | Gauge | Number of active workers |
| `ryuo_workers_total` | Gauge | Total registered workers |
| `ryuo_dag_runs_total` | Counter | Total DAG runs by state |
| `ryuo_queue_depth` | Gauge | Number of tasks in queue |

### Scrape Configuration

```yaml
# prometheus.yml
scrape_configs:
  - job_name: "ryuo"
    static_configs:
      - targets: ["localhost:3000"]
    scrape_interval: 15s
```

### Grafana Dashboard

A pre-built Grafana dashboard is available at `docs/grafana/ryuo-dashboard.json`. Import it into Grafana and point it at your Prometheus data source.

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) — System design and data flow
- [Deployment](./DEPLOYMENT.md) — Prometheus configuration
- [Resilience](./RESILIENCE.md) — Health monitoring and recovery
- [Events & Sensors](./EVENTS_SENSORS.md) — Event-driven architecture
