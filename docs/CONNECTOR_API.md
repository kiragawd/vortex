# Vortex Connector API

## Purpose
Vortex connectors provide a unified interface for databases, warehouses, APIs, and transformation systems.

Core trait: `EnterpriseConnector` in `src/enterprise_connector.rs`.
Implementations: `src/connectors.rs`.

## Architecture

```
┌──────────────────────────────────────────┐
│            ConnectorRegistry             │
│  register(name, Arc<dyn Connector>)      │
│  get(name) → Arc<dyn Connector>          │
└────────────────┬─────────────────────────┘
                 │
   ┌─────────┬───┼───────┬──────────┬──────────┬──────────┐
   │         │   │       │          │          │          │
┌──┴───┐ ┌───┴──┐ ┌──┴──┐ ┌────┴───┐ ┌────┴────┐ ┌───┴────┐
│Postgres│ │Snowfl│ │Databr│ │BigQuery│ │Redshift │ │MySQL / │
│(sqlx) │ │(REST)│ │(REST)│ │(REST)  │ │(sqlx)   │ │MSSQL   │
└───────┘ └──────┘ └─────┘ └────────┘ └─────────┘ └────────┘
   ┌──────────┐  ┌──────────┐  ┌──────────┐
   │dbt (CLI) │  │Kafka     │  │S3/GCS    │
   └──────────┘  └──────────┘  └──────────┘
```

## Connector Kinds
- `Database` — PostgreSQL, MySQL, MS SQL, Redshift
- `Warehouse` — Snowflake, Databricks, BigQuery
- `Api` — REST/HTTP endpoints
- `Transformation` — dbt
- `Streaming` — Kafka (scaffolded)
- `Storage` — S3, GCS, Delta Lake (scaffolded)

## Capabilities
Each connector declares its supported capabilities:

| Capability | Description | Connectors |
|-----------|-------------|------------|
| `Transactions` | ACID transaction support | Postgres, MySQL, MSSQL |
| `BatchRead` | Bulk row fetch | All |
| `BatchWrite` | Bulk insert/upsert | Postgres, MySQL, MSSQL |
| `StreamingRead` | Streamed row-by-row fetch | Postgres, MySQL, MSSQL |
| `AsyncJobs` | Long-running async query/job polling | Snowflake, Databricks |
| `ArrowZeroCopy` | Arrow-format result batches | Snowflake |
| `PushdownPredicates` | Server-side filter/projection pushdown | Snowflake, Databricks |

## Request Context
`ConnectorContext` includes:
- `request_id` — Correlation ID for tracing
- `timeout_ms` — Per-request timeout
- `retry_policy` — Retry configuration (max attempts, backoff)
- `auth` — Provider-specific auth context (tokens, credentials)
- `tags` — Arbitrary key-value metadata for routing/logging

## Request/Response
`QueryRequest` fields:
- `sql` — SQL statement (for database/warehouse connectors)
- `endpoint` — API endpoint (for REST connectors)
- `params` — JSON parameters for binding or request body
- `limit` — Optional row limit

`QueryResult` fields:
- `schema` — Column names and types
- `rows` — Result rows as generic values
- `stats` — Execution statistics (latency, row count)

---

## Implemented Connectors

### PostgreSQL — `PostgresEnterpriseConnector`
**Kind:** Database
**Capabilities:** Transactions, BatchRead, BatchWrite, StreamingRead

**Config:**
| Field | Type | Description |
|-------|------|-------------|
| `host` | String | Database hostname |
| `port` | u16 | Port (default: 5432) |
| `database` | String | Database name |
| `user` | String | Username |
| `password_secret_ref` | String | Vault secret key for password |
| `ssl_mode` | SslMode | `disable`, `prefer`, `require` |
| `max_connections` | u32 | Pool max connections |
| `min_connections` | u32 | Pool min connections |
| `idle_timeout_secs` | u64 | Connection idle timeout |

**Auth:** Username/password via vault secret reference.
**Driver:** `sqlx::PgPool` with async connection pooling.
**Instrumentation:** Emits `connector.postgres.latency_ms` and `connector.postgres.rows` metrics.

---

### Snowflake — `SnowflakeConnector`
**Kind:** Warehouse
**Capabilities:** BatchRead, AsyncJobs, ArrowZeroCopy, PushdownPredicates

**Auth strategies:**
- Username/password
- Key-pair authentication (enterprise preferred)
- OAuth bearer token

**Execution flow:**
1. Submit SQL query via Snowflake REST API
2. Poll async query status until terminal (SUCCESS/FAILED/CANCELED)
3. Fetch results — prefers Arrow record batches when available, falls back to JSON pages
4. Convert Arrow batches to Vortex generic row model lazily

**Arrow optimization:** When the Snowflake endpoint supports Arrow result format, results are fetched as Arrow record batches and converted lazily, avoiding full materialization for streaming workloads.

---

