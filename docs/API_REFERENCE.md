# API Reference — RYUO REST API

## Port Reference

| Port | Purpose |
|------|---------|
| **3000** | Ryuo REST API, web dashboard, and Prometheus `/metrics` endpoint (default; override with `--port`) |
| **9090** | Prometheus server (scrapes Ryuo `/metrics` on port 3000) |
| **50051** | gRPC swarm endpoint for worker–controller communication (override with `--swarm-port`) |

> The web UI, REST API, and `/metrics` endpoint all share the same HTTP port (default **3000**). There is no separate port 8080 — docs referencing 8080 should be updated to 3000.

## Base URL

```
http://localhost:3000/api
```

## Authentication

All endpoints (except `/api/login` and `/metrics`) require an API key via the `Authorization` header with the `Bearer` prefix:

```
Authorization: Bearer <api_key>
```

The API key is obtained via the login endpoint.

### Global Constraints

- **Payload Size Limits**: All endpoints have a strict `DefaultBodyLimit::max()` enforced at 10 MB. Any request surpassing this size will receive a `413 Payload Too Large` response.
- **CORS Policies**: Cross-origin requests are naturally supported with wide permissibility, as RYUO secures endpoints via stateless bearer tokens. Every response securely attaches `Content-Security-Policy`, `X-Frame-Options: DENY`, and `X-Content-Type-Options: nosniff` headers.

### RBAC Roles

| Role | Permissions |
|------|------------|
| `Admin` | Full access to all endpoints, including users, secrets, teams, and audit logs |
| `Operator` | DAG management (trigger, pause, edit, upload). Cannot manage users, secrets, or audit logs. |
| `Viewer` | Read-only access to DAGs, tasks, runs, and swarm status. |

---

## Authentication

### Login

**`POST /api/login`** — No auth required

```json
// Request
{ "username": "admin", "password": "admin" }

// Response (200)
{ "api_key": "vx_a1b2c3d4...", "role": "Admin", "username": "admin" }

// Error (401)
{ "error": "Invalid credentials" }

// Error (429 - After 10 failed login attempts within 60s)
{ "error": "Too many failed login attempts. Try again later." }
```

### System Health

**`GET /health`** — No auth required

Returns the system health along with RYUO release version and PostgreSQL connectivity status. Used for Kubernetes liveness probes/load-balancer configurations.

```json
// Response (200)
{ "status": "ok", "version": "v0.6.0", "db": "connected" }

// Response (503)
{ "status": "degraded", "version": "v0.6.0", "db": "disconnected" }
```

---

## Pagination

Most list endpoints return paginated responses:

```json
{
  "data": [...],
  "total": 42,
  "limit": 50,
  "offset": 0
}
```

**Query Parameters:**
- `limit` (default: 50, max: 500)
- `offset` (default: 0)

---

## DAG Management

### List All DAGs

**`GET /api/dags`**

Supports pagination via `?limit=N&offset=N`.

```json
// Response (200) — Paginated
{
  "data": [
    {
      "id": "parallel_benchmark",
      "created_at": "2026-02-25T20:55:06Z",
      "schedule_interval": null,
      "last_run": null,
      "is_paused": false,
      "timezone": "UTC",
      "max_active_runs": 1,
      "catchup": false,
      "next_run": null,
      "team_id": null
    }
  ],
  "total": 1,
  "limit": 50,
  "offset": 0
}

// Error (500)
{ "error": "Database error details..." }
```

> **Note:** Non-admin users with a `team_id` will only see DAGs belonging to their team.

### Get DAG Tasks & Dependencies

**`GET /api/dags/:id/tasks`**

Supports pagination for instances via `?limit=N&offset=N`.

```json
// Response (200)
{
  "dag_id": "parallel_benchmark",
  "dag": { "id": "parallel_benchmark", "created_at": "...", "is_paused": false, ... },
  "tasks": [
    { "id": "t1", "name": "Warm-up", "command": "echo 'Ryuo engine warm-up...'", "task_type": "bash", "config": {}, "max_retries": 0, "retry_delay_secs": 30 }
  ],
  "instances": [
    { "id": "uuid", "task_id": "t1", "state": "Success", "execution_date": "...", "run_id": "uuid", "stdout": "...", "stderr": "", "duration_ms": 42 }
  ],
  "instances_total": 1,
  "instances_limit": 50,
  "instances_offset": 0,
  "dependencies": [["t1", "t2"], ["t1", "t3"]]
}
```

