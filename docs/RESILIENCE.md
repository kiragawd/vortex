# Resilience — High Availability & Auto-Recovery

## Overview

RYUO is built for reliability in distributed systems. Worker failures (crashes, network partitions, timeouts) are inevitable—the system must recover automatically without losing tasks or compromising data. The resilience layer implements a distributed health monitoring and task recovery mechanism that ensures zero task loss and automatic failover.

### RYUO's Auto-Recovery Approach

1. **Continuous Health Monitoring**: Controller runs a health check loop every 15 seconds
2. **Heartbeat Detection**: Workers send heartbeats every 15 seconds; missed heartbeats trigger recovery
3. **Automatic Re-queueing**: All running tasks on a dead worker are reset to Queued state
4. **Distributed Execution**: No worker is special; any healthy worker can pick up the re-queued task
5. **Zero Data Loss**: Database state is consistent; no task is dropped

---

## Worker Lifecycle

Workers transition through several states based on heartbeat activity and task execution:

### State Definitions

| State | Meaning | Triggers Recovery? |
|-------|---------|-------------------|
| **Active** | Worker alive, heartbeat received | No |
| **Running** | Task execution in progress | Yes (if no heartbeat for 60s) |
| **Offline** | No heartbeat for 60s (dead or network partition) | Yes |

---

## Health Monitoring

### Health Check Loop

The controller runs a `health_check_loop` that executes every 15 seconds. It uses the `DatabaseBackend` trait to perform recovery:

```rust
async fn health_check_loop(db: Arc<dyn DatabaseBackend>) -> Result<()> {
    loop {
        // Mark stale workers offline and requeue tasks
        if let Ok(stale_ids) = db.mark_stale_workers_offline(60).await {
            for id in stale_ids {
                let count = db.requeue_worker_tasks(&id).await?;
                info!("Worker {} offline; re-queued {} tasks", id, count);
            }
        }
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}
```

### Key Properties

| Property | Value | Description |
|----------|-------|-------------|
| **Check Interval** | 15 seconds | How often health check runs |
| **Heartbeat Timeout** | 60 seconds | Time before worker marked Offline |
| **Detection Latency** | 15-75 seconds | Worst case: worker dies just after check |
| **Database** | PostgreSQL | All state persisted and recovered from PostgreSQL |

---

## Task Re-queueing (Recovery)

### When a Worker Goes Offline

The controller performs these steps:

1. **Identify stale workers**: Workers whose `last_heartbeat` exceeds the 60s timeout
2. **Mark worker offline**: Update worker state to `Offline`
3. **Reset running tasks**: Mark tasks as `Queued`, clear `worker_id`
4. **Re-inject into queue**: Task re-enters the swarm task queue for pickup

### Recovery Guarantees

| Guarantee | Implementation |
|-----------|----------------|
| **Atomicity** | Worker marked offline + tasks re-queued in database operations |
| **No Duplication** | Task `id` is unique; re-queueing resets `worker_id`, doesn't create new tasks |
| **No Loss** | All tasks are accounted for; none are silently dropped |
| **Idempotency** | Re-queueing the same task multiple times is safe |

---

## Operational Monitoring

### Query Worker Status

```sql
-- Active workers
SELECT id, hostname, state, last_heartbeat,
       NOW() - last_heartbeat AS time_since_heartbeat
FROM workers
ORDER BY last_heartbeat DESC;
```

### Query Orphaned Tasks

```sql
-- Tasks that were re-queued (no worker assignment)
SELECT id, dag_id, task_id, state, worker_id, updated_at
FROM task_instances
WHERE state = 'Queued' AND worker_id IS NULL
ORDER BY updated_at;
```

### Query Failed Tasks

```sql
-- Recently failed tasks
SELECT id, dag_id, task_id, state, worker_id, updated_at
FROM task_instances
WHERE state = 'Failed'
ORDER BY updated_at DESC
LIMIT 20;
```

### Swarm API

Use the REST API to monitor worker health:

```bash
# Swarm overview
curl http://localhost:3000/api/swarm/status \
  -H "Authorization: Bearer <api_key>"

# List all workers
curl http://localhost:3000/api/swarm/workers \
  -H "Authorization: Bearer <api_key>"

# Drain a worker (graceful shutdown)
curl -X POST http://localhost:3000/api/swarm/workers/<id>/drain \
  -H "Authorization: Bearer <api_key>"
```

---

## Example: Worker Failure & Recovery

### Step-by-Step Scenario

**Time: 22:59:00** — Worker A is running task_123
```
Worker A: state = Active, last_heartbeat = 22:59:00
Task 123: state = Running, worker_id = worker_a
```

**Time: 22:59:15** — Worker A crashes (unexpected failure)
```
Worker A: state = Active (no update yet — check hasn't fired)
Task 123: state = Running (unchanged)
```