### Databricks — `DatabricksConnector`
**Kind:** Warehouse
**Capabilities:** BatchRead, AsyncJobs, PushdownPredicates

**Two operation modes:**

| Mode | Use Case | API |
|------|----------|-----|
| SQL Warehouse | Direct SQL statements | `databricks.sql.submit` / `fetch_result` |
| Jobs API | Trigger workflow/job runs | `databricks.jobs.run_now` / `poll_job_run` |

**Auth:** Bearer token or workspace PAT.
**Execution flow:** Submit → poll until terminal → fetch result or job output.

---

### MySQL — `MySqlConnector`
**Kind:** Database
**Capabilities:** Transactions, BatchRead, BatchWrite, StreamingRead

**Driver:** `sqlx` MySQL feature (async).
**Type normalization:** Int/BigInt → JSON number, Decimal → string/decimal, DateTime → ISO 8601, Binary → Base64, Null → JSON null.

---

### MS SQL Server — `MsSqlConnector`
**Kind:** Database
**Capabilities:** Transactions, BatchRead, BatchWrite, StreamingRead

**Driver:** `tiberius` (TDS protocol, async).
**Type normalization:** Same normalization as MySQL connector.

---

### dbt — `DbtConnector`
**Kind:** Transformation
**Capabilities:** BatchRead

**Execution flow:**
1. Validate environment: `dbt --version`, project path, profiles path
2. Run selected action: `dbt deps`, `dbt compile`, `dbt run`, or `dbt test`
3. Capture stdout/stderr, parse JSON logs
4. Map exit code to connector result (success/failure)

**Security:** Secrets are redacted from command arguments and captured log output.
**Timeout:** Configurable execution timeout wraps the child process.

---

### BigQuery — `BigQueryConnector`
**Kind:** Warehouse
**Capabilities:** BatchRead, AsyncJobs

**Module:** `src/cloud_connectors.rs`

**Auth:** OAuth token authentication.
**Execution flow:**
1. Submit SQL query via BigQuery REST API
2. Poll job status until completion
3. Fetch result rows

---

### Redshift — `RedshiftConnector`
**Kind:** Database
**Capabilities:** Transactions, BatchRead, BatchWrite, StreamingRead

**Module:** `src/cloud_connectors.rs`

**Driver:** `sqlx` PostgreSQL driver (Redshift is PostgreSQL wire-compatible).
**Auth:** Username/password via connection string.

---

### Scaffolded Connectors

The following connectors have configuration types defined but are awaiting full implementation:

| Connector | Module | Notes |
|-----------|--------|-------|
| Kafka | `src/cloud_connectors.rs` | Producer/consumer configuration types |
| S3 | `src/cloud_connectors.rs` | Bucket, prefix, and credential configuration |
| GCS | `src/cloud_connectors.rs` | Bucket and service account configuration |
| Delta Lake | `src/cloud_connectors.rs` | Table path and storage configuration |

---

## Connector Ecosystem Summary

| Connector | Status | Driver |
|-----------|--------|--------|
| PostgreSQL | **Functional** | `sqlx::PgPool` (async) |
| Snowflake | **Functional** | REST API + Arrow |
| Databricks | **Functional** | REST API (SQL Warehouse + Jobs) |
| BigQuery | **Functional** | REST API + OAuth |
| Redshift | **Functional** | `sqlx` PostgreSQL wire |
| MySQL | **Scaffolded** | `sqlx` MySQL (async) |
| MS SQL | **Scaffolded** | `tiberius` TDS (async) |
| dbt | **Functional** | CLI shell |
| Kafka | **Scaffolded** | Config types only |
| S3/GCS | **Scaffolded** | Config types only |
| Delta Lake | **Scaffolded** | Config types only |

---

## Retry & Timeout Policy

All connectors share a cross-cutting retry wrapper:

```
with_retry(operation, policy):
  attempts = 0
  while attempts < policy.max_attempts:
    result = operation()
    if result.ok: return result
    if not is_retryable(result.err): break
    sleep(backoff(attempts, policy))
    attempts += 1
  return result
```

**Retryable errors:** Network timeouts, transient connection failures, server 5xx responses.
**Non-retryable errors:** Auth failures, invalid SQL, missing config, client 4xx responses.

## Error Handling
Connectors use `anyhow::Result` with structured error taxonomy:
- `ConnectorNotFound` — Unknown connector name in registry
- `DuplicateConnector` — Duplicate registration attempt
- `MissingSql` — SQL required but not provided
- `QueryFailed` — Query execution failure with upstream error
- `DbtCommandFailed` — dbt CLI non-zero exit with captured logs
- `UnsupportedRequest` — Request type not supported by connector

## Security Notes
- Avoid embedding credentials in SQL/params.
- Pass tokens via connector auth context.
- Redact sensitive values in command output and logs for shell-based connectors.
- Store all connector passwords/tokens in the Vortex Secrets Vault, referenced by key.
