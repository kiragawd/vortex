# High Availability (HA) in VORTEX

VORTEX is designed for ultra-low latency execution and minimalist deployment. Because of this, the core architecture leverages an in-memory topological struct and async `tokio` coroutines rather than a heavy, distributed metadata store like ZooKeeper or etcd.

However, running a single controller process introduces a Single Point of Failure (SPOF). This guide explains how to properly deploy VORTEX for production resilience.

---

## 1. Process Supervision (Recommended for 90%)

For the vast majority of workloads, true multi-node HA is unnecessary if **auto-recovery** is extremely fast. Since VORTEX starts up in milliseconds (compared to minutes for Airflow), the simplest and most effective "HA" strategy is to rely on an external supervisor to restart a crashed node.

*   **Kubernetes:** Deploy the VORTEX controller as a `Deployment` with `replicas: 1` and let the Kubelet instantly restart the pod if it fails.
*   **Systemd/Supervisord:** Configure the service unit with `Restart=always` and `RestartSec=1`.

Because VORTEX workers are stateless and use gRPC polling, they will automatically seamlessly reconnect to the controller once it restores.

---

## 2. Active-Standby Leader Election (Advanced)

If you must guarantee no downtime (e.g., zero single-machine SPOF) and intend to run multiple VORTEX controller instances simultaneously, you must orchestrate leader election to prevent split-brain execution (multiple controllers scheduling the same tasks).

VORTEX supports native active-standby leader election via **PostgreSQL Advisory Locks**.

### Enabling HA Mode

When starting the server, pass the `--ha-mode` flag:

```bash
vortex server --ha-mode --database-url "$DATABASE_URL"
```

### How it Works

1.  **Lock Acquisition:** Upon startup, the controller attempts to acquire a global, mutually-exclusive advisory lock on the Postgres database (`pg_try_advisory_lock(...)`).
2.  **Leader Role:** The instance that acquires the lock becomes the **Active Leader**, running the scheduler loops, SLA monitors, and the API.
3.  **Standby Role:** Any instance that fails to acquire the lock enters **Standby Mode**. In this mode, it routinely polls the database every 5 seconds, waiting for the lock to become free.
4.  **Failover:** If the Active Leader crashes, its database connection drops. Postgres automatically releases the advisory lock. Within 5 seconds, one of the Standby nodes will acquire the lock, promote itself to Leader, and resume scheduling pipeline execution exactly where the previous leader left off.

> **Note:** Even in Standby mode, instances can still safely route and respond to read-only API requests if placed behind a load balancer, as the state resides in Postgres.
