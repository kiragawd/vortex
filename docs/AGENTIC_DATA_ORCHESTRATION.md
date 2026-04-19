# Agentic Data-Aware Orchestration

## Overview

Ryuo is a **data-aware agentic orchestration engine** — built for autonomous agents, LLM-based pipelines, and data systems that sense data changes, reason about lineage, create and modify pipelines, and trigger complex workflows without human intervention.

Every capability is exposed as a CLI command (`--output json`), a REST endpoint, and an MCP tool — giving agents a tight sense → decide → act → observe loop with sub-millisecond overhead.

**Modules:** `src/agentic.rs`, `src/mcp_server.rs`, `src/advanced_scheduler.rs`, `src/event_framework.rs`, `src/sensors.rs`, `src/lineage.rs`, `src/executor.rs`, `src/connectors.rs`, `src/cloud_connectors.rs`

---

## What Is Agentic Data-Aware Orchestration?

Traditional schedulers run DAGs on cron timers. Data-aware orchestration goes further:

| Paradigm | Trigger | Decision Logic | Example |
|----------|---------|---------------|---------|
| **Time-based** | Cron schedule | None — runs on a clock | "Run ETL at midnight" |
| **Data-aware** | Dataset updated | Rules engine | "Run transform when both source tables are fresh" |
| **Event-driven** | External event | Filter + routing | "Run pipeline when S3 object lands" |
| **Agentic** | Any signal | LLM / ML model reasoning | "Detect data drift, decide which retraining DAG to trigger, allocate resources based on data volume" |

Ryuo supports all four paradigms, with the agentic layer building on top of the data-aware and event-driven foundations.

---

## Existing Capabilities

### 1. Dataset-Triggered Scheduling ✅

**Module:** `src/advanced_scheduler.rs`

DAGs can be triggered when upstream datasets are updated, replacing rigid cron schedules with data-dependency awareness.

```
Producer DAG completes → DatasetEvent emitted → DatasetScheduler evaluates triggers → Consumer DAG starts
```

**Key components:**

| Component | Description | Status |
|-----------|-------------|--------|
| `Dataset` | Named data asset with URI identifier (e.g., `s3://bucket/table`) | ✅ Implemented |
| `DatasetEvent` | Records dataset updates with source DAG/task/run metadata | ✅ Implemented |
| `DatasetTrigger` | Links a DAG to required datasets with `All` or `Any` condition | ✅ Implemented |
| `DatasetScheduler` | Evaluates triggers on each dataset event, returns triggered DAGs | ✅ Implemented |

**Trigger Conditions:**

| Mode | Behavior |
|------|----------|
| `All` | DAG fires only when **every** listed dataset has been updated |
| `Any` | DAG fires when **any** listed dataset is updated |

**Example — dataset-triggered DAG:**

```yaml
# dags/data_aware_pipeline.yaml
id: ml_training_pipeline
schedule_interval: null  # No cron — triggered by data only
tasks:
  - id: validate_data
    type: python
    code: "print('Validating new training data...')"
  - id: train_model
    type: bash
    command: "python train.py --dataset s3://data-lake/features/"
    dependencies: [validate_data]
  - id: deploy_model
    type: bash
    command: "python deploy.py --model latest"
    dependencies: [train_model]
```

**API — emit a dataset event (triggers downstream DAGs):**

> Dataset events are emitted automatically when producer DAGs complete. You can also emit them via CLI (`ryuo dataset event emit`) or the REST API:

```bash
curl -X POST http://localhost:3000/api/datasets/events \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "dataset_id": "features_table",
    "source_dag_id": "etl_pipeline",
    "source_task_id": "write_features",
    "event_type": "update",
    "metadata": {"row_count": 150000, "partition": "2026-04-14"}
  }'
```

---

### 2. Cross-DAG Dependencies ✅

**Module:** `src/advanced_scheduler.rs`

DAGs can declare dependencies on tasks in other DAGs, enabling multi-pipeline coordination.

```
DAG A (task: export_data) completes → CrossDagResolver checks → DAG B starts
```

| Component | Description | Status |
|-----------|-------------|--------|
| `CrossDagDependency` | Declares upstream DAG/task that must complete | ✅ Implemented |
| `CrossDagResolver` | Checks if all upstream dependencies are satisfied | ✅ Implemented |

Agents can use cross-DAG dependencies to chain complex multi-step workflows that span different teams or domains.

---

### 3. Event-Driven Architecture ✅

**Module:** `src/event_framework.rs`

A full event bus with publish/subscribe, trigger evaluation, and webhook ingestion.

| Component | Description | Status |
|-----------|-------------|--------|
| `EventBus` | Broadcast-channel event bus with trigger evaluation | ✅ Implemented |
| `EventTrigger` | Routes events to DAGs based on type, source pattern, and payload conditions | ✅ Implemented |
| `EventFilter` | Composable filters: source glob, JSON payload conditions, metadata match | ✅ Implemented |
| `WebhookReceiver` | HTTP endpoint that converts external webhooks to events (with HMAC validation) | ✅ Implemented |
| `SensorRegistry` | Pluggable sensor framework with lifecycle management (start/stop/poll) | ✅ Implemented |

**Supported Event Types:**

| Event Type | Source | Agent Use Case |
|------------|--------|---------------|
| `FileChange` | File watcher | Trigger pipeline when new data file lands |
| `WebhookReceived` | External HTTP push | GitHub commit, CI/CD completion, monitoring alert |
| `DatasetUpdated` | Dataset event emission | Data-aware scheduling (see above) |
| `DagCompleted` | DAG run finishes | Chain downstream agent workflows |
| `TaskStateChanged` | Task status update | Real-time monitoring by agent |
| `ExternalMessage` | Kafka, SQS, Pub/Sub | Cloud-native event ingestion |
| `Custom(string)` | User-defined | Agent-specific domain events |

