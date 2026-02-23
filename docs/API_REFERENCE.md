# API Reference — Complete Endpoint Documentation

## Overview

VORTEX exposes a comprehensive REST API for managing DAGs, tasks, workers, and secrets. All endpoints return JSON responses and support standard HTTP status codes.

### Base URL

```
http://localhost:8080/api
```

### Authentication

Most endpoints require a Bearer token or API key passed in the `Authorization` header.

```
Authorization: Bearer <api_key>
```

---

## DAG Submission & Validation API

### 1. Upload DAG

**Endpoint:** `POST /api/dags/upload`

Upload a Python DAG file. The file is validated and stored in the `dags/` directory.

**Request (Multipart Form):**
- `file`: The `.py` DAG file.

**Response (200 OK):**
```json
{
  "dag_id": "example_dag",
  "description": "An example DAG",
  "schedule_interval": "0 12 * * *",
  "tasks": [...],
  "edges": [...]
}
```

**Error Cases:**
- `400 Bad Request`: File missing, not a `.py` file, or invalid DAG structure (e.g., cycles, missing imports).

---

### 2. Validate DAG

**Endpoint:** `GET /api/dags/{id}/validate`

Validate an existing DAG's latest version without modifying state.

**Response (200 OK):**
```json
{
  "valid": true,
  "metadata": { ... }
}
```

**Error Cases:**
- `404 Not Found`: DAG ID not recognized.
- `400 Bad Request`: DAG file is currently invalid.

---

## Secrets Management API

### 1. Create Secret

**Endpoint:** `POST /api/secrets`

Create a new encrypted secret.

**Request:**
```json
{
  "key": "db_password",
  "value": "super_secret_password"
}
```

**Response (200 OK):**
```json
{
  "message": "Secret stored successfully"
}
```

---

## Worker Management API

### 1. List All Workers

**Endpoint:** `GET /api/swarm/workers`

List all registered workers with their status.

**Response (200 OK):**
```json
{
  "workers": [
    {
      "id": "worker_xyz789",
      "hostname": "vortex-worker-1",
      "state": "Active",
      "capacity": 4,
      "active_tasks": 1
    }
  ]
}
```

---

## Task Management API

### 1. Get Task Logs

**Endpoint:** `GET /api/tasks/{id}/logs`

Retrieve logs from a task instance.

**Response (200 OK):**
```json
{
  "logs": "2026-02-22T22:59:00Z [INFO] Task started..."
}
```

---

## DAG Management API

### 1. List DAGs

**Endpoint:** `GET /api/dags`

List all registered DAGs.

**Response (200 OK):**
```json
[
  {
    "id": "example_dag",
    "is_paused": false,
    "schedule_interval": "0 12 * * *",
    "next_run": "2026-02-23T12:00:00Z"
  }
]
```

---

### 2. Trigger DAG Run

**Endpoint:** `POST /api/dags/{id}/trigger`

Manually trigger a DAG execution.

**Response (200 OK):**
```json
{
  "message": "Triggered"
}
```
