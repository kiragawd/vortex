# Deployment Guide — Installation, Configuration, and Operations

## Prerequisites

| Component | Requirement |
|-----------|-------------|
| **OS** | macOS, Linux, or Windows (WSL2) |
| **Rust** | 1.70+ stable |
| **Python** | 3.13+ or 3.14+ |
| **PostgreSQL** | 14+ (required) |
| **protoc** | Protocol Buffers compiler |
| **Disk** | ~500MB for build, ~50MB for binary |

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
# Production mode with PostgreSQL (required)
./target/release/vortex server --swarm --database-url "postgres://user:pass@localhost/vortex"

# Custom web port (default: 3000)
./target/release/vortex server --database-url "postgres://..." --port 8080

# Custom gRPC port and bind address (default port: 50051, default bind: 0.0.0.0)
# Use --grpc-bind 127.0.0.1 to restrict gRPC to localhost in single-host deployments
./target/release/vortex server --swarm --database-url "postgres://..." --swarm-port 50052 --grpc-bind 127.0.0.1

# Enable the built-in synthetic benchmark DAG (for scale testing)
./target/release/vortex server --swarm --database-url "postgres://..." --benchmark
```

The REST API and dashboard are served on **http://localhost:3000** (or the port specified by `--port`).

> **Note:** PostgreSQL is the only supported database backend. The `--database-url` flag is required for production use.

### Worker

```bash
# Connect to controller
./target/release/vortex worker --controller http://localhost:50051 --capacity 4

# With custom ID and labels
./target/release/vortex worker \
  --controller http://localhost:50051 \
  --capacity 8 \
  --id worker-gpu-01 \
  --labels gpu,high-memory
```

### Official CLI (`vortex-cli`)

VORTEX includes a separate binary (`vortex-cli`) for administrative automation:

```bash
# Set environment variables
export VORTEX_API_KEY="your_api_key_here"
export VORTEX_SERVER_URL="http://localhost:3000"

# Use the CLI
vortex-cli dags list
vortex-cli dags trigger my_pipeline
vortex-cli secrets set DB_PASS "password123"
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
./target/release/vortex server --swarm --database-url "postgres://..." --tls-cert cert.pem --tls-key key.pem
```

For production, use certificates from Let's Encrypt or your organization's CA.

Workers connecting to a TLS-enabled controller:

```bash
./target/release/vortex worker --controller https://localhost:50051 --capacity 4
```

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `VORTEX_SECRET_KEY` | For Secrets Vault | 32-character string used as AES-256-GCM key |
| `VORTEX_NODE_ID` | HA mode | Unique identifier for this controller node (auto-generated if unset) |
| `VORTEX_TASK_API_KEY` | Optional | Scoped API key injected into task processes for API access |
| `VORTEX_BASE_URL` | Optional | Base URL for task API access (default: `http://localhost:3000`) |
| `PYO3_USE_ABI3_FORWARD_COMPATIBILITY` | Python 3.14+ | Set to `1` for PyO3 compatibility |

### Generate Encryption Key

```bash
# Generate a 32-character key (32 bytes = 256 bits for AES-256)
export VORTEX_SECRET_KEY=$(head -c 32 /dev/urandom | LC_ALL=C tr -dc 'a-zA-Z0-9' | head -c 32)
echo "VORTEX_SECRET_KEY=$VORTEX_SECRET_KEY"
```

**Note:** The key must be exactly 32 characters (bytes). The raw string bytes are used directly as the AES-256 key (not hex-decoded). Without it, the Secrets Vault is disabled (non-fatal warning).

### Database

VORTEX requires **PostgreSQL 14+** for production use.

**Migrations:** Database schema is managed via SQLx. The server auto-migrates on startup when a `--database-url` is provided.

```bash
# Migrations run automatically on server start
./target/release/vortex server --database-url "postgres://user:pass@localhost/vortex"
```

**Connection string format:**
```
postgres://username:password@hostname:port/database_name
```

---

## Default User

On first run, VORTEX seeds a default admin user:

| Username | Password | Role |
|----------|----------|------|
| `admin` | `admin` | Admin |

**Passwords are bcrypt-hashed** before storage. Change the admin password immediately in production.

The admin user's API key is generated on first run and returned from the login endpoint. Use the login API to obtain it.

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