**Event trigger with payload filtering:**

```json
{
  "id": "trigger-on-large-dataset",
  "name": "Retrain model on large batches",
  "event_type": "DatasetUpdated",
  "filter": {
    "source_pattern": "s3://data-lake/*",
    "payload_conditions": [
      { "field": "row_count", "operator": "greater_than", "value": 100000 }
    ],
    "required_metadata": { "environment": "production" }
  },
  "action": {
    "dag_id": "ml_retrain_large",
    "pass_event_payload": true,
    "config_overrides": {}
  }
}
```

An agent can register triggers dynamically at runtime, adjusting thresholds and routing rules based on learned patterns.

---

### 4. Sensor Framework ✅

**Module:** `src/sensors.rs`

Sensors poll external conditions and block/reschedule tasks until a condition is met.

| Sensor Type | What It Checks | Agent Use Case |
|-------------|---------------|---------------|
| `file` | File exists at path | Wait for data landing |
| `http` | HTTP endpoint returns expected status | Wait for API readiness |
| `external_task` | Task in another DAG reached "Success" | Cross-DAG synchronization |
| `sql` | SQL query returns ≥1 row | Wait for data availability in a warehouse |

**Execution Modes:**

| Mode | Behavior |
|------|----------|
| `poke` | Holds the worker slot, checking at `poke_interval_secs` |
| `reschedule` | Releases the worker slot between checks |

**SQL sensor with sqlparser validation:**

```yaml
- id: wait_for_data
  type: sensor
  sensor_config:
    sensor_type: sql
    poke_interval_secs: 60
    timeout_secs: 7200
    config:
      conn_id: "postgres://user:pass@host/db"
      sql: "SELECT 1 FROM staging.events WHERE partition_date = CURRENT_DATE"
```

The SQL sensor validates queries with `sqlparser` to reject injection attempts (no UNION, no DDL, single SELECT only).

---

### 5. Dynamic Task Mapping ✅ (Partial)

**Module:** `src/advanced_scheduler.rs`

Fan-out a single task template into multiple parallel instances based on runtime data.

| Component | Description | Status |
|-----------|-------------|--------|
| `TaskMapTemplate` | Template with `Expand` (fan-out) or `Reduce` (fan-in) semantics | ✅ Implemented |
| `expand_mapped_task` | Expands template + values into concrete task instances | ✅ Implemented |
| `DynamicTaskScheduler` | Orchestrates expansion and persists mapped instances | ⚠️ Stub — awaiting DB methods |

**Agent use case:** An agent discovers 500 new data partitions. It creates a mapped task that fans out to 500 parallel transform jobs, then fans in to a single aggregation.

---

### 6. LLM Integration for Code Translation ✅

**Module:** `src/agentic.rs`

Built-in LLM provider abstraction for agentic code generation and migration.

| Component | Description | Status |
|-----------|-------------|--------|
| `LlmProvider` trait | Abstract interface for LLM completions | ✅ Implemented |
| `OpenAiProvider` | OpenAI/ChatGPT-compatible endpoint | ✅ Implemented |
| `AnthropicProvider` | Anthropic Claude endpoint | ✅ Implemented |
| `translate_python_to_rust_agentic` | LLM-powered Python-to-Rust DAG translation with retry | ✅ Implemented |
| `convert_dbt_manifest_to_pipeline` | Parse dbt manifest.json into Ryuo pipeline nodes | ✅ Implemented |

An agent can use `translate_python_to_rust_agentic()` to automatically convert legacy Airflow Python DAGs to native Rust DAGs, with validation retries.

---

### 7. Data Lineage Tracking ✅

**Module:** `src/lineage.rs`

OpenLineage-compliant event emission for full data flow visibility.

| Component | Description | Status |
|-----------|-------------|--------|
| `LineageManager` | Multi-emitter dispatch (HTTP, Log, DB) | ✅ Implemented |
| `RunEvent` | OpenLineage run event with inputs/outputs/facets | ✅ Implemented |
| `HttpLineageEmitter` | Send events to Marquez/Datakin/OpenLineage server | ✅ Implemented |
| `DbLineageEmitter` | Persist events to Ryuo's PostgreSQL | ✅ Implemented |

**Agent use case:** An agent queries lineage events to understand the full data dependency graph, then uses that graph to decide which downstream DAGs need reprocessing after a schema change.

**CLI:**

```bash
# Query lineage events for a specific run
ryuo lineage run <run_id>
# Output: event_type | job_name | job_namespace | inputs | outputs

# List all tracked datasets
ryuo lineage datasets
# Output table: id | name | uri | last_updated

# View upstream/downstream events for a dataset
ryuo lineage dataset <dataset_id>
```

---

### 8. Inter-Task Communication (XCom) ✅

**Module:** `src/xcom.rs`

Tasks within a DAG run can pass data to each other via XCom (cross-communication).

| Operation | Description | Status |
|-----------|-------------|--------|
| `xcom_push` | Store a key-value pair scoped to (dag, task, run) | ✅ Implemented |
| `xcom_pull` | Retrieve a value by key from a specific task | ✅ Implemented |
| `xcom_pull_all` | List all XCom entries for a DAG run | ✅ Implemented |

**Agent use case:** A "data profiling" task pushes statistics (row count, null ratio, schema hash) to XCom. A downstream "quality gate" task pulls these values and decides whether to proceed or fail the pipeline.

