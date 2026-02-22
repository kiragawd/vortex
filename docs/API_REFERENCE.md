# API Reference — Complete Endpoint Documentation

## Overview

VORTEX exposes a comprehensive REST API for managing DAGs, tasks, workers, and secrets. All endpoints return JSON responses and support standard HTTP status codes.

### Base URL

```
http://localhost:8080/api
```

### Authentication

(Currently optional; future versions will support Bearer tokens and API keys)

### Response Format

All responses follow this structure:

**Success (2xx):**
```json
{
  "status": "success",
  "data": { /* endpoint-specific data */ }
}
```

**Error (4xx, 5xx):**
```json
{
  "status": "error",
  "error": "Error message describing the problem",
  "code": "error_code"
}
```

---

## Secrets Management API

### 1. Create Secret

**Endpoint:** `POST /api/secrets`

Create a new encrypted secret.

**Request:**
```json
{
  "name": "db_password",
  "value": "super_secret_password"
}
```

**Response (201 Created):**
```json
{
  "status": "success",
  "data": {
    "id": "secret_abc123",
    "name": "db_password",
    "created_at": "2026-02-22T22:59:00Z",
    "updated_at": "2026-02-22T22:59:00Z"
  }
}
```

**Error Cases:**
- `400 Bad Request`: Missing or invalid `name` or `value`
- `409 Conflict`: Secret with this name already exists

---

### 2. Get Secret

**Endpoint:** `GET /api/secrets/{id}`

Retrieve a secret by ID (returns decrypted value).

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "secret_abc123",
    "name": "db_password",
    "value": "super_secret_password",
    "created_at": "2026-02-22T22:59:00Z",
    "updated_at": "2026-02-22T22:59:00Z"
  }
}
```

**Error Cases:**
- `404 Not Found`: Secret with this ID doesn't exist
- `500 Internal Server Error`: Decryption failed (key mismatch or corruption)

---

### 3. Update Secret

**Endpoint:** `PUT /api/secrets/{id}`

Update an existing secret (re-encrypts with a new nonce).

**Request:**
```json
{
  "value": "new_secret_password"
}
```

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "secret_abc123",
    "name": "db_password",
    "created_at": "2026-02-22T22:59:00Z",
    "updated_at": "2026-02-22T23:05:30Z"
  }
}
```

**Error Cases:**
- `404 Not Found`: Secret doesn't exist
- `400 Bad Request`: Missing `value` field

---

### 4. Delete Secret

**Endpoint:** `DELETE /api/secrets/{id}`

Permanently delete a secret.

**Response (204 No Content)**

**Error Cases:**
- `404 Not Found`: Secret doesn't exist
- `409 Conflict`: Secret is in use by active tasks

---

### 5. List All Secrets

**Endpoint:** `GET /api/secrets`

List all secrets (metadata only; no values).

**Query Parameters:**
- `page` (optional): Page number (default: 1)
- `limit` (optional): Items per page (default: 20)

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "secrets": [
      {
        "id": "secret_abc123",
        "name": "db_password",
        "created_at": "2026-02-22T22:59:00Z",
        "updated_at": "2026-02-22T22:59:00Z"
      },
      {
        "id": "secret_def456",
        "name": "slack_token",
        "created_at": "2026-02-20T10:30:00Z",
        "updated_at": "2026-02-20T10:30:00Z"
      }
    ],
    "total": 2,
    "page": 1,
    "limit": 20
  }
}
```

---

## Worker Management API

### 1. Register Worker

**Endpoint:** `POST /api/workers/register`

Register a new worker with the controller.

**Request:**
```json
{
  "name": "worker-01",
  "host": "192.168.1.100",
  "port": 9090,
  "version": "1.0.0"
}
```

**Response (201 Created):**
```json
{
  "status": "success",
  "data": {
    "id": "worker_xyz789",
    "name": "worker-01",
    "state": "IDLE",
    "created_at": "2026-02-22T22:59:00Z"
  }
}
```

**Error Cases:**
- `400 Bad Request`: Missing or invalid fields
- `409 Conflict`: Worker with this name already exists

---

### 2. Get Worker Status

**Endpoint:** `GET /api/workers/{id}`

Get the current status of a worker.

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "worker_xyz789",
    "name": "worker-01",
    "state": "ACTIVE",
    "last_heartbeat": "2026-02-22T22:59:50Z",
    "current_task_id": "task_123abc",
    "uptime_seconds": 86400,
    "tasks_completed": 450,
    "created_at": "2026-02-22T22:59:00Z"
  }
}
```

**Error Cases:**
- `404 Not Found`: Worker doesn't exist

---

### 3. List All Workers

**Endpoint:** `GET /api/workers`

List all registered workers with their status.

