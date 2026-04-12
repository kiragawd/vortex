# Python Integration in VORTEX

VORTEX supports defining DAGs using Python, similar to Apache Airflow. This allows users to leverage Python's flexibility while benefiting from VORTEX's high-performance Rust core.

## Overview

Python integration in VORTEX is achieved through two complementary approaches:
1.  **Regex-based Parsing:** A fast, lightweight parser that extracts DAG structure from Python files without requiring a full Python interpreter for basic validation and UI visualization.
2.  **PyO3-based Runtime:** A robust integration that uses the Python interpreter to execute DAG files, supporting advanced features and dynamic task generation.

## DAG Upload & Management

VORTEX provides a secure API and user-friendly Web UI for uploading and versioning your Python DAG files.

### Web UI Workflow
1. Click the **"📤 Upload DAG"** button in the top navigation bar.
2. Select or drag-and-drop a `.py` file.
3. VORTEX automatically validates the file structure (checking for imports, `dag_id`, and cyclic dependencies).
4. On success, a preview of the parsed metadata (tasks, schedule) is shown.
5. The DAG is immediately registered and becomes visible in the registry.

### REST API Upload
You can upload DAGs programmatically using the `/api/dags/upload` endpoint.

```bash
# Upload a DAG file using curl
curl -X POST http://localhost:3000/api/dags/upload \
  -H "Authorization: Bearer <api_key>" \
  -F "file=@my_dag.py"
```

### DAG Versioning
Every time a DAG file is uploaded, VORTEX creates a new version in the `dag_versions` table.
- **Incremental Versioning:** Each upload for the same `dag_id` increments the version number.
- **Storage:** Files are stored in the `dags/` directory with their original names (overwriting the active file but tracked in the DB version history).
- **Metadata Tracking:** VORTEX tracks the creator, upload time, and file path for every version.
- **Rollback:** Use the API or Dashboard to rollback to any previous version.

## Supported Operators

VORTEX currently supports the following core operators:

### `BashOperator`
Executes a bash command or script.
- **Parameters:**
    - `task_id`: Unique identifier for the task.
    - `bash_command`: The command to be executed.

### `PythonOperator`
Executes a Python function.
- **Parameters:**
    - `task_id`: Unique identifier for the task.
    - `python_callable`: The Python function to call.

### `DummyOperator`
A no-op task that can be used for grouping or as a placeholder in the DAG structure.
- **Parameters:**
    - `task_id`: Unique identifier for the task.

## DAG Metadata Fields

When defining a DAG, the following fields are supported:

- `dag_id`: (Required) A unique identifier for the DAG.
- `schedule_interval`: A cron expression or preset (e.g., `@daily`, `@hourly`) defining when the DAG should run.
- `owner`: The owner/creator of the DAG.
- `description`: A short description of the DAG's purpose.
- `tags`: A list of tags for categorization.

## Task Relationship Syntax

VORTEX supports the standard Airflow bitshift operators and methods for defining task dependencies:

- **Bitshift Operators:**
    ```python
    t1 >> t2  # t1 is upstream of t2
    t1 << t2  # t1 is downstream of t2
    t1 >> t2 >> t3  # Chain dependencies
    ```
- **Explicit Methods:**
    - `t1.set_downstream(t2)`
    - `t2.set_upstream(t1)`

## Example Python DAG

Here is a complete example showing the supported features:

```python
from vortex import DAG
from vortex.operators.bash import BashOperator
from vortex.operators.python import PythonOperator
from vortex.operators.dummy import DummyOperator
from datetime import datetime

def my_python_logic():
    print("Executing custom logic!")

with DAG(
    dag_id="example_vortex_dag",
    schedule_interval="0 12 * * *",
    owner="vortex_team",
    description="An example DAG showcasing VORTEX features",
    tags=["example", "python"]
) as dag:

    start = DummyOperator(task_id="start")

    run_script = BashOperator(
        task_id="run_script",
        bash_command="echo 'Hello from VORTEX!'"
    )

    process_data = PythonOperator(
        task_id="process_data",
        python_callable=my_python_logic
    )

    end = DummyOperator(task_id="end")

    # Defining dependencies
    start >> [run_script, process_data] >> end
```

## Airflow Compatibility

