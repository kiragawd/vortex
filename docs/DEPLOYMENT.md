# Deployment Guide — Installation, Configuration, and Operations

## Prerequisites

Before deploying VORTEX, ensure your system has the following:

### System Requirements

| Component | Requirement | Details |
|-----------|-------------|---------|
| **OS** | Linux, macOS, or Windows (WSL2) | Any modern OS with Docker support |
| **CPU** | 2+ cores (controller), 1+ core per worker | Proportional to task concurrency |
| **Memory** | 2GB+ (controller), 1GB+ per worker | More for memory-intensive tasks |
| **Disk** | 10GB+ | For database, logs, and container images |
| **Network** | Low-latency LAN or cloud VPC | <100ms latency recommended |

### Software Dependencies

| Tool | Version | Purpose |
|------|---------|---------|
| **Rust** | 1.70+ | Compiling VORTEX from source |
| **Python** | 3.13+ or 3.14+ | Task execution and tooling |
| **Docker** | 20.10+ or Podman | Container execution |
| **SQLite** | 3.36+ | Built-in to VORTEX |
| **OpenSSL** | 1.1+ | Encryption and TLS (optional) |

### Optional Components

- **Docker Registry** (Harbor, ECR, Artifactory) — For private container images
- **Vault/Secrets Manager** — For rotating `VORTEX_SECRET_KEY`
- **Prometheus + Grafana** — For advanced monitoring
- **PostgreSQL** — Future alternative to SQLite

---

## Installation

### 1. Clone Repository

```bash
cd /Users/ashwin/vortex
git clone https://github.com/ashwin/vortex.git .
# or if already cloned:
cd /Users/ashwin/vortex
```

### 2. Install Dependencies

**macOS (Homebrew):**
```bash
brew install rust python@3.13 docker
```

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install -y rustc cargo python3.13 docker.io
sudo usermod -aG docker $USER  # Add user to docker group
newgrp docker                   # Activate docker group
```

**Windows (WSL2):**
```bash
# In WSL2 terminal
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
apt-get update && apt-get install -y python3.13 docker.io
```

### 3. Build from Source

```bash
cd /Users/ashwin/vortex

# Set Python ABI compatibility (if using Python 3.14)
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1

# Build release binary
cargo build --release

# Verify build succeeded
./target/release/vortex --version
# Output: vortex 1.0.0
```

### 4. Generate Encryption Key

```bash
# Generate a 32-byte hex string for AES-256
VORTEX_SECRET_KEY=$(openssl rand -hex 32)
echo "VORTEX_SECRET_KEY=$VORTEX_SECRET_KEY"

# Save to environment file
cat > .env <<EOF
VORTEX_SECRET_KEY=$VORTEX_SECRET_KEY
VORTEX_DATABASE_PATH=./vortex.db
VORTEX_LISTEN_PORT=8080
VORTEX_LISTEN_HOST=0.0.0.0
EOF
```

### 5. Create Configuration File

```bash
cat > vortex.toml <<EOF
[controller]
# Heartbeat timeout before marking worker OFFLINE
heartbeat_timeout_secs = 60

# Health check loop interval
health_check_interval_secs = 15

# Maximum task retries
max_task_retries = 3

# Task execution timeout (per task, overridable in DAG)
task_timeout_secs = 3600

[database]
# SQLite database file path
path = "./vortex.db"

# Connection pool size
pool_size = 10

[api]
# REST API listening host and port
host = "0.0.0.0"
port = 8080

# TLS configuration (optional)
# tls_cert_path = "/path/to/cert.pem"
# tls_key_path = "/path/to/key.pem"

[monitoring]
# Log level: debug, info, warn, error
log_level = "info"

# Enable Prometheus metrics endpoint
metrics_enabled = true

