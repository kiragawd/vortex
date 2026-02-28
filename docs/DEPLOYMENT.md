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
# Basic mode (SQLite development mode)
./target/debug/vortex server

# Production mode with PostgreSQL
./target/debug/vortex server --swarm --database-url "postgres://user:pass@localhost/vortex"

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

### Official CLI (`vortex-cli`)

VORTEX includes a binary for administrative automation:

```bash
# Set environment variables
export VORTEX_API_KEY="vortex_admin_key"
export VORTEX_SERVER_URL="http://localhost:3000"

# Use the CLI
vortex dags list
vortex dags trigger my_pipeline
vortex secrets set DB_PASS "password123"
```

---

## Configuration

### TLS / HTTPS

Generate self-signed certificates for development:

```bash
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes \
  -subj "/CN=localhost"
```

Run with TLS (both HTTP and gRPC):

```bash
./target/debug/vortex server --swarm --tls-cert cert.pem --tls-key key.pem
```

For production, use certificates from Let's Encrypt or your organization's CA.

Workers connecting to a TLS-enabled controller:

```bash
./target/debug/vortex worker --controller https://localhost:50051 --capacity 4
```

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `VORTEX_SECRET_KEY` | For Pillar 3 | 32-byte key for AES-256-GCM encryption |
| `VORTEX_DATABASE_URL` | Optional | Connection string for PostgreSQL |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | Python 3.14+ | Set to `1` for PyO3 compatibility |

### Generate Encryption Key

```bash
# Generate a 32-character key (32 bytes)
export VORTEX_SECRET_KEY=$(head -c 32 /dev/urandom | LC_ALL=C tr -dc 'a-zA-Z0-9' | head -c 32)
echo "VORTEX_SECRET_KEY=$VORTEX_SECRET_KEY"
```

**Note:** The key must be exactly 32 bytes. Without it, the Secret Vault is disabled (non-fatal warning).

### Database

VORTEX supports PostgreSQL and SQLite. For production, **PostgreSQL 14+** is highly recommended.

**Migrations:**
Database schema is managed via `sqlx`. Run migrations manually or let the server auto-migrate on startup:

```bash
vortex db migrate --database-url "postgres://..."
```

**SQLite (Local Dev):**
VORTEX defaults to `vortex.db` in the current working directory if no `--database-url` is provided.

---

## Default User

On first run, VORTEX seeds a default admin user:

| Username | Password | Role | API Key |
|----------|----------|------|---------|
| `admin` | `admin` | Admin | `vortex_admin_key` |

**Passwords are bcrypt-hashed** before storage. Change the admin password immediately.

---

## DAG Files

Place Python DAG files in the `dags/` directory. They are loaded automatically on server startup. VORTEX supports dynamic DAG generation and Task Groups.

```python
from vortex import DAG, BashOperator, TaskGroup

with DAG("dynamic_pipeline", schedule_interval="@daily") as dag:
    with TaskGroup("processing") as tg:
        for i in range(5):
             BashOperator(task_id=f"task_{i}", bash_command=f"echo {i}")
```

---

## Monitoring

### Dashboard

Open **http://localhost:3000** for the built-in dashboard featuring:
- Real-time DAG stats and aggregation
- Gantt Timeline execution visualization
- Monthly schedule Calendar
- Side-by-side version diffing and rollbacks
- Audit logging (Accountability trail)
- Prometheus metrics endpoint (`/metrics`)

### Server Logs

```bash
# Follow server output (Structured JSON or Text)
tail -f logs/vortex.log
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
- [Resilience](./PILLAR_4_RESILIENCE.md) — Auto-recovery on worker failure
