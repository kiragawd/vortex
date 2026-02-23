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
