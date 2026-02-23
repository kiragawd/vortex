# Python Integration in VORTEX

VORTEX now supports defining DAGs using Python, similar to Apache Airflow. This allows users to leverage Python's flexibility while benefiting from VORTEX's high-performance Rust core.

## Overview

Python integration in VORTEX is achieved through two complementary approaches:
1.  **Regex-based Parsing:** A fast, lightweight parser that extracts DAG structure from Python files without requiring a full Python interpreter for basic validation and UI visualization.
2.  **PyO3-based Runtime:** A robust integration that uses the Python interpreter to execute DAG files, supporting advanced features and dynamic task generation.

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

### Migration Example

#### Before (Airflow)
```python
from airflow import DAG
from airflow.operators.bash import BashOperator
from datetime import datetime

with DAG("my_airflow_dag", start_date=datetime(2023, 1, 1)) as dag:
    t1 = BashOperator(task_id="print_date", bash_command="date")
```

#### After (VORTEX)
No code changes required! VORTEX can parse the above file directly if the `vortex` package is available in the environment or by simply changing the import to `from vortex import DAG`.

### Context Manager Syntax

The `with DAG(...) as dag:` pattern is fully supported. Tasks created within the context manager (or explicitly passed `dag=dag`) will be correctly associated with the DAG.

```python
with DAG(dag_id="my_dag", schedule_interval="@daily") as dag:
    task1 = DummyOperator(task_id="task1")
    task2 = DummyOperator(task_id="task2")
    task1 >> task2
```

### Limitations

- **Complex Metadata:** Metadata fields like `access_control`, `doc_md`, or complex `sla_miss_callback` are ignored by the shim.
- **Advanced Operators:** Operators not listed above (e.g., `KubernetesPodOperator`, `EmailOperator`) are not currently shimmed.
- **Provider Packages:** Imports from `airflow.providers.*` are not supported by the shim.

## Current Limitations vs Full Airflow

While VORTEX provides a familiar interface, there are currently some limitations:
- **XComs:** Cross-task communication (XCom) is not yet implemented.
- **Complex Schedules:** Only standard cron and simple presets are supported; complex `Dataset` or `Timetable` schedules are not.
- **Dynamic Task Mapping:** `expand()` and `partial()` syntax for dynamic tasks is not yet supported.
- **Rich Operator Library:** VORTEX currently focuses on core operators; cloud-specific operators (S3, BigQuery, etc.) are in development.
- **Execution Context:** The full Airflow `context` (e.g., `ds`, `task_instance`) is not yet passed to `PythonOperator` callables.

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

### Execution Flow

```text
  Scheduler             Worker               TaskExecutor          OS
      |                    |                      |                |
      |--- (Assignment) -->|                      |                |
      |                    |--- (Exec Bash) ----->|                |
      |                    |                      |--- (Spawn) --->|
      |                    |                      |<-- (Result) ---|
      |                    |--- (Exec Python) --->|                |
      |                    |                      |--- (Write) --->|
      |                    |                      |--- (Spawn) --->|
      |                    |                      |<-- (Result) ---|
      |                    |                      |--- (Delete) ---|
      |                    |<-- (Result Struct) --|                |
      |--- (Ack/Result) ---|                      |                |
      |                    |                      |                |
```