**Query Parameters:**
- `state` (optional): Filter by state (IDLE, ACTIVE, OFFLINE, etc.)
- `page` (optional): Page number (default: 1)
- `limit` (optional): Items per page (default: 20)

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "workers": [
      {
        "id": "worker_xyz789",
        "name": "worker-01",
        "state": "ACTIVE",
        "last_heartbeat": "2026-02-22T22:59:50Z",
        "tasks_completed": 450
      },
      {
        "id": "worker_abc123",
        "name": "worker-02",
        "state": "OFFLINE",
        "last_heartbeat": "2026-02-22T22:50:00Z",
        "tasks_completed": 320
      }
    ],
    "total": 42,
    "active": 40,
    "offline": 2
  }
}
```

---

### 4. Reset Worker State

**Endpoint:** `POST /api/workers/{id}/reset`

Manually reset a worker to IDLE state (for recovery).

**Request:**
```json
{
  "force": false
}
```

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "worker_xyz789",
    "state": "IDLE",
    "message": "Worker reset to IDLE state"
  }
}
```

---

## Task Management API

### 1. List Tasks

**Endpoint:** `GET /api/tasks`

List all tasks across all DAGs.

**Query Parameters:**
- `dag_id` (optional): Filter by DAG ID
- `state` (optional): Filter by state (Queued, Running, Completed, Failed)
- `worker_id` (optional): Filter by assigned worker
- `page` (optional): Page number (default: 1)
- `limit` (optional): Items per page (default: 20)

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "tasks": [
      {
        "id": "task_123abc",
        "name": "fetch_data",
        "dag_id": "dag_xyz789",
        "state": "Running",
        "worker_id": "worker_abc123",
        "started_at": "2026-02-22T22:59:00Z",
        "attempt": 1
      },
      {
        "id": "task_456def",
        "name": "process_data",
        "dag_id": "dag_xyz789",
        "state": "Queued",
        "worker_id": null,
        "depends_on": ["task_123abc"]
      }
    ],
    "total": 250,
    "completed": 200,
    "running": 30,
    "queued": 20
  }
}
```

---

### 2. Get Task Details

**Endpoint:** `GET /api/tasks/{id}`

Get detailed information about a specific task.

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "task_123abc",
    "name": "fetch_data",
    "dag_id": "dag_xyz789",
    "image": "postgres-client:latest",
    "command": ["psql", "-h", "db.example.com", "-U", "admin", "-c", "SELECT * FROM users"],
    "state": "Completed",
    "worker_id": "worker_abc123",
    "exit_code": 0,
    "stdout": "Fetched 10,000 records",
    "stderr": "",
    "duration_ms": 3500,
    "created_at": "2026-02-22T22:50:00Z",
    "started_at": "2026-02-22T22:59:00Z",
    "completed_at": "2026-02-22T23:05:30Z",
    "depends_on": [],
    "attempt": 1,
    "max_retries": 3
  }
}
```

**Error Cases:**
- `404 Not Found`: Task doesn't exist

---

### 3. Get Task Logs

**Endpoint:** `GET /api/tasks/{id}/logs`

Stream or retrieve logs from a task.

**Query Parameters:**
- `follow` (optional): If true, stream logs as they arrive (WebSocket)
- `tail` (optional): Only return last N lines (default: 100)

**Response (200 OK - plain text):**
```
2026-02-22T22:59:00Z [INIT] Starting PostgreSQL client
2026-02-22T22:59:02Z [INFO] Connected to db.example.com
2026-02-22T22:59:05Z [INFO] Fetched 10,000 records
2026-02-22T22:59:30Z [SUCCESS] Task completed
```

**WebSocket (follow=true):**
```
GET /api/tasks/{id}/logs?follow=true
Upgrade: websocket

# Server streams logs in real-time
```

---

### 4. Cancel Task

**Endpoint:** `POST /api/tasks/{id}/cancel`

Cancel a task (only works if state is Queued or Running).

**Request:**
```json
{
  "reason": "User requested cancellation"
}
```

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "task_123abc",
    "state": "Cancelled",
    "reason": "User requested cancellation",
    "cancelled_at": "2026-02-22T23:05:30Z"
  }
}
```

**Error Cases:**
- `404 Not Found`: Task doesn't exist
- `409 Conflict`: Task is already completed or failed
- `403 Forbidden`: Task is executing on a remote worker (can't forcefully kill)

---

### 5. Retry Task

**Endpoint:** `POST /api/tasks/{id}/retry`

Retry a failed task.

**Request:**
```json
{
  "reset_state": true
}
```

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "task_123abc",
    "state": "Queued",
    "attempt": 2,
    "message": "Task re-queued for execution"
  }
}
```

**Error Cases:**
- `404 Not Found`: Task doesn't exist
- `409 Conflict`: Task is not in a failed state
- `400 Bad Request`: Max retries exceeded

---

## DAG Management API

### 1. Submit DAG

**Endpoint:** `POST /api/dags`

Submit a new DAG for execution.

