# Web Dashboard

## Overview

Ryuo includes an enterprise single-page application embedded directly into the binary via `rust-embed`. No separate web server or Node.js runtime is required in production.

**Modules:** `ui/`, `assets/`

---

## Technology Stack

| Component | Technology | Version |
|-----------|-----------|---------|
| **Framework** | React | 18.3 |
| **Language** | TypeScript | 5.3 |
| **Build Tool** | Vite | 5.1 |
| **CSS** | Tailwind CSS | 3.4 |
| **Global State** | Zustand | 4.5 |
| **Server State** | TanStack React Query | 5.20 |
| **Routing** | React Router | 6.22 |
| **Charts** | Recharts | 2.12 |
| **Icons** | Lucide React | 0.344 |
| **Utilities** | clsx, date-fns | 2.1 / 3.3 |

---

## Pages

| Page | Description |
|------|-------------|
| **Dashboard** | Overview with DAG stats, recent runs, and system health |
| **DAGs** | List all DAGs with status, schedule, and quick actions |
| **DAG Detail** | Individual DAG view with dependency graph, runs, and code editor |
| **Runs** | DAG run history with status, duration, and trigger details |
| **Compliance** | Compliance controls, audit trail, and approval workflows |
| **RBAC** | Role and permission management, user-role assignments |
| **Monitoring** | Swarm health, worker status, and system metrics |
| **Settings** | System configuration, auth providers, and feature flags |
| **Swarm** | Worker pool overview with capacity, heartbeat, and drain controls |
| **Lineage** | Data lineage graph visualization |
| **Connectors** | Enterprise connector status and health checks |
| **Events** | Event bus activity and sensor status |
| **Secrets** | Vault management (Admin only) |
| **Users** | User management (Admin only) |

---

## Features

### Dark/Light Mode

Full theme toggle with `localStorage` persistence. Applies across all pages and components.

### SPA Routing

React Router v6 with server-side fallback — Axum serves `index.html` for all non-API, non-file paths, enabling client-side routing.

### Auto-Refresh

5-second polling for DAG status and Swarm health using TanStack React Query's `refetchInterval`.

### RBAC-Aware UI

- **Admin** — Full visibility: all DAGs, users, secrets, audit logs
- **Operator/Viewer with team** — Sees only their team's DAGs
- **Operator/Viewer without team** — Sees only unassigned DAGs

### Code Editor

In-browser DAG source editing with live re-parse. Changes can be saved directly through the API.

### Run History

Collapsible accordion view with per-run graph snapshots showing task states.

### Temporal Analysis

- **Gantt charts** — Recharts-based execution timeline for identifying bottlenecks
- **Calendar view** — Monthly schedule visualization

### Version Diffing

Side-by-side DAG version comparison with one-click rollback capability.

---

## Development

### Prerequisites

- Node.js 18+
- npm 9+

### Local Development

```bash
cd ui
npm install
npm run dev
```

The Vite dev server runs on `http://localhost:5173` with hot module replacement (HMR) and proxies API requests to the Ryuo backend.

### Production Build

```bash
cd ui
npm run build
```

Build output goes to `assets/` where it's picked up by `rust-embed` during the Rust compilation step.

### Testing

```bash
# Playwright E2E tests
npm install
npm test
```

---

## Embedding

The dashboard is embedded into the Ryuo binary using `rust-embed`:

1. `npm run build` produces optimized static assets in `assets/`
2. `rust-embed` includes these files at Rust compile time
3. Axum serves them from memory at runtime — no filesystem access needed
4. SPA fallback route serves `index.html` for all non-API paths

---

## Related Documentation

- [Architecture](./ARCHITECTURE.md) — System design and component overview
- [API Reference](./API_REFERENCE.md) — REST API endpoints consumed by the dashboard
- [Authentication](./AUTHENTICATION.md) — Login flow and RBAC
- [Deployment](./DEPLOYMENT.md) — Build and run instructions
