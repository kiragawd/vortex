# VORTEX CLI Reference

The `vortex-cli` binary allows you to manage VORTEX components from the command line. It communicates directly with the VORTEX server REST API.

## Global Configuration

Use environment variables to configure the CLI:
- `VORTEX_SERVER_URL` — Base URL of the VORTEX server (default: `http://localhost:3000`)
- `VORTEX_API_KEY` — API key for authentication (sent as `Bearer <API_KEY>` in the Authorization header)

## Commands

### Manage DAGs
```bash
vortex-cli dags <action>
```

#### DAG Actions
- **`list`**
  ```bash
  vortex-cli dags list
  ```
  Lists all DAGs registered in the system. Returns paginated results.

- **`trigger <id>`**
  ```bash
  vortex-cli dags trigger my_pipeline
  ```
  Triggers a manual run of the specified DAG.

- **`pause <id>`**
  ```bash
  vortex-cli dags pause my_pipeline
  ```
  Pauses the specified DAG (calls `PATCH /api/dags/:id/pause`).

- **`unpause <id>`**
  ```bash
  vortex-cli dags unpause my_pipeline
  ```
  Unpauses the specified DAG (calls `PATCH /api/dags/:id/unpause`).

- **`backfill <id> --start <date> --end <date> [--parallel N]`**
  ```bash
  vortex-cli dags backfill my_pipeline --start 2026-01-01T00:00:00Z --end 2026-02-01T00:00:00Z --parallel 4
  ```
  Triggers a backfill run for the specified date range.

### Manage Tasks
```bash
vortex-cli tasks logs <instance_id>
```
Fetches the logs for a specific task instance.

### Manage Secrets
```bash
vortex-cli secrets <action>
```

#### Secrets Actions
- **`set <key> <value>`**
  ```bash
  vortex-cli secrets set DB_PASSWORD mysecretpassword
  ```
  Stores a new encrypted secret in the Vault.

### Manage Users
```bash
vortex-cli users <action>
```

#### Users Actions
- **`create <user> --role <role> --password <password>`**
  ```bash
  vortex-cli users create operator1 --role Operator --password supersecret
  ```
  Creates a new user. Default role is `Operator` and default password is `changeme`. Valid roles are `Admin`, `Operator`, `Viewer`.

## API Endpoints Used

| CLI Command | HTTP Method | API Endpoint |
|-------------|-------------|-------------|
| `dags list` | `GET` | `/api/dags` |
| `dags trigger <id>` | `POST` | `/api/dags/:id/trigger` |
| `dags pause <id>` | `PATCH` | `/api/dags/:id/pause` |
| `dags unpause <id>` | `PATCH` | `/api/dags/:id/unpause` |
| `dags backfill <id>` | `POST` | `/api/dags/:id/backfill` |
| `tasks logs <id>` | `GET` | `/api/tasks/:id/logs` |
| `secrets set <k> <v>` | `POST` | `/api/secrets` |
| `users create <u>` | `POST` | `/api/users` |