# Enable request logging
request_logging = true
EOF
```

---

## Running the Controller

### Single-Process Mode

```bash
# Load environment and run
export $(cat .env | xargs)
./target/release/vortex --config vortex.toml
```

Output:
```
[INFO] VORTEX Controller v1.0.0 starting
[INFO] Database: ./vortex.db (initialized)
[INFO] Listening on http://0.0.0.0:8080
[INFO] Health check loop: every 15 seconds
[INFO] Ready to accept DAG submissions
```

### High-Availability Mode (Future)

```bash
# Multiple controller instances with shared database
./target/release/vortex --config vortex.toml --instance 1
./target/release/vortex --config vortex.toml --instance 2
./target/release/vortex --config vortex.toml --instance 3

# Load balancer (nginx, HAProxy, etc.) distributes requests
```

### Docker Deployment

```bash
# Build Docker image
docker build -t vortex-controller:latest .

# Run container
docker run -d \
  --name vortex-controller \
  -p 8080:8080 \
  -v $(pwd)/vortex.db:/app/vortex.db \
  -e VORTEX_SECRET_KEY=$VORTEX_SECRET_KEY \
  vortex-controller:latest
```

---

## Registering Workers

Workers register with the controller after startup. Each worker needs network access to the controller.

### Run a Worker Locally

```bash
# In a new terminal
export $(cat .env | xargs)
export VORTEX_CONTROLLER_URL=http://localhost:8080

./target/release/vortex --worker --name "worker-01"
```

Output:
```
[INFO] Worker worker-01 starting
[INFO] Registering with controller at http://localhost:8080
[INFO] Registered as worker_abc123
[INFO] Polling for tasks every 500ms
```

### Run Multiple Workers

```bash
# Start 4 workers in parallel
for i in {1..4}; do
  VORTEX_CONTROLLER_URL=http://localhost:8080 \
  ./target/release/vortex --worker --name "worker-$i" &
done

# View running workers
curl http://localhost:8080/api/workers | jq '.data.workers'
```

### Docker Compose Orchestration

```yaml
# docker-compose.yml
version: '3.8'

services:
  controller:
    image: vortex-controller:latest
    ports:
      - "8080:8080"
    environment:
      VORTEX_SECRET_KEY: ${VORTEX_SECRET_KEY}
      VORTEX_DATABASE_PATH: /data/vortex.db
    volumes:
      - ./data:/data
      - /var/run/docker.sock:/var/run/docker.sock
    networks:
      - vortex

  worker-1:
    image: vortex-worker:latest
    environment:
      VORTEX_CONTROLLER_URL: http://controller:8080
      VORTEX_WORKER_NAME: worker-1
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
    depends_on:
      - controller
    networks:
      - vortex

  worker-2:
    image: vortex-worker:latest
    environment:
      VORTEX_CONTROLLER_URL: http://controller:8080
      VORTEX_WORKER_NAME: worker-2
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
    depends_on:
      - controller
    networks:
      - vortex

networks:
  vortex:
    driver: bridge
```

Run with:
```bash
docker-compose up -d
```

---

## Configuration Reference

### Controller Configuration

```toml
[controller]
heartbeat_timeout_secs = 60          # Time before worker marked OFFLINE
health_check_interval_secs = 15      # Health check frequency
max_task_retries = 3                 # Retry failed tasks N times
task_timeout_secs = 3600             # Default per-task timeout
```

### Worker Configuration

```toml
[worker]
controller_url = "http://localhost:8080"  # Controller address
name = "worker-01"                         # Worker name (optional; auto-generated)
heartbeat_interval_secs = 10               # Heartbeat frequency
max_concurrent_tasks = 5                   # Tasks to execute in parallel
docker_timeout_secs = 3600                 # Container execution timeout
```

### Database Configuration

```toml
[database]
path = "./vortex.db"              # SQLite file location
pool_size = 10                    # Connection pool size
wal_mode = true                   # Write-Ahead Logging (faster)
pragma_journal_mode = "WAL"       # WAL pragma
pragma_synchronous = "NORMAL"     # Sync mode (faster for batch ops)
```

### Security Configuration

```toml
[security]
# Encryption key for secrets (required)
secret_key_env = "VORTEX_SECRET_KEY"

# TLS for API (optional but recommended for production)
tls_enabled = false
tls_cert_path = "/path/to/cert.pem"
tls_key_path = "/path/to/key.pem"

