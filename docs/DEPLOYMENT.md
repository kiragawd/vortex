# Deployment Guide — Installation, Configuration, and Operations

## Prerequisites

| Component | Requirement |
|-----------|-------------|
| **OS** | macOS, Linux, or Windows (WSL2) |
| **Rust** | 1.70+ stable |
| **Python** | 3.13+ or 3.14+ |
| **protoc** | Protocol Buffers compiler |
| **Disk** | ~500MB for build, ~50MB for binary + DB |

## Build from Source

```bash
git clone https://github.com/kiragawd/vortex.git
cd vortex

# Required for Python 3.14+
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

# Development build
cargo build

# Release build (optimized)
cargo build --release
```

## Running

### Controller (Server)

```bash
# Basic mode (local execution only)
./target/debug/vortex server

# With Swarm mode (distributed workers via gRPC)
./target/debug/vortex server --swarm

# Custom gRPC port (default: 50051)
./target/debug/vortex server --swarm --swarm-port 50052
```

The REST API and dashboard are served on **http://localhost:3000**.

### Worker

```bash
# Connect to controller
./target/debug/vortex worker --controller http://localhost:50051 --capacity 4

# With custom ID and labels
./target/debug/vortex worker \
  --controller http://localhost:50051 \
  --capacity 8 \
  --id worker-gpu-01 \
  --labels gpu,high-memory
```

### Background Mode

```bash
# Server
nohup ./target/debug/vortex server --swarm > server.log 2>&1 &

# Worker
nohup ./target/debug/vortex worker --controller http://localhost:50051 --capacity 4 > worker.log 2>&1 &
```

---

## Configuration

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `VORTEX_SECRET_KEY` | For Pillar 3 | 32-byte key for AES-256-GCM encryption |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | Python 3.14+ | Set to `1` for PyO3 compatibility |

### Generate Encryption Key

```bash
# Generate a 32-character key (32 bytes)
export VORTEX_SECRET_KEY=$(head -c 32 /dev/urandom | LC_ALL=C tr -dc 'a-zA-Z0-9' | head -c 32)
echo "VORTEX_SECRET_KEY=$VORTEX_SECRET_KEY"
```

**Note:** The key must be exactly 32 bytes. Without it, the Secret Vault is disabled (non-fatal warning).

### Database

VORTEX creates `vortex.db` in the current working directory. All tables are auto-migrated on startup.

```bash
# Inspect database
sqlite3 vortex.db ".tables"
# dags  dag_runs  dag_versions  secrets  task_instances  tasks  users  workers

# Check integrity
sqlite3 vortex.db "PRAGMA integrity_check;"
```

### Default User

On first run, VORTEX seeds a default admin user:

| Username | Password | Role | API Key |
|----------|----------|------|---------|
| `admin` | `admin` | Admin | `vortex_admin_key` |

**⚠️ Change the admin password in production!**

---

## DAG Files

Place Python DAG files in the `dags/` directory. They are loaded automatically on server startup.

```bash
dags/
├── example_dag.py
├── parallel_benchmark.py
└── my_pipeline.py
```

DAGs can also be uploaded at runtime via:
- **Web UI:** Click "Upload" in the navbar
- **API:** `POST /api/dags/upload` with multipart form

---

## Monitoring

### Dashboard

Open **http://localhost:3000** for the built-in dashboard featuring:
- Real-time DAG stats (total, active, paused)
- Swarm worker count and queue depth
- Visual DAG dependency graphs
- Run history with per-run task breakdowns
- Task log viewer

### Server Logs

```bash
# Follow server output
tail -f server.log

# Task execution logs (per-dag/per-task)
ls logs/<dag_id>/<task_id>/
```

### Database Queries

```bash
# Active workers
sqlite3 vortex.db "SELECT id, hostname, state, last_heartbeat FROM workers;"

# Recent runs
sqlite3 vortex.db "SELECT id, dag_id, state, triggered_by FROM dag_runs ORDER BY execution_date DESC LIMIT 10;"

# Failed tasks
sqlite3 vortex.db "SELECT id, dag_id, task_id, state FROM task_instances WHERE state='Failed' ORDER BY execution_date DESC LIMIT 10;"
```

---

## Backup & Recovery

```bash
# Backup
cp vortex.db vortex.db.backup-$(date +%Y%m%d)

# Restore
pkill vortex
cp vortex.db.backup-20260225 vortex.db
./target/debug/vortex server --swarm
```

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| **Blank page at localhost:3000** | Check browser console for JS errors. Clear localStorage and refresh. |
| **Worker can't connect** | Ensure controller is running with `--swarm`. Check port 50051 is open. |
| **DAG not loading** | Check `dags/` directory exists. Look for parse errors in server log. |
| **Secret Vault disabled** | Set `VORTEX_SECRET_KEY` env var (exactly 32 bytes). |
| **"Unauthorized" errors** | Login via UI or pass API key in `Authorization` header. |
| **Tasks stuck in Running** | On restart, controller auto-marks interrupted tasks as Failed. |

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) — System design
- [API Reference](./API_REFERENCE.md) — REST API endpoints
- [Python Integration](./PHASE_2_PYTHON_INTEGRATION.md) — DAG authoring
- [Secrets Vault](./PILLAR_3_SECRETS_VAULT.md) — Secret management
- [Resilience](./PILLAR_4_RESILIENCE.md) — Auto-recovery