### Get Run History

**`GET /api/dags/:id/runs`**

Supports pagination via `?limit=N&offset=N`.

```json
// Response (200) — Paginated
{
  "data": [
    { "id": "uuid", "dag_id": "parallel_benchmark", "state": "Success", "execution_date": "...", "start_time": "...", "end_time": "...", "triggered_by": "api" }
  ],
  "total": 1,
  "limit": 50,
  "offset": 0
}
```

### Trigger DAG Run

**`POST /api/dags/:id/trigger`**

```json
// Response (200)
{ "message": "Triggered" }
```

### Retry Failed Tasks

**`POST /api/dags/:id/retry`**

Re-runs only failed tasks from the last failed run, skipping previously successful tasks.

```json
// Response (200)
{ "message": "Retry triggered" }
```

### Pause / Unpause DAG

**`PATCH /api/dags/:id/pause`**
```json
{ "message": "Paused" }
```

**`PATCH /api/dags/:id/unpause`**
```json
{ "message": "Unpaused" }
```

### Update Schedule

**`PATCH /api/dags/:id/schedule`**

```json
// Request
{ "schedule_interval": "0 12 * * *", "timezone": "US/Eastern", "max_active_runs": 2, "catchup": false }

// Response (200)
{ "message": "Updated" }
```

### Upload DAG File

**`POST /api/dags/upload`** — Multipart form upload

```bash
curl -X POST http://localhost:3000/api/dags/upload \
  -H "Authorization: Bearer <api_key>" \
  -F "file=@dags/my_pipeline.py"
```

```json
// Response (200) — All parsed DAGs registered
{ "dag_ids": ["my_pipeline", "another_dag"], "dag_count": 2 }

// Single-DAG file (still works)
{ "dag_ids": ["my_pipeline"], "dag_count": 1 }

// Error (400)
{ "error": "Invalid DAG file: Could not extract dag_id from DAG file" }
```