# Authentication (future)
auth_enabled = false
# auth_provider = "oauth2"
```

### Monitoring Configuration

```toml
[monitoring]
log_level = "info"                    # Log verbosity
metrics_enabled = true                # Enable Prometheus endpoint (/metrics)
request_logging = true                # Log HTTP requests
access_log_path = "./logs/access.log" # Access log file
error_log_path = "./logs/error.log"   # Error log file
```

---

## Monitoring & Observability

### Health Check Endpoint

```bash
curl http://localhost:8080/api/health | jq '.'
```

Output:
```json
{
  "status": "healthy",
  "uptime_seconds": 3600,
  "version": "1.0.0",
  "components": {
    "database": "healthy",
    "workers": {"total": 4, "active": 4, "offline": 0},
    "tasks": {"running": 8, "queued": 12, "completed": 450}
  },
  "timestamp": "2026-02-22T23:05:30Z"
}
```

### Prometheus Metrics

```bash
curl http://localhost:8080/api/metrics | head -20
```

Output:
```
# HELP vortex_workers_active Current active workers
# TYPE vortex_workers_active gauge
vortex_workers_active 4

# HELP vortex_tasks_completed_total Total completed tasks
# TYPE vortex_tasks_completed_total counter
vortex_tasks_completed_total 450

# HELP vortex_task_duration_seconds Task execution time
# TYPE vortex_task_duration_seconds histogram
vortex_task_duration_seconds_bucket{le="1"} 100
vortex_task_duration_seconds_bucket{le="10"} 300
vortex_task_duration_seconds_bucket{le="100"} 450
```

### Log Files

```bash
# Real-time controller logs
tail -f logs/vortex.log

# Filter by level
grep ERROR logs/vortex.log
grep "recovery\|offline" logs/vortex.log  # Worker recovery events

# Worker logs
tail -f logs/worker-01.log
```

### Database Inspection

```bash
# Connect to SQLite
sqlite3 vortex.db

# Query worker status
SELECT id, name, state, last_heartbeat FROM workers;

# Query task status
SELECT id, name, state, worker_id, exit_code FROM task_instances LIMIT 10;

# Query secret count
SELECT COUNT(*) as secret_count FROM secrets;

# Find failed tasks
SELECT id, name, error_message FROM task_instances WHERE state = 'Failed';
```

---

## Backup & Recovery

### Backup Database

```bash
# One-off backup
cp vortex.db vortex.db.backup-$(date +%Y%m%d-%H%M%S)

# Automated daily backup (cron)
0 2 * * * cp /path/to/vortex.db /backups/vortex.db.$(date +\%Y\%m\%d) >> /var/log/vortex-backup.log 2>&1
```

### Restore from Backup

```bash
# Stop VORTEX
pkill vortex

# Restore database
cp vortex.db.backup-20260222-120000 vortex.db

# Restart
./target/release/vortex --config vortex.toml
```

### Database Integrity Check

```bash
# Verify database integrity
sqlite3 vortex.db "PRAGMA integrity_check;"
# Output: ok (if no issues)

# Rebuild indices (if corrupted)
sqlite3 vortex.db "REINDEX;"
```

---

## Troubleshooting

### Controller Won't Start

**Error:** `[ERROR] Failed to open database: file is locked`

**Solution:**
```bash
# Another process is using the database
pkill -f vortex

# Or wait and retry
sleep 5
./target/release/vortex --config vortex.toml
```

### Workers Registering But Not Picking Up Tasks

**Error:** Workers stay in IDLE state

**Solution:**
```bash
# Check controller is running
curl http://localhost:8080/api/health

# Verify workers are connected
curl http://localhost:8080/api/workers

# Check for errors in logs
tail -f logs/vortex.log | grep ERROR
```

### Task Stuck in RUNNING State

**Error:** Task never completes

**Solution:**
```bash
# Check if container is still running
docker ps | grep vortex

# Check task logs
curl http://localhost:8080/api/tasks/<task_id>/logs