```sql
-- Active workers
SELECT id, hostname, state, last_heartbeat FROM workers;

-- Recent runs
SELECT id, dag_id, state, triggered_by FROM dag_runs ORDER BY execution_date DESC LIMIT 10;

-- Failed tasks
SELECT id, dag_id, task_id, state FROM task_instances WHERE state='Failed' ORDER BY execution_date DESC LIMIT 10;

-- Pool usage
SELECT p.name, p.slots, COUNT(ps.id) AS occupied FROM pools p LEFT JOIN pool_slots ps ON p.name = ps.pool_name GROUP BY p.name, p.slots;
```

---

## Backup & Recovery

### PostgreSQL Backup

```bash
# Backup
pg_dump -h localhost -U vortex_user vortex > vortex_backup_$(date +%Y%m%d).sql

# Restore
psql -h localhost -U vortex_user vortex < vortex_backup_20260225.sql
```

For automated backups, consider `pg_dump` cron jobs or PostgreSQL continuous archiving (WAL).

---

## Troubleshooting

| Problem | Solution |
|---------|----------|
| **Blank page at localhost:3000** | Check browser console for JS errors. Clear localStorage and refresh. |
| **Wrong port** | Use `--port <N>` to change the web server port (default 3000). |
| **Worker can't connect** | Ensure controller is running with `--swarm`. Check port 50051 is open. For internal deployments, use `--grpc-bind 127.0.0.1`. |
| **DAG not loading** | Check `dags/` directory exists. Look for parse errors in server log. |
| **Invalid schedule rejected** | Schedule expressions are validated on upload. Use `@daily`, `@hourly`, or valid 5/6/7-field cron. |
| **Secret Vault disabled** | Set `VORTEX_SECRET_KEY` env var (exactly 32 characters). |
| **"Unauthorized" errors** | Login via UI or pass API key with `Bearer` prefix in `Authorization` header. |
| **"Too many login attempts"** | Rate limit is 10 attempts per 60 seconds per username. Wait and retry. |
| **Tasks stuck in Running** | On restart, controller auto-marks interrupted tasks as Failed. |
| **Database connection errors** | Verify PostgreSQL is running and `--database-url` is correct. |

---

## Graceful Shutdown

VORTEX handles `SIGINT` (Ctrl+C) and `SIGTERM` gracefully:

1. All task instances currently in `Running` state are marked `Failed` in the database.
2. The HA leader lock is released (if running in `--ha-mode`).
3. The process exits with code 0.

This prevents tasks from being permanently stuck in the `Running` state after a controller restart.

> **Note:** Workers do not receive a drain signal — tasks already dispatched to workers will still complete. Only tasks tracked as Running by the controller (but not dispatched) are marked Failed.

---

## Health Check

All deployments expose a lightweight health endpoint:

```bash
curl http://localhost:3000/health
# {"status":"ok","version":"0.6.0","db":"connected"}
```

Returns `200 OK` when healthy, `503 Service Unavailable` when the DB is unreachable. Use this with load-balancer health probes, Kubernetes liveness/readiness checks, or monitoring tools.

---

## Security Headers

Every response from VORTEX includes:

| Header | Value |
|--------|-------|
| `X-Frame-Options` | `DENY` |
| `X-Content-Type-Options` | `nosniff` |
| `Content-Security-Policy` | Restricts scripts, styles, fonts, and images to `self` plus Google Fonts |
| `X-Request-ID` | Auto-generated UUID per request (or echoed from incoming `X-Request-ID`) |

---

## Security Limitations & Constraints

### 1. Python DAG Parsing (PyO3)
Currently, VORTEX parses and executes Python DAG files natively using the PyO3 runtime. **This executes actual Python code on the controller.** There is currently no AST-level sandboxing. 
> ⚠️ **You must ensure that only trusted personnel have write access to the `dags/` folder.** Do not process untrusted DAG definitions.

### 2. Worker gRPC Connections (TLS)
VORTEX workers currently connect to the controller's gRPC port over **plaintext HTTP/2**, even if the REST API frontend is behind a TLS reverse proxy.
> ⚠️ **Workers must run within a trusted private network (VPC).** Do not expose the Swarm gRPC port (`50051`) to the public internet.

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) — System design
- [API Reference](./API_REFERENCE.md) — REST API endpoints
- [CLI Reference](./CLI_REFERENCE.md) — CLI commands
- [Python Integration](./PHASE_2_PYTHON_INTEGRATION.md) — DAG authoring
- [Secrets Vault](./PILLAR_3_SECRETS_VAULT.md) — Secret management
- [Resilience](./PILLAR_4_RESILIENCE.md) — Auto-recovery on worker failure
- [High Availability](./high-availability.md) — HA deployment