**Size limit:** 64 KB per value (enforced — prevents OOM from unbounded data passing).

---

### 9. Connector Ecosystem ✅

**Modules:** `src/enterprise_connector.rs`, `src/connectors.rs`, `src/cloud_connectors.rs`

Unified interface for databases, warehouses, APIs, and streaming platforms.

| Connector | Kind | Capabilities | Status |
|-----------|------|-------------|--------|
| PostgreSQL | Database | Transactions, BatchRead/Write, Streaming, Pushdown | ✅ Full |
| Snowflake | Warehouse | BatchRead/Write, AsyncJobs, Arrow, Pushdown | ✅ Full |
| Databricks | Warehouse | BatchRead/Write, AsyncJobs, Pushdown | ✅ Full |
| BigQuery | Warehouse | BatchRead/Write, AsyncJobs | ✅ Full |
| Redshift | Warehouse | BatchRead/Write, Transactions | ✅ Full |
| MySQL | Database | BatchRead/Write, Transactions | ✅ Full |
| MS SQL | Database | BatchRead/Write | ✅ Full |
| dbt | Transformation | CLI-based run/test/compile | ✅ Full |
| Kafka | Streaming | Produce/Consume | ⚠️ Scaffolded |
| S3/GCS | Storage | Read/Write objects | ⚠️ Scaffolded |
| Delta Lake | Storage | Read/Write | ⚠️ Scaffolded |

Agents can use connectors to query data freshness, validate schemas, and execute queries as part of data-aware decision-making.

---

### 10. Plugin System ✅

**Module:** `src/executor.rs`, `src/sdk.rs`

Extensible operator and sensor plugin system with dynamic loading.

| Component | Description | Status |
|-----------|-------------|--------|
| `RyuoOperator` trait | Interface for custom task executors | ✅ Implemented |
| `PluginRegistry` | Named registry with dynamic `.so`/`.dylib` loading | ✅ Implemented |
| `HttpOperator` | Built-in HTTP request operator | ✅ Implemented |
| `PluginScaffold` | `vortex plugin init` CLI generator | ✅ Implemented |
| `PluginManifest` | `ryuo-plugin.toml` validation | ✅ Implemented |

**Agent use case:** An agent loads a custom ML inference operator plugin, then creates DAGs that use it for batch prediction tasks.

---

### 11. Distributed Execution ✅

**Module:** `src/swarm.rs`, `src/k8s_executor.rs`

| Component | Description | Status |
|-----------|-------------|--------|
| gRPC Swarm | Leader-worker architecture with task queue dispatch | ✅ Implemented |
| Worker heartbeat | Automatic stale worker detection and task re-queuing | ✅ Implemented |
| K8s Executor | Pod-per-task execution with resource limits, labels, tolerations | ✅ Implemented |
| Pool Manager | Concurrency slots to prevent resource starvation | ✅ Implemented |

**Agent use case:** An agent monitors queue depth and worker count, then scales worker pods via K8s API when load increases.

**CLI:**

```bash
# Check swarm status
ryuo swarm status
ryuo swarm workers

# Manage execution pools
ryuo pool list
ryuo pool create high_priority --slots 20
ryuo pool delete high_priority
```

---

### 12. Observability & Tracing ✅

**Module:** `src/telemetry.rs`, `src/metrics.rs`

| Component | Description | Status |
|-----------|-------------|--------|
| W3C Trace Context | Propagation through gRPC and HTTP | ✅ Implemented |
| OTLP Span Builder | Configurable span creation for scheduler, executor, DAG ops | ✅ Implemented |
| Prometheus Metrics | Gauges, counters, histograms for all subsystems | ✅ Implemented |

**Agent use case:** An agent queries Prometheus metrics to detect anomalies (e.g., task duration spike > 3σ), then triggers an investigation DAG.

---

## Architecture: How Agents Use Ryuo

**Why CLI-first?** Agents (LLM-based, ML, autonomous) work best with structured text I/O. CLI commands produce tabular or JSON output that agents can parse directly from stdout — no HTTP client, no auth headers, no JSON serialization. The `ryuo` binary connects directly to PostgreSQL, so agents can operate without a running HTTP server.

```
┌─────────────────────────────────────────────────────────────────────┐
│                        AGENT LAYER                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌──────────────────┐  │
│  │ LLM Agent│  │ ML Agent │  │ Data Agent│  │ Monitoring Agent │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  └────────┬─────────┘  │
│       │              │              │                  │            │
│       ▼              ▼              ▼                  ▼            │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                     Ryuo CLI (primary)                       │   │
│  │  ryuo dag trigger   ryuo lineage datasets   ryuo audit recent│  │
│  │  ryuo secret set    ryuo pool list          ryuo config show │  │
│  └─────────────────────────────────────────────────────────────┘   │
│                              │                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                   Ryuo REST API (secondary)                  │   │
│  │  POST /api/dags/:id/trigger   POST /api/datasets/events     │   │
│  │  POST /api/events/webhook     GET  /api/xcom/pull           │   │
│  └─────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     RYUO ORCHESTRATION ENGINE                       │
│                                                                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │  EventBus    │  │  Dataset     │  │  CrossDag               │  │
│  │  (pub/sub)   │  │  Scheduler   │  │  Resolver               │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────────┘  │
│         │                 │                      │                  │
│         ▼                 ▼                      ▼                  │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                  DAG Scheduler + Executor                    │   │
│  │  Cron → Trigger → Resolve Deps → Execute Tasks → Emit Events│  │
│  └──────────────────────────────────────────────────────────────┘   │
│         │                                                │          │
│         ▼                                                ▼          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │  Sensors     │  │  XCom        │  │  Lineage Manager         │  │
│  │  (poll ext)  │  │  (task data) │  │  (OpenLineage emit)      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────────┘   │
│         │                                                           │
│         ▼                                                           │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  Connectors: Postgres, Snowflake, BigQuery, Kafka, S3, dbt  │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

### CLI vs REST API — When to Use Which

| Interface | Best For | Auth | Requires Server? |
|-----------|----------|------|-----------------|
| **CLI** (`ryuo`) | Agent tool-calling, scripting, CI/CD, direct DB operations | `DATABASE_URL` env var | No — connects directly to PostgreSQL |
| **REST API** | Browser UI, webhook ingestion, external service integration | `Authorization: Bearer <token>` | Yes — needs `ryuo` server running |

**Agent recommendation:** Use CLI for all read/write operations. Use REST API only for webhook ingestion and real-time event subscriptions (future).

---

## Agent Workflow Patterns

### Pattern 1: Data Quality Gate Agent

An agent monitors data freshness and quality, blocking downstream pipelines until conditions are met.

```bash
# 1. Check if upstream ETL completed
ryuo dag get etl_pipeline
# Output: status=Success, last_run=2026-04-14T03:00:00Z