# Force timeout (if configured)
curl -X POST http://localhost:8080/api/tasks/<task_id>/cancel
```

### Decryption Errors for Secrets

**Error:** `[ERROR] Decryption failed: authentication tag mismatch`

**Cause:** Wrong `VORTEX_SECRET_KEY` or database corruption

**Solution:**
```bash
# Verify key is set correctly
echo $VORTEX_SECRET_KEY | wc -c  # Should be 65 (64 hex + newline)

# Check database integrity
sqlite3 vortex.db "PRAGMA integrity_check;"
```

---

## Performance Tuning

### SQLite Optimization

```bash
# Enable WAL mode (faster concurrent access)
sqlite3 vortex.db "PRAGMA journal_mode=WAL;"

# Increase page cache
sqlite3 vortex.db "PRAGMA cache_size=10000;"

# Verify optimizations
sqlite3 vortex.db "PRAGMA journal_mode; PRAGMA cache_size;"
```

### Worker Throughput

Adjust worker concurrency:
```toml
[worker]
max_concurrent_tasks = 5  # Increase for more parallelism
docker_timeout_secs = 3600 # Decrease for faster failures
```

### Controller Responsiveness

Tune health check frequency:
```toml
[controller]
health_check_interval_secs = 10  # More frequent checks (higher CPU)
heartbeat_timeout_secs = 45      # Faster failure detection
```

---

## Security Hardening

### 1. Enable TLS

```toml
[security]
tls_enabled = true
tls_cert_path = "/etc/vortex/cert.pem"
tls_key_path = "/etc/vortex/key.pem"
```

### 2. Rotate Secret Key Regularly

```bash
# Generate new key
NEW_KEY=$(openssl rand -hex 32)

# Backup current key
echo "Old key: $VORTEX_SECRET_KEY" >> keys.backup

# Update environment
export VORTEX_SECRET_KEY=$NEW_KEY

# Controller will re-encrypt secrets with new key on next startup
./target/release/vortex --config vortex.toml
```

### 3. Restrict API Access

Use a reverse proxy (nginx):
```nginx
server {
    listen 443 ssl;
    server_name vortex.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    # Allow only authenticated users
    auth_basic "VORTEX";
    auth_basic_user_file /etc/nginx/.htpasswd;

    location / {
        proxy_pass http://localhost:8080;
    }
}
```

### 4. Enable Audit Logging

```bash
# All secret access is logged (via request logging)
grep "POST /api/secrets\|GET /api/secrets" logs/access.log

# Correlate with task execution
grep "task_123abc" logs/vortex.log
```

---

## Upgrade & Migration

### In-Place Upgrade

```bash
# Stop controller
pkill vortex

# Backup database
cp vortex.db vortex.db.pre-upgrade

# Build new version
git pull origin main
cargo build --release

# Start new version
./target/release/vortex --config vortex.toml

# Verify
curl http://localhost:8080/api/health
```

### Blue-Green Deployment

```bash
# Start new controller on different port
VORTEX_LISTEN_PORT=8081 ./target/release/vortex --config vortex.toml &

# Verify health
curl http://localhost:8081/api/health

# Switch load balancer to port 8081
# Shut down old controller
pkill -9 -f "vortex.*--config vortex.toml"

# Start new controller on port 8080
VORTEX_LISTEN_PORT=8080 ./target/release/vortex --config vortex.toml &
```

---

## Production Checklist

- [ ] Database backed up daily
- [ ] TLS enabled for API
- [ ] Secret key stored securely (Vault/Secrets Manager)
- [ ] Monitoring enabled (Prometheus/Grafana)
- [ ] Log aggregation configured (ELK, Splunk, etc.)
- [ ] High-availability controller setup
- [ ] Worker auto-scaling configured
- [ ] Disaster recovery plan tested
- [ ] Security audit completed
- [ ] Load testing completed (100+ workers)

---

## Related Documentation

- [Pillar 3: Secrets Vault](./PILLAR_3_SECRETS_VAULT.md) — Secret management
- [Pillar 4: Resilience](./PILLAR_4_RESILIENCE.md) — Auto-recovery mechanisms
- [Architecture Overview](./ARCHITECTURE.md) — System design
- [API Reference](./API_REFERENCE.md) — REST API endpoints
