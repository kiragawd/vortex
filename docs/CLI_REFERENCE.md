# RYUO CLI Reference

The `ryuo-cli` binary allows you to manage RYUO components from the command line. It communicates directly with the RYUO server REST API.

## Global Configuration

Use environment variables to configure the CLI:
- `RYUO_BASE_URL` — Base URL of the RYUO server (default: `http://localhost:3000`)
- `RYUO_SERVER_URL` — Alias for `RYUO_BASE_URL` (deprecated)
- `RYUO_API_KEY` — API key for authentication (sent as `Bearer <API_KEY>` in the Authorization header)

## Commands

### Manage DAGs
```bash
ryuo-cli dags <action>
```

#### DAG Actions
- **`list`**
  ```bash
  ryuo-cli dags list
  ```
  Lists all DAGs registered in the system. Returns paginated results.

- **`trigger <id>`**
  ```bash
  ryuo-cli dags trigger my_pipeline
  ```
  Triggers a manual run of the specified DAG.

- **`pause <id>`**
  ```bash
  ryuo-cli dags pause my_pipeline
  ```
  Pauses the specified DAG (calls `PATCH /api/dags/:id/pause`).

- **`unpause <id>`**
  ```bash
  ryuo-cli dags unpause my_pipeline
  ```
  Unpauses the specified DAG (calls `PATCH /api/dags/:id/unpause`).

- **`backfill <id> --start-date <date> --end-date <date> [--parallel] [--dry-run]`**
  ```bash
  ryuo-cli dags backfill my_pipeline --start-date 2026-01-01 --end-date 2026-02-01 --parallel
  ryuo-cli dags backfill my_pipeline --start-date 2026-01-01 --end-date 2026-02-01 --dry-run
  ```
  Triggers a backfill run for the specified date range.

### Migrate Airflow DAGs
```bash
ryuo-cli migrate <path-to-airflow-dags> [options]
```

Generates Rust DAG modules from Airflow DAG Python files or dbt projects.

#### Options

| Flag | Description | Default |
|------|-------------|---------|
| `--output-dir <dir>` | Directory for generated Rust modules and reports | `./generated_dags` |
| `--strict` | Fail if any unresolved placeholders remain | `false` |
| `--report-format <fmt>` | Report format: `json` or `md` | `json` |
| `--use-shim-fallback` | Preserve original Python callable payloads for runtime shim fallback | `false` |
| `--agentic` | Enable LLM-assisted conversion for unresolved tasks | `false` |
| `--llm-provider <provider>` | LLM provider: `openai` or `anthropic` (requires `--agentic`) | — |
| `--model <model>` | LLM model name (requires `--agentic`) | — |

#### Standard migration
```bash
ryuo-cli migrate ./dags --output-dir ./generated_dags
ryuo-cli migrate ./dags --output-dir ./generated_dags --strict
ryuo-cli migrate ./dags --output-dir ./generated_dags --report-format md
ryuo-cli migrate ./dags --output-dir ./generated_dags --use-shim-fallback
```

#### Agentic conversion (AI-assisted)
```bash
ryuo-cli migrate ./dags --agentic --llm-provider openai --model gpt-4o-mini
ryuo-cli migrate ./dags --agentic --llm-provider anthropic --model claude-3-5-sonnet-latest
```

Required environment variables for agentic mode:
- **OpenAI:** `OPENAI_API_KEY` (optional: `OPENAI_ENDPOINT`)
- **Anthropic:** `ANTHROPIC_API_KEY` (optional: `ANTHROPIC_ENDPOINT`)

#### dbt project conversion
```bash
ryuo-cli migrate ./dbt_project --output-dir ./generated_dags --agentic --llm-provider openai --model gpt-4o-mini
```

When migration runs, RYUO performs:
- Generated Rust syntax validation.
- DAG graph equivalence validation (dependency parity).
- Strict failure if unresolved placeholders remain when `--strict` is enabled.
- For agentic mode: iterative compile-check and lint validation of LLM-generated code.

### Manage Tasks
```bash
ryuo-cli tasks logs <instance_id>
```
Fetches the logs for a specific task instance.

### Manage Secrets
```bash
ryuo-cli secrets <action>
```

#### Secrets Actions
- **`set <key> <value>`**
  ```bash
  ryuo-cli secrets set DB_PASSWORD mysecretpassword
  ```
  Stores a new encrypted secret in the Vault.

### Manage Users
```bash
ryuo-cli users <action>
```

#### Users Actions
- **`create <user> --role <role> --password <password>`**
  ```bash
  ryuo-cli users create operator1 --role Operator --password supersecret
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

## Notes
- `migrate` runs locally and writes generated files/report to disk.
- `migrate` does not call RYUO REST API endpoints.
- `migrate --agentic` requires a valid LLM provider API key set via environment variable.
- `migrate` with dbt projects parses the dbt manifest, expands Jinja SQL, and generates Rust pipeline modules.
