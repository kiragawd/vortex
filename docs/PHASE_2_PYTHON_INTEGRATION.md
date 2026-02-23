# Python Integration in VORTEX

VORTEX now supports defining DAGs using Python, similar to Apache Airflow. This allows users to leverage Python's flexibility while benefiting from VORTEX's high-performance Rust core.

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
curl -X POST http://localhost:8080/api/dags/upload \
  -H "Authorization: Bearer vortex_admin_key" \
  -F "file=@my_dag.py"
```

### DAG Versioning
Every time a DAG file is uploaded, VORTEX creates a new version in the `dag_versions` table.
- **Incremental Versioning:** Each upload for the same `dag_id` increments the version number.
- **Storage:** Files are stored in the `dags/` directory with their original names (overwriting the active file but tracked in the DB version history).
- **Metadata Tracking:** VORTEX tracks the creator, upload time, and file path for every version.

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
- **Secrets:** All associated secrets for the task are injected as environment variables.
- **Timeout:** Tasks are automatically timed out after 300 seconds (configurable).
- **Result:** Captures stdout, stderr, exit code, and execution duration.

### PythonOperator Execution
When a `PythonOperator` task is received:
- **Preparation:** The worker writes the Python code to a temporary file.
- **Command:** `python3 /tmp/vortex_task_{task_id}.py`
- **Secrets:** Secrets are injected via environment variables and accessible through `os.environ`.
- **Cleanup:** The temporary file is automatically removed after execution.
- **Result:** Captures all print statements (stdout), exceptions (stderr), and duration.

### Secret Injection
Secrets are securely fetched from the VORTEX vault and injected only at the moment of execution.

```rust
// Example of secret injection in Rust
cmd.envs(env_vars); // Inject secrets as environment variables
```

## End-to-End Execution Flow

1.  **DAG Submission:** A user uploads a `.py` file via the Web UI or API.
2.  **Scheduling:** The VORTEX Scheduler identifies tasks ready to run based on dependencies and triggers.
3.  **Task Queuing:** Ready tasks are enqueued into the Swarm task queue with their type (`bash` or `python`) and configuration.
4.  **Worker Polling:** An active worker polls the Swarm Controller for available tasks.
5.  **Execution:**
    - The worker receives the task, fetches required secrets from the vault (via the controller).
    - It routes the task to the appropriate executor based on type.
    - Secrets are injected as environment variables.
6.  **Result Reporting:** The worker reports stdout, stderr, and exit code back to the Controller.
7.  **DB Update:** the Controller stores the results and updates the `task_instances` table.

## Monitoring

The VORTEX Dashboard provides real-time monitoring of task execution:
-   **Live Logs:** Click "View Logs" on any task instance to see real-time stdout and stderr.
-   **Status Badges:** Color-coded badges indicate task state:
    -   ✅ **Success** (Green)
    -   ❌ **Failed** (Red)
    -   🔄 **Running** (Blue)
    -   ⏳ **Queued** (Gray)
-   **Execution Duration:** Precise duration tracking (e.g., "2.3s") for performance analysis.
-   **Auto-Refresh:** The DAG detail view automatically refreshes every 10 seconds to show the latest task states.

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