# 2. Query lineage to verify dataset freshness
ryuo lineage datasets
# Output table: dataset_id | uri | last_updated

# 3. Check audit trail for recent data events
ryuo audit recent --limit 10
# Output: timestamp | actor | action | resource

# 4. If quality checks pass → trigger downstream transform
ryuo dag trigger transform_pipeline

# 5. If quality fails → check compliance status
ryuo compliance status
```

### Pattern 2: Autonomous ML Pipeline Agent

An agent manages the full ML lifecycle — detecting data drift, triggering retraining, and deploying models.

```bash
# 1. Check lineage for the training data DAG
ryuo lineage run <run_id>
# Output: event_type | job_name | inputs | outputs | facets

# 2. Check all tracked datasets for freshness
ryuo lineage datasets
# Output: id | name | uri | last_updated

# 3. Look at the specific dataset's lineage events
ryuo lineage dataset features_v2
# Output: upstream/downstream event list

# 4. Trigger retraining with pooled resources
ryuo pool list
# Output: name | slots | used
ryuo dag trigger ml_training_pipeline

# 5. Monitor run status
ryuo dag get ml_training_pipeline
```

### Pattern 3: Cross-Team Data Mesh Agent

An agent coordinates pipelines across independent teams using cross-DAG dependencies and events.

```bash
# 1. List all DAGs — find team A's and team B's pipelines
ryuo dag list
# Output table: id | schedule | status | last_run | team_id

# 2. Check if Team A's ETL completed
ryuo dag get team_a_etl
# Output: status=Success

# 3. Query audit log to confirm Team A exported data
ryuo audit by-actor team_a_service_account
# Output: timestamp | action=dag_completed | resource=team_a_etl

# 4. Trigger Team B's downstream transform
ryuo dag trigger team_b_transform

# 5. Track cross-team lineage
ryuo lineage run <team_b_run_id>
```

### Pattern 4: Cost-Aware Resource Agent

An agent optimizes compute resources based on data volume and SLA requirements.

```bash
# 1. Check current pool utilization
ryuo pool list
# Output: name | total_slots | used_slots

# 2. Check connector availability
ryuo connector list
# Output: name | type | status

# 3. For high load: create a dedicated pool
ryuo pool create high_priority --slots 20

# 4. Trigger the DAG (it will use pool assignment from DAG config)
ryuo dag trigger batch_processing_pipeline

# 5. After processing: clean up pool
ryuo pool delete high_priority
```

### Pattern 5: LLM-Powered DAG Migration Agent

An agent converts legacy Airflow/dbt pipelines to Ryuo automatically.

```bash
# 1. Migrate Airflow DAGs using LLM-assisted translation
ryuo-cli migrate ./legacy_dags/ --agentic --llm-provider openai --model gpt-4

# 2. List converted DAGs to verify
ryuo dag list

# 3. Trigger a validation run
ryuo dag trigger converted_etl_pipeline

# 4. Check compliance against organizational policies
ryuo compliance list
# Output: control | status | description

# 5. Review audit trail for the migration
ryuo audit recent --limit 20
```

### Pattern 6: Security & Access Management Agent

An agent manages IAM policies, token lifecycle, and secret rotation.

```bash
# 1. Create a service account for the agent
ryuo user create agent-ml-pipeline --role operator

# 2. Assign team-scoped RBAC
ryuo rbac assign agent-ml-pipeline operator
ryuo rbac user-roles agent-ml-pipeline

# 3. Create scoped API token
ryuo token create --user agent-ml-pipeline --note "ML pipeline automation"
# Output: token=<uuid>

# 4. Manage secrets for connector credentials
ryuo secret set snowflake_password --value '<encrypted>' --team ml_team
ryuo secret list

