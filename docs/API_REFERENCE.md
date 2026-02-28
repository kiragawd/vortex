# API Reference — VORTEX REST API

## Base URL

```
http://localhost:3000/api
```

## Authentication

All endpoints (except `/api/login`) require an API key in the `Authorization` header:

```
Authorization: <api_key>
```

The API key is obtained via the login endpoint. The default admin key is `vortex_admin_key`.

### RBAC Roles

| Role | Permissions |
|------|------------|
| `Admin` | Full access to all endpoints, including users, secrets, and audit logs |
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
{ "api_key": "vortex_admin_key", "role": "Admin", "username": "admin" }

// Error (401)
{ "error": "Invalid credentials" }
```

---

## DAG Management

### List All DAGs

**`GET /api/dags`**

```json
// Response (200)
[
  {
    "id": "parallel_benchmark",
    "created_at": "2026-02-25T20:55:06Z",
    "schedule_interval": null,
    "last_run": null,
    "is_paused": false,
    "timezone": "UTC",
    "max_active_runs": 1,
    "catchup": false,
    "next_run": null
  }
]
```

### Get DAG Tasks & Dependencies

**`GET /api/dags/:id/tasks`**

Returns tasks, current task instances, DAG metadata, and dependency edges.

```json
// Response (200)
{
  "dag_id": "parallel_benchmark",
  "dag": { "id": "parallel_benchmark", "created_at": "...", "is_paused": false, ... },
  "tasks": [
    { "id": "t1", "name": "Warm-up", "command": "echo 'Vortex engine warm-up...'", "task_type": "bash", "config": {}, "max_retries": 0, "retry_delay_secs": 30 }
  ],
  "instances": [
    { "id": "uuid", "task_id": "t1", "state": "Success", "execution_date": "...", "run_id": "uuid", "stdout": "...", "stderr": "", "duration_ms": 42 }
  ],
  "dependencies": [["t1", "t2"], ["t1", "t3"]]
}
```

### Get Run History

**`GET /api/dags/:id/runs`**

```json
// Response (200)
{
  "dag_id": "parallel_benchmark",
  "runs": [
    { "id": "uuid", "dag_id": "parallel_benchmark", "state": "Success", "execution_date": "...", "start_time": "...", "end_time": "...", "triggered_by": "api" }
  ]
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
  -H "Authorization: vortex_admin_key" \
  -F "file=@dags/my_pipeline.py"
```

```json
// Response (200) — Parsed DAG metadata
{ "dag_id": "my_pipeline", "tasks": [...], "edges": [...] }

// Error (400)
{ "error": "Invalid DAG file: Could not extract dag_id from DAG file" }
```

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
{ "dag_id": "example_dag", "source": "from vortex import DAG...", "file_path": "dags/example_dag.py" }
```

### Update DAG Source Code

**`PATCH /api/dags/:id/source`**

Writes updated source to disk, re-parses with PyO3, and updates the in-memory DAG map.

```json
// Request
{ "source": "from vortex import DAG, BashOperator\n..." }

// Response (200)
{ "message": "Source updated and re-parsed" }
```

### Backfill DAG

**`POST /api/dags/:id/backfill`**

```json
// Request
{ "start_date": "2026-01-01", "end_date": "2026-02-01" }

// Response (200)
{ "message": "Backfill triggered" }
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

### Rollback DAG Version

**`POST /api/dags/:id/versions/:version/rollback`**

Overwrites the current DAG file with the source code of the requested version, triggering a re-parse and creating a new version audit entry.

```json
// Response (200)
{ "message": "Rollback successful" }
```

---

## Task Logs

### Get Task Instance Logs

**`GET /api/tasks/:id/logs`**

Checks DB first (stdout/stderr columns), falls back to filesystem logs.

```json
// Response (200)
{ "stdout": "Vortex engine warm-up...\n", "stderr": "" }

// Error (404)
{ "error": "Log not found" }
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

---

## User & Team Management

### List Teams

**`GET /api/teams`** — Admin only

```json
[
  { "id": "uuid-1", "name": "Data Engineering", "max_concurrent_tasks": 100, "max_dags": 10 },
  { "id": "uuid-2", "name": "Analytics", "max_concurrent_tasks": 20, "max_dags": 5 }
]
```

### Create Team

**`POST /api/teams`** — Admin only

```json
// Request
{ "name": "Data Engineering", "description": "Core data team", "max_concurrent_tasks": 100, "max_dags": 10 }

// Response (200)
{ "message": "Team created", "team_id": "uuid-1" }
```

### Assign User to Team

**`POST /api/teams/:id/users/:username`** — Admin only

```json
// Response (200)
{ "message": "User assigned to team" }
```

### List Users

**`GET /api/users`** — Admin only

```json
[
  { "username": "admin", "role": "Admin", "api_key": "vortex_admin_key" },
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
// Response (200)
{
  "logs": [
    {
      "id": 42,
      "timestamp": "2026-02-28T17:35:00Z",
      "actor": "admin",
      "action": "dag.trigger",
      "target_type": "dag",
      "target_id": "example_dag",
      "metadata": { "run_type": "Full" }
    }
  ],
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

Returns scheduled runs (based on cron) and completed runs for the requested period.

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

### Prometheus Metrics

**`GET /metrics`**

Exposes internal engine metrics in Prometheus text exposition format. Includes metrics for DAG runs, tasks, queue depth, swarms, and scheduler health.
No authentication is required for this endpoint so Prometheus servers can easily scrape it.

```text
// Response (200)
# HELP vortex_dags_total Total number of registered DAGs
# TYPE vortex_dags_total gauge
vortex_dags_total 3
# HELP vortex_scheduler_heartbeat_timestamp Unix epoch (seconds) of the last scheduler tick
# TYPE vortex_scheduler_heartbeat_timestamp gauge
vortex_scheduler_heartbeat_timestamp 1709249581
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
