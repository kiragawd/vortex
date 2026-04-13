# High Availability (HA) in RYUO

RYUO is designed for ultra-low latency execution and minimalist deployment. Because of this, the core architecture leverages an in-memory topological struct and async `tokio` coroutines rather than a heavy, distributed metadata store like ZooKeeper or etcd.

However, running a single controller process introduces a Single Point of Failure (SPOF). This guide explains how to properly deploy RYUO for production resilience.

---

## 1. Process Supervision (Recommended for 90%)

For the vast majority of workloads, true multi-node HA is unnecessary if **auto-recovery** is extremely fast. Since RYUO starts up in milliseconds (compared to minutes for Airflow), the simplest and most effective "HA" strategy is to rely on an external supervisor to restart a crashed node.

*   **Kubernetes:** Deploy the RYUO controller as a `Deployment` with `replicas: 1` and let the Kubelet instantly restart the pod if it fails.
*   **Systemd/Supervisord:** Configure the service unit with `Restart=always` and `RestartSec=1`.

Because RYUO workers are stateless and use gRPC polling, they will automatically seamlessly reconnect to the controller once it restores.

---

## 2. Active-Standby Leader Election (Advanced)

If you must guarantee no downtime (e.g., zero single-machine SPOF) and intend to run multiple RYUO controller instances simultaneously, you must orchestrate leader election to prevent split-brain execution (multiple controllers scheduling the same tasks).

RYUO supports native active-standby leader election via **PostgreSQL Advisory Locks**.

### Enabling HA Mode

When starting the server, pass the `--ha-mode` flag:

```bash
ryuo server --ha-mode --database-url "$DATABASE_URL"
```

### How it Works

1. **Leader Lease Acquisition:** Upon startup, the controller attempts to insert a row into the `leader_election` table (`lock_key = 1`, `node_id`, `expires_at = NOW() + 30s`). This succeeds only if there is no unexpired row owned by another node.
2. **Leader Role:** The instance that wins the upsert becomes the **Active Leader**, running the scheduler loops, SLA monitors, and the API.
3. **Lease Renewal:** The Active Leader renews its lease every **10 seconds** (3× headroom before the 30s expiry). This renewal is connection-agnostic — any pool connection can execute the UPDATE, unlike session-scoped advisory locks.
4. **Standby Role:** Instances that don't hold the lease continuously retry every 10 seconds. They wait for the in-memory `leader_rx` watch channel to signal `true` before starting background loops.
5. **Failover:** If the Active Leader crashes or stops renewing, its lease expires within 30 seconds. The next Standby to retry will upsert the row, acquire the lease, and promote itself to Leader.
6. **Stepdown:** If a running leader loses the lease in a race (very rare), it detects `try_acquire_leader_lock()` returning `false` and calls `leader_tx.send(false)` to step down gracefully.

### Node Identity

Each controller instance is identified by a `node_id`. Set this explicitly:

```bash
export RYUO_NODE_ID="controller-primary"
ryuo server --ha-mode --database-url "$DATABASE_URL"
```

If not set, a random ID is generated at startup (e.g., `node-a1b2c3d4`). In Kubernetes, a good value is the pod name (`$POD_NAME`).

> **Note:** The previous implementation used `pg_try_advisory_lock`, which is session-scoped and can be silently released by connection pool recycling, causing split-brain. This has been fixed (Bug #15) with the `leader_election` table approach.