# 5. Audit all agent actions
ryuo audit by-actor agent-ml-pipeline
```

---

## Implementation Status

### Priority 0 — Critical CLI Commands for Agent Autonomy ✅ ALL COMPLETE

| Gap | Proposed CLI | Status | Description |
|-----|-------------|--------|-------------|
| **XCom CLI** | `ryuo xcom push/pull/list` | ✅ Done | Agents can read/write inter-task data from CLI with 64KB limit enforcement |
| **Dataset Event CLI** | `ryuo dataset event emit` | ✅ Done | Emit dataset events to trigger downstream DAGs, shows triggered DAG IDs |
| **DAG Runs CLI** | `ryuo dag runs <dag_id>` | ✅ Done | List run history with `--state` filter |
| **DAG Create from YAML** | `ryuo dag create --from-yaml <file>` | ✅ Done | Register DAGs from YAML/JSON files, supports `--dry-run` validation |
| **Swarm Status (real)** | `ryuo swarm status` | ✅ Done | Queries workers table: worker count, capacity, active tasks |
| **Connector Health (real)** | `ryuo connector health <name>` | ✅ Done | Tests connectivity for postgres, lists status for all connectors |

### Priority 1 — Enhanced Agent CLI Integration ✅ ALL COMPLETE

| Gap | Proposed CLI | Status | Description |
|-----|-------------|--------|-------------|
| **JSON Output Mode** | `ryuo --output json dag list` | ✅ Done | Global `--output json` flag on all 35+ commands |
| **Conditional DAG Triggering** | `ryuo dag trigger <id> --config '{"key":"val"}'` | ✅ Done | Trigger with runtime config stored as XCom `__dagrun_conf__` |
| **DAG Backfill** | `ryuo dag backfill <id> --start --end` | ✅ Done | Backfill with `--interval`, `--dry-run`, 10K run safety cap |
| **Task Logs** | `ryuo task logs <instance_id>` | ✅ Done | Task instance logs with `--tail N` option |
| **Event Trigger CRUD** | `ryuo event trigger create/list/delete` | ✅ Done | DB-backed event triggers with JSON filter/config |
| **Sensor Status** | `ryuo sensor list` | ✅ Done | Lists sensor-type task instances and their state |
| **Connector Query** | `ryuo connector query <name> --sql "SELECT ..."` | ✅ Done | sqlparser-validated SELECT only, timeout, row limit |
| **Agent Memory / State Store** | `ryuo agent state get/set/list` | ✅ Done | Key-value store for agents to persist state across DAG runs |

### Priority 2 — Advanced Agent Capabilities ✅ ALL COMPLETE

| Gap | Proposed CLI | Status | Description |
|-----|-------------|--------|-------------|
| **Agent State Store** | `ryuo agent state get/set/list/delete` | ✅ Done | Key-value store with TTL for agent state persistence across runs |
| **Agent Decision Log** | `ryuo agent log insert/query` | ✅ Done | Structured decision audit log with JSON context |
| **Event Watch (Reactive)** | `ryuo event recent/watch` | ✅ Done | Poll-based event watch with `--timeout`, `--interval` |
| **Inter-Agent Events** | `ryuo event publish/custom` | ✅ Done | Custom event bus for agent-to-agent communication |
| **MCP Tool Server** | `ryuo mcp tools/describe` | ✅ Done | MCP tool definitions for 12 Ryuo operations |
| **Data Profiling** | `ryuo profile <connector> --table <name>` | ✅ Done | Row count, null %, distinct, min/max per column |
| **Anomaly Detection** | `ryuo sensor check-anomaly --sql --baseline` | ✅ Done | Statistical anomaly detection with configurable sigma |
| **K8s Executor CLI** | `ryuo k8s status/pods/logs/config` | ✅ Done | K8s pod management via REST API |
| **Kafka Connector** | `ryuo kafka topics/produce/consume` | ✅ Done | Kafka REST Proxy integration |
| **S3/GCS Storage** | `ryuo storage ls/stat/freshness` | ✅ Done | S3-compatible object storage operations |
| **Delta Lake** | `ryuo delta-lake info/schema/history` | ✅ Done | Delta Lake metadata via _delta_log parsing |
| **Agent-Scoped Tokens** | `ryuo token create --scope-rule` | ✅ Done | Scoped tokens with resource:pattern:actions syntax |
| **DR Backup** | `ryuo backup create/list/info` | ✅ Done | Real pg_dump-based database backup |
| **Health Check** | `ryuo health` | ✅ Done | Deep health: DB, workers, queue, datasets |

---

## Configuration for Agent Integration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string (required for all CLI commands) | — |
| `RYUO_BASE_URL` | Ryuo server URL (only needed for REST API calls) | `http://localhost:3000` |
| `RYUO_API_KEY` | API key for REST API authentication | — |
| `RYUO_EVENT_BUS_CAPACITY` | Max events in the broadcast channel | `10000` |
| `RYUO_SENSOR_POLL_INTERVAL` | Default sensor poke interval (seconds) | `30` |
| `RYUO_DATASET_MIN_TRIGGER_INTERVAL` | Minimum seconds between dataset trigger firings | `60` |
| `RYUO_XCOM_MAX_VALUE_BYTES` | Maximum XCom value size | `65536` |
| `RYUO_LINEAGE_ENDPOINT` | OpenLineage server URL for HTTP emitter | `""` |
| `RYUO_LLM_PROVIDER` | Default LLM provider: `openai` or `anthropic` | `openai` |
| `RYUO_LLM_API_KEY` | API key for the configured LLM provider | `""` |
| `RYUO_LLM_MODEL` | Model name (e.g., `gpt-4`, `claude-3-opus`) | `gpt-4` |

---

## Quick Start: Building an Agent with Ryuo

### Prerequisites

```bash
# Set the database connection (all CLI commands use this directly)
export DATABASE_URL="postgres://ryuo:password@localhost:5432/ryuo"

# Verify configuration
ryuo config show
ryuo config validate-db
```

### Step 1: Explore Available DAGs and Resources