VORTEX provides an Airflow-compatible shim that allows many existing Airflow DAGs to run on VORTEX with zero or minimal modifications. This is particularly useful for migrating from Airflow to VORTEX or for teams that prefer the familiar Airflow API.

### Import Syntax

You can use either VORTEX-native imports or standard Airflow-style imports. The VORTEX parser recognizes all of these:

```python
# Standard Airflow imports
from airflow import DAG
from airflow.operators.bash import BashOperator
from airflow.operators.python import PythonOperator
from airflow.operators.dummy import DummyOperator
# or
from airflow.models import DAG

# VORTEX-style Airflow shim
from vortex import DAG, BashOperator, PythonOperator, DummyOperator, EmptyOperator
```

### Supported Operators List

The shim provides the following classes that mimic the Airflow 2.x API:

- `DAG`
- `BaseOperator` (provides `>>`, `<<`, `set_upstream`, `set_downstream`)
- `BashOperator`
- `PythonOperator`
- `DummyOperator`
- `EmptyOperator` (alias for `DummyOperator`)

### Context Manager Syntax

The `with DAG(...) as dag:` pattern is fully supported. Tasks created within the context manager (or explicitly passed `dag=dag`) will be correctly associated with the DAG.

```python
with DAG(dag_id="my_dag", schedule_interval="@daily") as dag:
    task1 = DummyOperator(task_id="task1")
    task2 = DummyOperator(task_id="task2")
    task1 >> task2
```

## Task Execution

VORTEX workers handle the execution of both Bash and Python tasks using an isolated `TaskExecutor`.

### BashOperator Execution
When a `BashOperator` task is received, the worker spawns a subprocess:
- **Command:** `sh -c "{bash_command}"`
- **Isolation:** Each command runs in its own process.
- **Secrets:** All associated secrets are injected as environment variables.
- **Timeout:** Tasks are automatically timed out after 300 seconds (default, configurable per-task).
- **Result:** Captures stdout, stderr, exit code, and execution duration.

### PythonOperator Execution
When a `PythonOperator` task is received:
- **Preparation:** The worker writes the Python code to a temporary file.
- **Command:** `python3 /tmp/vortex_task_{task_id}.py`
- **Secrets:** Secrets are injected via environment variables and accessible through `os.environ`.
- **Cleanup:** The temporary file is automatically removed after execution.
- **Result:** Captures all print statements (stdout), exceptions (stderr), and duration.

### Secret Injection

Secrets are securely fetched from the VORTEX vault and injected as environment variables at execution time. Additionally, the following helper variables are injected:

| Variable | Description |
|----------|-------------|
| `VORTEX_BASE_URL` | Base URL of the VORTEX server (default: `http://localhost:3000`) |
| `VORTEX_API_KEY` | Task-scoped API key (only if `VORTEX_TASK_API_KEY` is configured on the server) |

> **Security:** Tasks do NOT receive the admin API key. See [Secrets Vault](./SECRETS_VAULT.md) for details.

## Retry Configuration

Tasks can be configured to retry automatically on failure.

### Example Retry Config
```python
task = BashOperator(
    task_id="flaky_task",
    bash_command="curl https://api.example.com/data",
    max_retries=3,
    retry_delay_secs=60
)
```
-   **max_retries:** Number of retry attempts (default: 0).
-   **retry_delay_secs:** Delay between retries in seconds (default: 30).
-   **Retry Tracking:** The `retry_count` is tracked in the database and visible in the UI logs.

## Monitoring

The VORTEX Dashboard provides real-time monitoring of task execution:
-   **Live Logs:** Click "View Logs" on any task instance to see stdout and stderr.
-   **Status Badges:** Color-coded badges indicate task state:
    -   ✅ **Success** (Green)
    -   ❌ **Failed** (Red)
    -   🔄 **Running** (Blue)
    -   ⏳ **Queued** (Gray)
-   **Execution Duration:** Precise duration tracking (e.g., "2.3s") for performance analysis.
-   **Auto-Refresh:** The DAG detail view automatically refreshes to show the latest task states.

## Testing

### Running Integration Tests

Ensure the VORTEX server is running (defaulting to `http://localhost:3000`):

```bash
# In one terminal, start VORTEX
cargo run -- server --database-url "postgres://..."

# In another terminal, run the integration test
python3 tests/integration_full.py
```

### Test Coverage

