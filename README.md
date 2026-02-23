# VORTEX 🌪️

**VORTEX** is a high-performance, single-binary enterprise orchestration tool designed to replace Apache Airflow with disruptive speed and simplicity.

## Vision

Airflow is powerful but often slow, complex to deploy, and resource-intensive. VORTEX aims to be **100x faster** while maintaining a compatible feel (and eventually API/Shim support) for Airflow DAGs.

### Key Pillars

- **🚀 Disruptive Speed:** Built in Rust with a lock-free, async-first scheduler (Tokio).
- **📦 Single Binary:** No more managing complex Python environments, Redis, or Celery. A single binary for everything.
- **🔋 Battery-Included Simplicity:** Lightweight state store (SQLite/Sled) by default, with pluggable backends for scale.
- **🐍 Python-Friendly:** First-class support for Python DAGs via a high-performance Rust-Python bridge.

## License

**VORTEX is dual-licensed:**

- **Personal & Open Source:** MIT License — Free for personal projects, education, research, and non-commercial open source work
- **Enterprise:** Commercial license required for business use, SaaS, or revenue-generating applications

See [LICENSE.md](./LICENSE.md) for complete terms and pricing.

**Quick checklist:**
- ✅ Personal hobby project? → Use Personal License (free)
- ✅ Startup/company < 10 people? → Reach out, we'll work with you
- ✅ Production commercial use? → Enterprise License required

Questions? Email: `licensing@vortex.dev`

## Technical Roadmap

### Phase 1: Foundation (Current)
- [x] Project Initialization
- [x] Core Async Scheduler (Tokio)
- [x] Lightweight State Store (SQLite) - Dependency added, integration next.
- [x] Basic DAG Execution Model
- [x] "Hello World" DAG in Rust

### Phase 2: Python Integration
- [x] PyO3-based DAG Parser
- [x] Airflow-compatible Shim layer
- [x] Python Task Executor (isolated execution)
- [x] Full Integration Test Suite (End-to-End Pipeline Verified)

### Phase 3: Enterprise Features (Current)
- [x] Web UI (built-in, Axum + Vanilla JS + Tailwind)
- [x] Secrets Vault (Pillar 3 - AES-256-GCM encryption)
- [x] Resilience & Auto-Recovery (Pillar 4 - worker health monitoring, task re-queueing)
- [x] RBAC (Admin/Operator/Viewer roles)
- [ ] Distributed Execution (Optional/Pluggable)
- [ ] Advanced Metrics & Observability

## Web Dashboard

A built-in web UI is included at `http://localhost:8080` (when the server is running):

- **DAG Management** — View, trigger, pause, schedule, and backfill DAGs
- **Swarm Monitoring** — Real-time worker status, queue depth, health checks
- **Secrets Management** — Encrypted secret vault with CRUD operations
- **RBAC & Users** — User management with Admin/Operator/Viewer roles
- **Task Instances** — Detailed task execution logs and status tracking

See `assets/index.html` for the dashboard source (Axum embedded static assets).

## Testing

**Unit Tests (57 tests):**
```bash
cargo test --all
```
Covers: vault encryption, swarm recovery, worker lifecycle, task queue, DB operations, integration workflows.

**UI Tests (131 tests):**
```bash
npm install
npm test
```
Covers: dashboard rendering, API integration, RBAC enforcement, form validation, responsive design, error handling.

See `tests/ui/README.md` for detailed UI test documentation.

## Documentation

Complete documentation is available in the `docs/` directory:

- **[Python Integration](./docs/PHASE_2_PYTHON_INTEGRATION.md)** — Defining DAGs in Python, supported operators, and current limitations
- **[Pillar 3: Secrets Vault](./docs/PILLAR_3_SECRETS_VAULT.md)** — Encrypted secret management, API endpoints, and best practices
- **[Pillar 4: Resilience](./docs/PILLAR_4_RESILIENCE.md)** — High availability, worker health monitoring, and auto-recovery
- **[Architecture Overview](./docs/ARCHITECTURE.md)** — System design, data flow, and component interactions
- **[API Reference](./docs/API_REFERENCE.md)** — Complete REST API documentation with examples
- **[Deployment Guide](./docs/DEPLOYMENT.md)** — Installation, configuration, monitoring, and operations

## Getting Started (Developer Preview)

Currently in active development.

### Build Requirements

- **Python:** 3.13.x or 3.14+ (Python 3.14 requires environment variable workaround below)
- **Rust:** Latest stable (1.70+)

### Building

```bash
cargo build
cargo run -- hello_world
```

### Python 3.14+ Builds (Important!)

Due to PyO3 v0.23 compatibility with Python 3.14, builds require an environment variable:

```bash
# Option 1: Set inline (one-time)
PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo build

# Option 2: Use .env file (persisted)
# The .env file in this repo already contains this flag
# If loading .env automatically, ensure your shell/IDE supports it
source .env
cargo build

# Option 3: Export globally (for your session)
export PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1
cargo build
```

**Long-term fix:** Upgrade to PyO3 v0.24+ when available, which natively supports Python 3.14.

## Why VORTEX?

Because your data pipelines shouldn't spend more time scheduling tasks than executing them.