```bash
# List all registered DAGs
ryuo dag list
# ┌──────────────────────┬───────────┬──────────┬─────────────────────┐
# │ ID                   │ Schedule  │ Status   │ Last Run            │
# ├──────────────────────┼───────────┼──────────┼─────────────────────┤
# │ etl_pipeline         │ 0 * * * * │ active   │ 2026-04-14T03:00:00 │
# │ ml_training          │ null      │ active   │ 2026-04-13T12:00:00 │
# └──────────────────────┴───────────┴──────────┴─────────────────────┘

# Inspect a specific DAG (outputs JSON with full task graph)
ryuo dag get etl_pipeline

# Check available pools
ryuo pool list

# List connectors
ryuo connector list
```

### Step 2: Query Lineage for Decision-Making

```bash
# List all tracked datasets
ryuo lineage datasets
# ┌────────────────┬─────────────────────────────┬─────────────────────┐
# │ ID             │ URI                         │ Last Updated        │
# ├────────────────┼─────────────────────────────┼─────────────────────┤
# │ features_v2    │ s3://ml-data/features/v2/   │ 2026-04-14T02:30:00 │
# │ raw_events     │ postgres://db/raw_events    │ 2026-04-14T03:00:00 │
# └────────────────┴─────────────────────────────┴─────────────────────┘

# Check lineage events for a specific run
ryuo lineage run <run_id>

# View upstream/downstream for a dataset
ryuo lineage dataset features_v2
```

### Step 3: Trigger DAGs Programmatically

```bash
# Trigger a DAG (creates a new run in the database)
ryuo dag trigger ml_training

# Pause/unpause DAGs based on agent decisions
ryuo dag pause etl_pipeline       # Agent decides to pause during maintenance
ryuo dag unpause etl_pipeline     # Agent resumes after maintenance window
```

### Step 4: Manage Secrets and Credentials

```bash
# Set connector credentials (vault-encrypted)
ryuo secret set snowflake_password --value 'my-secret-value'

# List secrets (values are masked)
ryuo secret list
# ┌────────────────────┬──────────┬─────────────────────┐
# │ Key                │ Team     │ Updated             │
# └────────────────────┴──────────┴─────────────────────┘

# Get a specific secret value (for agent use)
ryuo secret get snowflake_password
```

### Step 5: Monitor and Audit

```bash
# Check recent audit events
ryuo audit recent --limit 20
# Output: timestamp | actor | action | resource | details

# Filter by a specific agent/user
ryuo audit by-actor agent-ml-pipeline

# Check compliance controls
ryuo compliance list
ryuo compliance status

# View system configuration
ryuo config show
ryuo config export  # Full config dump
```

### Step 6: Manage Access (for Multi-Agent Environments)

```bash
# Create a dedicated agent user
ryuo user create ml-agent --role operator

# Assign team-scoped RBAC permissions
ryuo team create ml_team
ryuo rbac assign ml-agent operator

# Create API tokens for agents that need REST API access
ryuo token create --user ml-agent --note "ML pipeline agent"

# List and audit token usage
ryuo token list
ryuo rbac user-roles ml-agent
```

### Step 7: LLM-Assisted DAG Migration

```bash
# Convert Airflow Python DAGs to Ryuo using LLM
ryuo-cli migrate ./airflow_dags/ --agentic --llm-provider openai --model gpt-4

# Convert without LLM (rule-based AST translation)
ryuo-cli migrate ./airflow_dags/

# After migration, verify converted DAGs
ryuo dag list
ryuo dag get converted_pipeline
```

---

## Summary: Feature Matrix