> **Note:** A single `.py` file may define multiple DAGs. All DAGs found in the file are registered. Previously, only the first DAG was registered and the rest were silently discarded (Bug #9).

### Validate DAG

**`GET /api/dags/:id/validate`**

```json
// Response (200)
{ "valid": true, "metadata": { ... } }
```

### Get DAG Source Code

**`GET /api/dags/:id/source`**

```json
// Response (200)
{ "dag_id": "example_dag", "source": "from ryuo import DAG...", "file_path": "dags/example_dag.py" }
```

### Update DAG Source Code

**`PATCH /api/dags/:id/source`**

Writes updated source to disk, re-parses with PyO3, and updates the in-memory DAG map.

```json
// Request
{ "source": "from ryuo import DAG, BashOperator\n..." }

// Response (200)
{ "message": "Source updated and re-parsed" }
```

### Backfill DAG

**`POST /api/dags/:id/backfill`**

```json
// Request
{ "start_date": "2026-01-01T00:00:00Z", "end_date": "2026-02-01T00:00:00Z" }

// Response (200)
{ "message": "Backfill triggered" }

// Error (400) - If dates are omitted or formatted incorrectly
{ "error": "Invalid start_date or end_date format" }
```

> **Note:** `start_date` and `end_date` must be RFC 3339 timestamps.

### Get Backfill Progress

**`GET /api/dags/:id/backfill/progress`**

```json
// Response (200)
{ "dag_id": "example_dag", "progress": 0.75 }
```

---

## DAG Versioning

### Get DAG Versions

**`GET /api/dags/:id/versions`**

```json
// Response (200)
{
  "versions": [
    { "version": 2, "file_path": "/path/to/dag_v2.py", "created_at": "2026-02-28T18:00:00Z" },
    { "version": 1, "file_path": "/path/to/dag_v1.py", "created_at": "2026-02-27T10:00:00Z" }
  ]
}
```

### Get Version Source Code

**`GET /api/dags/:id/versions/:version/source`**

Returns the source code for a specific DAG version.

```json
// Response (200)
{ "version": 1, "source": "from ryuo import DAG..." }
```

### Rollback DAG Version

**`POST /api/dags/:id/versions/:version/rollback`**

Overwrites the current DAG file with the source code of the requested version, triggering a re-parse and creating a new version audit entry.

```json
// Response (200)
{ "message": "Rollback successful" }
```

---

## Task Logs & Events

### Get Task Instance Logs

**`GET /api/tasks/:id/logs`**

Checks DB first (stdout/stderr columns), falls back to filesystem logs.

```json
// Response (200)
{ "stdout": "Ryuo engine warm-up...\n", "stderr": "" }

// Error (404)
{ "error": "Log not found" }
```

### Get Task Instance Events

**`GET /api/task-instances/:dag_id/:ti_id/events`**

Returns lifecycle events for a specific task instance.

```json
// Response (200)
[
  { "event_type": "state_change", "from": "Queued", "to": "Running", "timestamp": "..." }
]
```

---

## XCom (Cross-Task Communication)

### Push XCom Value

**`POST /api/xcom/push`**

```json
// Request
{ "dag_id": "my_dag", "task_id": "extract", "run_id": "uuid", "key": "row_count", "value": "42" }

// Response (200)
{ "status": "ok" }
```

### Pull XCom Value

**`GET /api/xcom/pull?dag_id=my_dag&task_id=extract&run_id=uuid&key=row_count`**

```json
// Response (200)
{ "value": "42" }

// Not found (404)
{ "value": null }
```

### List XCom Values for a Run

**`GET /api/dags/:id/runs/:run_id/xcom`**

Supports pagination via `?limit=N&offset=N`.

```json
// Response (200) — Paginated
{
  "data": [
    { "dag_id": "my_dag", "task_id": "extract", "key": "row_count", "value": "42", "timestamp": "..." }
  ],
  "total": 1,
  "limit": 50,
  "offset": 0
}
```

---

## Task Pools

### List Pools

**`GET /api/pools`**

```json
// Response (200)
{ "pools": [ { "name": "db_connections", "slots": 10, "description": "Database connection pool" } ] }
```

### Create Pool

**`POST /api/pools`**

```json
// Request
{ "name": "db_connections", "slots": 10, "description": "Database connection pool" }

// Response (200)
{ "status": "created", "name": "db_connections" }
```

### Get Pool

**`GET /api/pools/:name`**

```json
// Response (200)
{ "name": "db_connections", "slots": 10, "description": "..." }

// Not found (404)
{ "error": "Pool not found" }
```

### Update Pool

**`PUT /api/pools/:name`**

```json
// Request
{ "slots": 20, "description": "Updated description" }

// Response (200)
{ "status": "updated", "name": "db_connections" }
```

### Delete Pool

**`DELETE /api/pools/:name`**

```json
// Response (200)
{ "status": "deleted", "name": "db_connections" }
```

---

## Webhook Callbacks (Notifications)

### Get DAG Callbacks

**`GET /api/dags/:id/callbacks`**

```json
// Response (200)
{ "dag_id": "my_dag", "config": { "on_success": [...], "on_failure": [...] } }

// Not configured (404)
{ "error": "No callbacks configured" }
```

### Set DAG Callbacks

**`PUT /api/dags/:id/callbacks`**

Configure notifications for DAG lifecycle events. Supports Webhook, Slack, and Email targets.

```json
// Request
{
  "config": {
    "on_success": [
      { "type": "Webhook", "config": { "url": "https://hooks.example.com/success", "headers": {} } }
    ],
    "on_failure": [
      { "type": "Slack", "config": { "webhook_url": "https://hooks.slack.com/...", "channel": "#alerts" } }
    ],
    "on_retry": null,
    "on_sla_miss": null
  }
}

// Response (200)
{ "status": "saved", "dag_id": "my_dag" }
```

**Supported notification targets:**
- **Webhook** — `{ "type": "Webhook", "config": { "url": "...", "headers": {} } }`
- **Slack** — `{ "type": "Slack", "config": { "webhook_url": "...", "channel": "..." } }`
- **Email** — `{ "type": "Email", "config": { "smtp_host": "...", "smtp_port": 587, "from": "...", "to": ["..."], "username": "...", "password": "..." } }`

### Delete DAG Callbacks

**`DELETE /api/dags/:id/callbacks`**

```json
// Response (200)
{ "status": "deleted", "dag_id": "my_dag" }
```

---

## Swarm Management

### Swarm Status

**`GET /api/swarm/status`**

```json
{ "enabled": true, "active_workers": 1, "queue_depth": 0 }
```

### List Workers

**`GET /api/swarm/workers`**

```json
{
  "workers": [
    { "worker_id": "worker-a1b2c3d4", "hostname": "MacBook-Air.local", "capacity": 4, "active_tasks": 0, "labels": [], "last_heartbeat": "...", "status": "active" }
  ]
}
```

### Drain Worker

**`POST /api/swarm/workers/:id/drain`** — Worker finishes current tasks then stops accepting new ones.

```json
{ "message": "Draining" }
```

### Remove Worker

**`DELETE /api/swarm/workers/:id`**

```json
{ "message": "Removed" }
```

---

## Secrets Vault

### List Secret Keys

**`GET /api/secrets`** — Admin only

Returns secret names only (never values).

```json
{ "secrets": ["DB_PASSWORD", "API_TOKEN"] }
```

### Store Secret

**`POST /api/secrets`** — Admin only. Value is encrypted with AES-256-GCM before storage.

```json
// Request
{ "key": "DB_PASSWORD", "value": "super_secret" }

// Response (200)
{ "message": "Secret stored successfully" }
```

### Delete Secret

**`DELETE /api/secrets/:key`** — Admin only

```json
{ "message": "Secret deleted" }
```

> **Note:** There is no endpoint to retrieve a secret value via the API. Secrets are only decrypted at task execution time and injected as environment variables.

---

## User & Team Management

### List Teams

**`GET /api/teams`** — Admin only

```json
{
  "teams": [
    { "id": "uuid-1", "name": "Data Engineering", "max_concurrent_tasks": 100, "max_dags": 10 }
  ]
}
```

### Create Team

**`POST /api/teams`** — Admin only

```json
// Request
{ "id": "team-de", "name": "Data Engineering", "description": "Core data team", "max_concurrent_tasks": 100, "max_dags": 10 }

// Response (200)
{ "status": "created", "id": "team-de" }
```

### Get Team

**`GET /api/teams/:id`** — Admin or team member

```json
// Response (200)
{ "id": "team-de", "name": "Data Engineering", "description": "...", "max_concurrent_tasks": 100, "max_dags": 10 }
```

### Update Team

**`PUT /api/teams/:id`** — Admin only

```json
// Request
{ "name": "Data Engineering", "description": "Updated", "max_concurrent_tasks": 200, "max_dags": 20 }

// Response (200)
{ "status": "updated", "id": "team-de" }
```

### Delete Team

**`DELETE /api/teams/:id`** — Admin only

```json
{ "status": "deleted", "id": "team-de" }
```

### Assign User to Team

**`PUT /api/teams/:id/users/:username`** — Admin only

```json
// Request
{ "team_id": "team-de" }

// Response (200)
{ "message": "User assigned to team" }
```

> To unassign a user, pass `{ "team_id": "unassign" }`.

### List Users

**`GET /api/users`** — Admin only

```json
[
  { "username": "admin", "role": "Admin", "api_key": "vx_..." },
  { "username": "operator1", "role": "Operator", "api_key": "vx_abc123..." }
]
```

### Create User

**`POST /api/users`** — Admin only

```json
// Request
{ "username": "viewer1", "password": "password123", "role": "Viewer" }

// Response (200)
{ "message": "User created", "api_key": "vx_generated_key..." }
```

### Delete User

**`DELETE /api/users/:username`** — Admin only. Cannot delete the `admin` user.

```json
{ "message": "User deleted" }
```

---

## Audit Log

### Get Audit Logs

**`GET /api/audit`** — Admin only. Returns paginated audit logs.

**Query Parameters:**
- `limit` (default: 50, max: 500)
- `offset` (default: 0)
- `actor` (optional filter)
- `action` (optional filter)

```json
// Response (200) — Paginated
{
  "data": [
    {
      "id": 42,
      "timestamp": "2026-02-28T17:35:00Z",
      "actor": "admin",
      "action": "dag.trigger",
      "target_type": "dag",
      "target_id": "example_dag",
      "metadata": "{ \"run_type\": \"Full\" }"
    }
  ],
  "total": 42,
  "limit": 50,
  "offset": 0
}
```

---

## Analysis & Visualization

### Get Gantt Timeline

**`GET /api/analysis/gantt?dag_id=example_dag`**

Returns task execution timing data for a specific DAG.

```json
// Response (200)
{
  "dag_id": "example_dag",
  "tasks": [
    {
      "task_id": "t1",
      "instances": [
        { "run_id": "uuid", "state": "Success", "start_time": "...", "end_time": "...", "duration_ms": 42 }
      ]
    }
  ]
}
```

### Get Schedule Calendar

**`GET /api/analysis/calendar?days=30`**

Returns scheduled runs (based on cron) and completed runs for the requested period. Maximum 90 days.

```json
// Response (200)
{
  "events": [
    { "dag_id": "example_dag", "scheduled_time": "2026-03-01T12:00:00Z", "type": "scheduled" },
    { "dag_id": "example_dag", "scheduled_time": "2026-02-28T12:00:00Z", "type": "completed", "state": "Success" }
  ]
}
```

---

## Observability

### Health Check

**`GET /health`** — No authentication required

Returns the controller status and database connectivity.

```json
{"status": "ok", "version": "0.6.0", "db": "connected"}
```

Returns `200 OK` when healthy. Returns `503 Service Unavailable` with `"status":"degraded"` when the database is unreachable. Use with load-balancer probes and Kubernetes readiness checks.

---

### Prometheus Metrics

**`GET /metrics`** — No authentication required

Exposes internal engine metrics in Prometheus text exposition format.

```text
# HELP ryuo_dags_total Total number of registered DAGs
# TYPE ryuo_dags_total gauge
ryuo_dags_total 3
# HELP ryuo_scheduler_heartbeat_timestamp Unix epoch (seconds) of the last scheduler tick
# TYPE ryuo_scheduler_heartbeat_timestamp gauge
ryuo_scheduler_heartbeat_timestamp 1709249581
...
```

---

## Error Handling

All error responses follow:

```json
{ "error": "Description of what went wrong" }
```

| Status Code | Meaning |
|-------------|---------|
| `200` | Success |
| `400` | Bad request (invalid input, parse error) |
| `401` | Unauthorized (missing or invalid API key) |
| `403` | Forbidden (insufficient role permissions) |
| `404` | Resource not found |
| `500` | Internal server error |
| `503` | Service unavailable (e.g., vault not initialized) |

---

## Approval Workflows

### List Pending Approvals

**`GET /api/v1/approvals/pending`** — Requires `approval_workflows` feature flag enabled

Returns all approval requests awaiting a vote.

```json
// Response (200)
[
  {
    "id": "uuid",
    "dag_id": "my_pipeline",
    "requested_by": "operator1",
    "action": "trigger",
    "created_at": "2026-04-01T10:00:00Z",
    "status": "pending"
  }
]
```

### Vote on Approval Request

**`POST /api/v1/approvals/:id/vote`** — Admin only

```json
// Request
{ "approved": true, "comment": "Looks good" }

// Response (200)
{ "message": "Vote recorded" }

// Error (404)
{ "error": "Approval request not found" }
```

### Get Approval Status

**`GET /api/v1/approvals/:id/status`**

```json
// Response (200)
{
  "id": "uuid",
  "dag_id": "my_pipeline",
  "status": "approved",
  "votes": [
    { "voter": "admin", "approved": true, "comment": "Looks good", "voted_at": "2026-04-01T10:05:00Z" }
  ]
}
```

---

## API Token Management

### List API Tokens

**`GET /api/tokens`** — Admin only

```json
// Response (200)
[
  {
    "id": "uuid",
    "name": "ci-pipeline-token",
    "prefix": "vx_a1b2c3",
    "scopes": ["dag:read", "dag:trigger"],
    "expires_at": "2027-01-01T00:00:00Z",
    "created_at": "2026-01-01T00:00:00Z"
  }
]
```

### Create API Token

**`POST /api/tokens`** — Admin only

```json
// Request
{
  "name": "ci-pipeline-token",
  "scopes": ["dag:read", "dag:trigger"],
  "expires_at": "2027-01-01T00:00:00Z"
}

// Response (200) — Token value only shown once at creation time
{
  "id": "uuid",
  "token": "vx_a1b2c3d4e5f6...",
  "name": "ci-pipeline-token",
  "scopes": ["dag:read", "dag:trigger"]
}
```

### Revoke API Token

**`POST /api/tokens/:id/revoke`** — Admin only

```json
// Response (200)
{ "message": "Token revoked" }

// Error (404)
{ "error": "Token not found" }
```

---

## Data Lineage

### Query Lineage Events

**`GET /api/lineage/events/:dag_id`**

Returns OpenLineage-compliant lineage events for a specific DAG.

```json
// Response (200)
[
  {
    "event_type": "COMPLETE",
    "dag_id": "my_pipeline",
    "task_id": "extract",
    "run_id": "uuid",
    "inputs": [{ "namespace": "postgres://host/db", "name": "raw_events" }],
    "outputs": [{ "namespace": "postgres://host/db", "name": "processed_events" }],
    "emitted_at": "2026-04-01T10:00:00Z"
  }
]
```

### List Lineage Datasets

**`GET /api/lineage/datasets`**

Returns all dataset namespaces and names discovered via lineage tracking.

```json
// Response (200)
[
  { "namespace": "postgres://host/db", "name": "raw_events" },
  { "namespace": "s3://bucket", "name": "output/data.parquet" }
]
```

---

## Incident Management

### List Incident Configs

**`GET /api/incidents/configs`** — Admin only

Returns configured incident management integrations (PagerDuty, OpsGenie, Datadog).

```json
// Response (200)
[
  { "id": "uuid", "provider": "pagerduty", "routing_key": "****masked****", "created_at": "..." }
]
```

### Create Incident Config

**`POST /api/incidents/configs`** — Admin only

```json
// Request
{ "provider": "pagerduty", "routing_key": "your-routing-key" }

// Response (200)
{ "id": "uuid", "message": "Incident config created" }
```

### Delete Incident Config

**`DELETE /api/incidents/configs/:id`** — Admin only

```json
{ "message": "Deleted" }
```

---

## RBAC — Role-Based Access Control

### List RBAC Roles

**`GET /api/rbac/roles`** — Admin only

Returns all defined RBAC roles with their permissions.

```json
// Response (200)
[
  {
    "id": "admin",
    "name": "Admin",
    "permissions": ["dag:*", "secrets:*", "users:*", "audit:read"]
  },
  {
    "id": "operator",
    "name": "Operator",
    "permissions": ["dag:read", "dag:trigger", "dag:edit"]
  }
]
```

### Get Role Permissions

**`GET /api/rbac/roles/:role_id/permissions`**

```json
// Response (200)
{ "role_id": "operator", "permissions": ["dag:read", "dag:trigger", "dag:edit"] }
```

### Get User Roles

**`GET /api/rbac/users/:user_id/roles`**

```json
// Response (200)
{ "user_id": "operator1", "roles": ["operator"] }
```

### Assign Role to User

**`POST /api/rbac/users/:user_id/roles`** — Admin only

```json
// Request
{ "role_id": "operator" }

// Response (200)
{ "message": "Role assigned" }
```

### Revoke Role from User

**`DELETE /api/rbac/users/:user_id/roles/:role_id`** — Admin only

```json
{ "message": "Role revoked" }
```

### Get User Effective Permissions

**`GET /api/rbac/users/:user_id/permissions`**

Returns the union of all permissions from all roles assigned to the user.

```json
// Response (200)
{ "user_id": "operator1", "permissions": ["dag:read", "dag:trigger", "dag:edit"] }
```

---

## Network Security — IP Allowlist

### List IP Allowlist Rules

**`GET /api/network/ip-allowlist`** — Admin only

```json
// Response (200)
[
  { "id": "uuid", "cidr": "10.0.0.0/8", "description": "Internal network", "enabled": true }
]
```

### Add IP Allowlist Rule

**`POST /api/network/ip-allowlist`** — Admin only

> **Note:** When the allowlist is empty, all IPs are permitted (open-by-default). Adding any rule enables enforcement.

```json
// Request
{ "cidr": "192.168.1.0/24", "description": "Office network", "enabled": true }

// Response (200)
{ "id": "uuid", "message": "Rule created" }
```

### Delete IP Allowlist Rule

**`DELETE /api/network/ip-allowlist/:id`** — Admin only

```json
{ "message": "Rule deleted" }
```