**Request:**
```json
{
  "name": "data-pipeline",
  "description": "Daily ETL pipeline",
  "tasks": [
    {
      "name": "fetch_data",
      "image": "postgres-client:latest",
      "command": ["psql", "-h", "db.example.com", "-U", "admin", "-c", "SELECT * FROM users"],
      "secrets": ["db_password"],
      "timeout_seconds": 3600
    },
    {
      "name": "process_data",
      "image": "python:3.13",
      "command": ["python", "process.py"],
      "depends_on": ["fetch_data"],
      "timeout_seconds": 7200
    },
    {
      "name": "notify",
      "image": "alpine:latest",
      "command": ["echo", "Pipeline complete"],
      "depends_on": ["process_data"]
    }
  ]
}
```

**Response (202 Accepted):**
```json
{
  "status": "success",
  "data": {
    "id": "dag_xyz789",
    "name": "data-pipeline",
    "state": "Running",
    "task_count": 3,
    "created_at": "2026-02-22T22:59:00Z",
    "estimated_completion": "2026-02-22T23:30:00Z"
  }
}
```

**Error Cases:**
- `400 Bad Request`: Invalid DAG structure (cycles, missing dependencies)
- `422 Unprocessable Entity`: Tasks or secrets don't exist

---

### 2. Get DAG Status

**Endpoint:** `GET /api/dags/{id}`

Get the current status and metadata of a DAG.

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "dag_xyz789",
    "name": "data-pipeline",
    "description": "Daily ETL pipeline",
    "state": "Running",
    "progress": {
      "total_tasks": 3,
      "completed": 1,
      "running": 1,
      "queued": 1,
      "failed": 0
    },
    "created_at": "2026-02-22T22:59:00Z",
    "started_at": "2026-02-22T22:59:05Z",
    "estimated_completion": "2026-02-22T23:30:00Z"
  }
}
```

---

### 3. Get DAG Tasks

**Endpoint:** `GET /api/dags/{id}/tasks`

List all tasks in a DAG with their current status.

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "dag_id": "dag_xyz789",
    "tasks": [
      {
        "id": "task_123abc",
        "name": "fetch_data",
        "state": "Completed",
        "duration_ms": 3500
      },
      {
        "id": "task_456def",
        "name": "process_data",
        "state": "Running",
        "duration_ms": 1200
      },
      {
        "id": "task_789ghi",
        "name": "notify",
        "state": "Queued"
      }
    ]
  }
}
```

---

### 4. Cancel DAG

**Endpoint:** `POST /api/dags/{id}/cancel`

Cancel all tasks in a DAG.

**Request:**
```json
{
  "reason": "User requested cancellation"
}
```

**Response (200 OK):**
```json
{
  "status": "success",
  "data": {
    "id": "dag_xyz789",
    "state": "Cancelled",
    "cancelled_tasks": 2,
    "cancelled_at": "2026-02-22T23:05:30Z"
  }
}
```

---

## System Health API

### 1. Health Check

**Endpoint:** `GET /api/health`

Check system health and readiness.

**Response (200 OK):**
```json
{
  "status": "healthy",
  "uptime_seconds": 86400,
  "version": "1.0.0",
  "components": {
    "database": "healthy",
    "workers": {
      "total": 42,
      "active": 40,
      "offline": 2
    },
    "tasks": {
      "running": 30,
      "queued": 20,
      "completed": 15000
    }
  },
  "timestamp": "2026-02-22T23:05:30Z"
}
```

---

### 2. Metrics

**Endpoint:** `GET /api/metrics`

Retrieve system metrics in Prometheus format.

**Response (200 OK - text/plain):**
```
# HELP vortex_workers_active Current number of active workers
# TYPE vortex_workers_active gauge
vortex_workers_active 40

# HELP vortex_tasks_completed_total Total completed tasks
# TYPE vortex_tasks_completed_total counter
vortex_tasks_completed_total 15000

# HELP vortex_task_duration_seconds Task execution duration
# TYPE vortex_task_duration_seconds histogram
vortex_task_duration_seconds_bucket{le="1"} 1000
vortex_task_duration_seconds_bucket{le="10"} 12000
vortex_task_duration_seconds_bucket{le="100"} 14900
```

---

## Error Codes

| Code | HTTP Status | Meaning |
|------|-------------|---------|
| `INVALID_REQUEST` | 400 | Malformed request body or invalid parameters |
| `UNAUTHORIZED` | 401 | Authentication required |
| `FORBIDDEN` | 403 | User lacks permission for this action |
| `NOT_FOUND` | 404 | Resource doesn't exist |
| `CONFLICT` | 409 | Resource already exists or state conflict |
| `UNPROCESSABLE` | 422 | Semantic error (e.g., invalid DAG structure) |
| `RATE_LIMITED` | 429 | Too many requests |
| `INTERNAL_ERROR` | 500 | Server error |
| `UNAVAILABLE` | 503 | Service temporarily unavailable |

---

## Related Documentation

- [Pillar 3: Secrets Vault](./PILLAR_3_SECRETS_VAULT.md) — Secrets management implementation
- [Pillar 4: Resilience](./PILLAR_4_RESILIENCE.md) — Task recovery and worker failure handling
- [Architecture Overview](./ARCHITECTURE.md) — System design and components
- [Deployment Guide](./DEPLOYMENT.md) — Setup and configuration