| Capability | Status | CLI Command | Module |
|-----------|--------|------------|--------|
| Dataset-triggered scheduling | ✅ Complete | `ryuo dataset event emit` | `advanced_scheduler.rs` |
| Cross-DAG dependencies | ✅ Complete | `ryuo dag get <id>` | `advanced_scheduler.rs` |
| Event bus with pub/sub | ✅ Complete | `ryuo event publish/custom` | `event_framework.rs` |
| Event trigger routing (type + filter + payload) | ✅ Complete | `ryuo event trigger create/list/delete` | `event_framework.rs` |
| Webhook ingestion with HMAC | ✅ Complete | *(REST-only — POST /api/events/webhook)* | `event_framework.rs` |
| File / HTTP / SQL / ExternalTask sensors | ✅ Complete | `ryuo sensor list` | `sensors.rs` |
| Dynamic task mapping (expand/reduce) | ✅ Complete | `ryuo task dynamic-map` | `advanced_scheduler.rs` |
| LLM DAG migration | ✅ Complete | `ryuo-cli migrate --agentic` | `agentic.rs` |
| dbt manifest conversion | ✅ Complete | `ryuo-cli migrate` | `agentic.rs` |
| OpenLineage data lineage | ✅ Complete | `ryuo lineage run/datasets/dataset` | `lineage.rs` |
| XCom inter-task data passing | ✅ Complete | `ryuo xcom push/pull/list` | `xcom.rs` |
| Plugin system | ✅ Complete | `vortex plugin init` | `executor.rs`, `sdk.rs` |
| DAG management | ✅ Complete | `ryuo dag list/get/trigger/pause/unpause/create/backfill/validate/rollback/versions` | `main.rs` |
| Secret vault | ✅ Complete | `ryuo secret list/get/set/delete` | `vault.rs` |
| User & team management | ✅ Complete | `ryuo user/team list/create/delete` | `main.rs` |
| RBAC permissions | ✅ Complete | `ryuo rbac assign/revoke/user-roles` | `rbac.rs` |
| API token management | ✅ Complete | `ryuo token list/create/revoke/inspect` | `auth.rs` |
| Agent-scoped tokens | ✅ Complete | `ryuo token create --scope-rule` | `auth.rs` |
| Audit logging | ✅ Complete | `ryuo audit recent/by-actor` | `compliance.rs` |
| Compliance controls | ✅ Complete | `ryuo compliance list/status` | `compliance.rs` |
| Pool management | ✅ Complete | `ryuo pool list/create/delete` | `pools.rs` |
| Connector registry | ✅ Complete | `ryuo connector list/health/query` | `connectors.rs` |
| Configuration | ✅ Complete | `ryuo config show/validate-db/export/override` | `config_ops.rs` |
| Auth provider management | ✅ Complete | `ryuo auth-provider list/enable/disable` | `auth.rs` |
| Database migrations | ✅ Complete | `ryuo db --migrate` | `migration.rs` |
| Swarm status | ✅ Complete | `ryuo swarm status/workers` | `swarm.rs` |
| Connector health check | ✅ Complete | `ryuo connector health <name>` | `connectors.rs` |
| JSON output mode | ✅ Complete | `ryuo --output json <cmd>` | `main.rs` |
| Agent state store | ✅ Complete | `ryuo agent state get/set/list/delete` | `main.rs` |
| Agent decision log | ✅ Complete | `ryuo agent log insert/query` | `main.rs` |
| Event watch/subscribe | ✅ Complete | `ryuo event recent/watch` | `main.rs` |
| Inter-agent communication | ✅ Complete | `ryuo event publish/custom` | `main.rs` |
| MCP tool server | ✅ Complete | `ryuo mcp tools/describe` | `mcp_server.rs` |
| Data profiling | ✅ Complete | `ryuo profile <connector> --table <name>` | `main.rs` |
| Anomaly detection | ✅ Complete | `ryuo sensor check-anomaly` | `main.rs` |
| Queue management | ✅ Complete | `ryuo queue list/reprioritize` | `main.rs` |
| Dataset freshness | ✅ Complete | `ryuo dataset freshness` | `main.rs` |
| Schema change detection | ✅ Complete | `ryuo dataset schema store/diff` | `main.rs` |
| Data volume stats | ✅ Complete | `ryuo dataset stats` | `main.rs` |
| Approval gates | ✅ Complete | `ryuo approval request/list/approve/reject` | `main.rs` |
| Rate limiting | ✅ Complete | `ryuo rate-limit check/status` | `main.rs` |
| DAG validation | ✅ Complete | `ryuo validate <file>` | `main.rs` |
| K8s executor | ✅ Complete | `ryuo k8s status/pods/logs/config` | `main.rs` |
| Kafka connector | ✅ Complete | `ryuo kafka topics/produce/consume` | `main.rs` |
| S3/GCS storage | ✅ Complete | `ryuo storage ls/stat/freshness` | `main.rs` |
| Delta Lake connector | ✅ Complete | `ryuo delta-lake info/schema/history` | `main.rs` |
| Backup/DR | ✅ Complete | `ryuo backup create/list/info` | `main.rs` |
| Health check | ✅ Complete | `ryuo health` | `main.rs` |

---

## Complete CLI Reference for Agents

All commands connect directly to PostgreSQL via `DATABASE_URL`. No running server needed.