The integration suite (`tests/integration_full.py`) covers:
1.  **Full Pipeline:** DAG upload → Registry validation → Trigger → Multi-task dependency execution → Log capture
2.  **Secret Injection:** Verifying secrets are accessible as environment variables in tasks
3.  **Error Handling & Retries:** Verifying failing tasks are retried correctly

---

## Related Documentation

- [API Reference](./API_REFERENCE.md) — Complete endpoint documentation
- [Secrets Vault](./SECRETS_VAULT.md) — Encrypted secret management
- [Architecture Overview](./ARCHITECTURE.md) — System design and data flow
- [Deployment Guide](./DEPLOYMENT.md) — Setup and configuration

---

## Python SDK API Reference

The `vortex` Python package provides the following modules for DAG authoring and runtime integration:

### `vortex.dag`

DAG definition classes.

```python
from vortex import DAG

# Create a DAG context manager
with DAG(
    dag_id="my_pipeline",
    schedule_interval="@daily",
    owner="data_team",
    description="My pipeline description",
    tags=["production", "etl"],
    catchup=False,
    max_active_runs=1,
) as dag:
    pass
```

| Class/Function | Description |
|---------------|-------------|
| `DAG(dag_id, schedule_interval, ...)` | Root workflow definition. Supports context manager syntax. |
| `TaskGroup(group_id)` | Logical grouping of tasks for visual nesting. |

### `vortex.task`

Task operator classes for defining units of work.

```python
from vortex.operators.bash import BashOperator
from vortex.operators.python import PythonOperator
from vortex.operators.dummy import DummyOperator, EmptyOperator

task = BashOperator(task_id="hello", bash_command="echo hello", max_retries=3, retry_delay_secs=30)
```

| Class | Description |
|-------|-------------|
| `BashOperator(task_id, bash_command)` | Executes a shell command via `sh -c`. |
| `PythonOperator(task_id, python_callable)` | Executes a Python callable. |
| `DummyOperator(task_id)` | No-op placeholder task for DAG structure. |
| `EmptyOperator(task_id)` | Alias for `DummyOperator`. |

### `vortex.xcom`

Cross-task communication — push and pull values between tasks in the same DAG run.

```python
from vortex.xcom import xcom_push, xcom_pull

# In a task: push a value
xcom_push(dag_id="my_pipeline", task_id="extract", run_id=run_id, key="row_count", value="42")

# In a downstream task: pull the value
row_count = xcom_pull(dag_id="my_pipeline", task_id="extract", run_id=run_id, key="row_count")
```

| Function | Description |
|----------|-------------|
| `xcom_push(dag_id, task_id, run_id, key, value)` | Store a string value in the XCom store. |
| `xcom_pull(dag_id, task_id, run_id, key)` | Retrieve a stored XCom value. Returns `None` if not found. |

The Vortex server base URL can be overridden with the `VORTEX_BASE_URL` environment variable (default: `http://localhost:3000`). A task-scoped API key is available via `VORTEX_API_KEY` if `VORTEX_TASK_API_KEY` is configured on the server.

### `vortex.secrets`

Secret retrieval from the Vortex vault. Secrets are injected as environment variables at task start — direct vault access from Python is generally not needed.

```python
import os

# Secrets are pre-injected as environment variables before task execution
db_password = os.environ["MY_DB_PASSWORD"]    # set from Secrets Vault key "MY_DB_PASSWORD"
api_key     = os.environ["THIRD_PARTY_TOKEN"] # set from Secrets Vault key "THIRD_PARTY_TOKEN"
```

| Mechanism | Description |
|-----------|-------------|
| Environment variable injection | The recommended approach. All vault secrets assigned to the DAG are automatically decrypted and injected as env vars before task execution. Tasks do not need direct vault API access. |
| `VORTEX_API_KEY` | Task-scoped API key (available if `VORTEX_TASK_API_KEY` is set on the server). |

### `vortex.notifications`

Alert and notification hooks for sending messages on task events.

```python
from vortex.notifications import notify_failure

# Call in a PythonOperator on exception:
def my_task():
    try:
        run_pipeline()
    except Exception as e:
        notify_failure(dag_id="my_pipeline", task_id="my_task", error=str(e))
        raise
```

Notification channels are configured per-DAG via the `POST /api/dags/:id/callbacks` endpoint (Webhook, Slack, Email). See the [API Reference](./API_REFERENCE.md) for configuration details.
