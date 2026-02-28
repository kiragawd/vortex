# VORTEX CLI Reference

The `vortex-cli` allows you to manage VORTEX components natively from the command line. It communicates directly with the VORTEX server APIs.

## Global Configuration
Use environment variables to configure the CLI behavior:
- `VORTEX_SERVER_URL` - Base URL of the VORTEX server (default: `http://localhost:3000`)
- `VORTEX_API_KEY` - API key for authentication (creates `Bearer <API_KEY>` authorization header)

## Commands

### Manage DAGs
```bash
vortex-cli dags <action>
```

#### Dags Actions
- **`list`**
  ```bash
  vortex-cli dags list
  ```
  Lists all DAGs registered in the system.

- **`trigger <id>`**
  ```bash
  vortex-cli dags trigger my_pipeline
  ```
  Triggers a manual run of the specified DAG.

- **`pause/unpause <id>`**
  ```bash
  vortex-cli dags pause my_pipeline
  ```

- **`backfill <id> --start <date> --end <date> [--parallel N]`**
  ```bash
  vortex-cli dags backfill my_pipeline --start 2026-01-01 --end 2026-02-01 --parallel 4
  ```

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
  Sets a new securely encrypted secret in the Vault.

### Manage Users
```bash
vortex-cli users <action>
```

#### Users Actions
- **`create <user> --role <role> --password <password>`**
  ```bash
  vortex-cli users create admin --role Admin --password supersecret
  ```
  Creates a new user. Default role is `Operator` and default password is `changeme`. Valid roles are `Admin`, `Operator`, `Viewer`.