**Time: 23:00:00** — Health check runs, Worker A is 60 seconds stale
```
Detection: Worker A last heartbeat at 22:59:00, now 23:00:00 = 60s timeout

Action:
  - UPDATE workers SET state = 'Offline' WHERE id = 'worker_a'
  - UPDATE task_instances SET state = 'Queued', worker_id = NULL
    WHERE worker_id = 'worker_a' AND state = 'Running'
  - Log: "Worker worker_a marked Offline; re-queued 1 task (task_123)"
```

**Time: 23:00:05** — Another worker picks up the re-queued task
```
Worker B: Polls for tasks, receives task_123 (now Queued)
Task 123: state = Running, worker_id = worker_b (restarted from scratch)
```

**Time: 23:05:00** — Task completes
```
Task 123: state = Success
Total recovery time: ~60 seconds (detection + re-queueing)
```

### Recovery Summary

| Metric | Value |
|--------|-------|
| **Worker crash time** | 22:59:15 |
| **Detection time** | 23:00:00 (45 seconds later) |
| **Re-queue time** | 23:00:00 (immediate after detection) |
| **Task restart time** | 23:00:05 |
| **Total downtime** | ~50 seconds |
| **Task status** | Recovered (not lost) |

---

## Scaling Considerations

### Performance at Scale

| Operation | Complexity | Optimization |
|-----------|-----------|--------------|
| **Health Check** | O(n) workers | Indexed query on `state` + `last_heartbeat` |
| **Re-queueing** | O(m) tasks per worker | Batch update in database |
| **Heartbeat Processing** | O(1) per worker | Index on `id`, single row update |
| **Task Distribution** | O(1) per task | Worker polls from queue |

### Recommended Indexes

```sql
-- For health check performance
CREATE INDEX idx_workers_state_heartbeat ON workers(state, last_heartbeat);

-- For task re-queueing
CREATE INDEX idx_task_instances_worker_state ON task_instances(worker_id, state);

-- For monitoring queries
CREATE INDEX idx_task_instances_state ON task_instances(state);
```

---

## Best Practices

### 1. Use Process Supervision

Run RYUO behind a supervisor for automatic restart on failure:

- **Kubernetes:** `Deployment` with `replicas: 1` and liveness probe
- **systemd:** `Restart=always` with `RestartSec=1`

### 2. Implement Graceful Worker Shutdown

Workers should:
1. Stop accepting new tasks (drain mode via API)
2. Wait for in-flight tasks to complete
3. Send final heartbeat
4. Exit cleanly

### 3. Monitor Worker Health

Set up alerts when offline worker count exceeds a threshold:

```bash
# Check worker health via API
curl -s http://localhost:3000/api/swarm/workers \
  -H "Authorization: Bearer <api_key>" | \
  jq '.workers | map(select(.status == "offline")) | length'
```

### 4. Test Failure Scenarios

- Kill random workers during DAG execution
- Simulate network partitions (firewall rules)
- Verify all tasks complete without loss

---

## Troubleshooting

### Workers Stuck in Offline State

**Symptom:** Worker shows as Offline but is actually running.

**Cause:** Network partition or controller unreachable.

**Solution:**
1. Check network connectivity between worker and controller
2. Verify gRPC port 50051 is accessible
3. Restart the worker to re-register

### Task Re-queued Multiple Times

**Symptom:** Same task appears in re-queue logs repeatedly.

**Cause:** Task always fails (crash, timeout, etc.) on every worker.

**Solution:**
1. Check task logs via API: `GET /api/tasks/<id>/logs`
2. Identify root cause (resource exhaustion, missing dependency)
3. Fix the DAG and re-trigger

### High False Positive Offline Detections

**Symptom:** Workers marked Offline while still running.

**Cause:** Heartbeat timeout too aggressive for your network.

**Solution:** This requires a code change to increase the heartbeat timeout constant (default: 60 seconds). Consider deploying on a lower-latency network.

---

## Related Documentation

- [Architecture Overview](./ARCHITECTURE.md) — System design and component interactions
- [API Reference](./API_REFERENCE.md) — Complete endpoint documentation
- [Deployment Guide](./DEPLOYMENT.md) — Setup and operational procedures
- [Secrets Vault](./SECRETS_VAULT.md) — Encrypted secret storage
- [High Availability](./high-availability.md) — HA with leader election

---

## Disaster Recovery

**Module:** `src/disaster_recovery.rs`

### Backup Metadata

In-memory backup tracking and metadata management:

- `BackupRecord` — Records backup ID, timestamp, status, and storage location
- `BackupStatus` — Tracks backup lifecycle: `Pending`, `InProgress`, `Completed`, `Failed`
- `BackupConfig` — Defines backup schedule, retention, and target storage

### Failover

Cluster node and health state type definitions for multi-region deployments:

- Node health states for active/standby cluster members
- Failover trigger conditions based on health check failures

### Chaos Testing

Chaos test hook definitions for validating system resilience:

- Simulated worker crashes during task execution
- Network partition scenarios between controller and workers
- Database connectivity interruption testing

### Current Status

Backup I/O and automated restore are not yet operational. Metadata tracking and type definitions are in place. Use PostgreSQL-native backup tools (`pg_dump`, `pg_basebackup`) for production backup needs in the interim.