```bash
# ─── Configuration ───────────────────────────────────────
export DATABASE_URL="postgres://ryuo:password@localhost:5432/ryuo"

# ─── DAG Operations ─────────────────────────────────────
ryuo dag list                              # Table: id, schedule, status, last_run
ryuo dag get <dag_id>                      # JSON: full DAG definition + task graph
ryuo dag trigger <dag_id>                  # Create a new run
ryuo dag pause <dag_id>                    # Pause scheduling
ryuo dag unpause <dag_id>                  # Resume scheduling

# ─── Data Lineage ───────────────────────────────────────
ryuo lineage datasets                      # Table: all tracked datasets
ryuo lineage dataset <dataset_id>          # Upstream/downstream events
ryuo lineage run <run_id>                  # Lineage events for a run

# ─── Secrets (vault-encrypted) ──────────────────────────
ryuo secret list                           # Table: key, team, updated
ryuo secret get <key>                      # Decrypted value
ryuo secret set <key> --value <val>        # Encrypt and store
ryuo secret delete <key>                   # Remove

# ─── Audit & Compliance ────────────────────────────────
ryuo audit recent --limit <n>              # Recent audit events
ryuo audit by-actor <actor>                # Events by user/agent
ryuo compliance list                       # All compliance controls
ryuo compliance status                     # Summary: pass/fail counts

# ─── Users, Teams & RBAC ───────────────────────────────
ryuo user list                             # Table: all users
ryuo user create <name> --role <role>      # Create user
ryuo user get <name>                       # JSON: user details
ryuo user delete <name>                    # Remove user
ryuo team list                             # Table: all teams
ryuo team create <name>                    # Create team
ryuo team delete <name>                    # Remove team
ryuo rbac list-roles                       # Table: role definitions
ryuo rbac list-permissions                 # Permissions grouped by role
ryuo rbac assign <user> <role>             # Grant role
ryuo rbac revoke <user> <role>             # Revoke role
ryuo rbac user-roles <user>                # List user's roles

# ─── API Tokens ─────────────────────────────────────────
ryuo token list                            # Table: active tokens
ryuo token create --user <u> --note <n>    # Create token (returns UUID)
ryuo token revoke <token_id>               # Revoke token

# ─── Auth Providers ─────────────────────────────────────
ryuo auth-provider list                    # Table: SSO/OIDC/SAML providers
ryuo auth-provider enable <name>           # Enable provider
ryuo auth-provider disable <name>          # Disable provider

# ─── Pools ──────────────────────────────────────────────
ryuo pool list                             # Table: name, slots, used
ryuo pool create <name> --slots <n>        # Create pool
ryuo pool delete <name>                    # Remove pool

# ─── Connectors ────────────────────────────────────────
ryuo connector list                        # Registered connectors
ryuo connector health <name>               # Test connectivity
ryuo connector query <name> --sql "..."    # Read-only SQL query (SELECT only)

# ─── Swarm ──────────────────────────────────────────────
ryuo swarm status                          # Cluster status (workers, capacity)
ryuo swarm workers                         # Worker list with task counts

# ─── Configuration ──────────────────────────────────────
ryuo config show                           # Current configuration
ryuo config validate-db                    # Test database connection
ryuo config export                         # Full config dump
ryuo config override <key> <value>         # Set a runtime config override

# ─── XCom (Inter-Task Data) ─────────────────────────────
ryuo xcom push --dag <d> --task <t> --run <r> --key <k> --value <v>
ryuo xcom pull --dag <d> --task <t> --run <r> --key <k>
ryuo xcom list --dag <d> --task <t> --run <r>

# ─── Datasets ──────────────────────────────────────────
ryuo dataset event emit --uri <u> --dag <d> --task <t> --run <r>
ryuo dataset freshness --uri <u>           # Age and last update time
ryuo dataset freshness --stale-after 3600  # List stale datasets
ryuo dataset stats --uri <u>               # Row count, byte size
ryuo dataset schema store --uri <u> --schema '...'
ryuo dataset schema diff --uri <u>         # Compare current vs previous schema

# ─── Events ─────────────────────────────────────────────
ryuo event trigger create --name <n> --event-type <t> --filter '...' --config '...'
ryuo event trigger list                    # List event triggers
ryuo event trigger delete <name>           # Remove event trigger
ryuo event recent                          # Show recent events
ryuo event watch --timeout 300             # Poll for new events
ryuo event publish --event-type <t> --source <s> --payload '...'
ryuo event custom                          # List custom events

# ─── Sensors ────────────────────────────────────────────
ryuo sensor list                           # Sensor task instances
ryuo sensor check-anomaly --sql "..." --baseline "1,2,3,4,5" --sigma 2.0

# ─── Queue & Scheduling ────────────────────────────────
ryuo queue list                            # Task queue by priority
ryuo queue reprioritize <task_id> --priority <n>

# ─── DAG Validation & Versioning ───────────────────────
ryuo validate <file>                       # Validate YAML/JSON DAG
ryuo dag versions <dag_id>                 # List DAG versions
ryuo dag rollback <dag_id> --version <n>   # Roll back to a version

# ─── Approval Gates ────────────────────────────────────
ryuo approval request --dag <d> --task <t> --run <r> --approver <u>
ryuo approval list                         # Pending approvals
ryuo approval approve <id>                 # Approve
ryuo approval reject <id>                  # Reject

# ─── Rate Limiting ─────────────────────────────────────
ryuo rate-limit check <key> --max-requests <n> --window <secs>
ryuo rate-limit status <key>               # Current usage

# ─── Agent State & Decision Log ────────────────────────
ryuo agent state get <key>                 # Get agent state
ryuo agent state set <key> <value>         # Set with optional --ttl
ryuo agent state list                      # List all agent state keys
ryuo agent state delete <key>              # Remove state
ryuo agent log insert <msg> --context '...'  # Log agent decision
ryuo agent log query                       # Query agent logs

# ─── MCP Tool Server ───────────────────────────────────
ryuo mcp tools                             # List all MCP-exposed tools
ryuo mcp describe <tool_name>              # Show tool input schema

# ─── Data Profiling ────────────────────────────────────
ryuo profile postgres --table <name>       # Column-level profiling

# ─── K8s Executor ──────────────────────────────────────
ryuo k8s status                            # Executor status
ryuo k8s pods                              # List task pods
ryuo k8s logs <pod>                        # Get pod logs
ryuo k8s config                            # Show K8s config

# ─── Kafka Connector ──────────────────────────────────
ryuo kafka topics --url <rest_proxy_url>   # List Kafka topics
ryuo kafka produce --topic <t> --value <v> # Produce message
ryuo kafka consume --topic <t> --group <g> # Consume messages

# ─── S3/GCS Storage ───────────────────────────────────
ryuo storage ls --bucket <b>               # List objects
ryuo storage stat --bucket <b> --key <k>   # Object metadata
ryuo storage freshness --bucket <b> --key <k>  # Last modified time

# ─── Delta Lake ────────────────────────────────────────
ryuo delta-lake info <path>                # Table metadata
ryuo delta-lake schema <path>              # Latest schema
ryuo delta-lake history <path>             # Version history

# ─── API Tokens (Scoped) ──────────────────────────────
ryuo token list                            # Table: active tokens
ryuo token create --user <u> --scope-rule "dag:etl_*:trigger,read"
ryuo token inspect <token_id>              # Show scopes & metadata
ryuo token revoke <token_id>               # Revoke token

# ─── Backup & DR ──────────────────────────────────────
ryuo backup create --output-dir ./backups  # pg_dump backup
ryuo backup list --dir ./backups           # List backup files
ryuo backup info <path>                    # Backup file metadata

# ─── Health ────────────────────────────────────────────
ryuo health                                # Deep health check

# ─── Database ───────────────────────────────────────────
ryuo db --migrate                          # Run pending migrations

# ─── DAG Migration (from Airflow/dbt) ──────────────────
ryuo-cli migrate <path>                    # Rule-based AST translation
ryuo-cli migrate <path> --agentic \        # LLM-assisted translation
    --llm-provider openai --model gpt-4
```
