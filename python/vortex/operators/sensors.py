"""
Sensor operators for VORTEX — Airflow-compatible poll-based waiting tasks.

These operators serialize to the same JSON schema that the Rust `SensorConfig`
struct expects, so they can be parsed by `python_parser.rs` and dispatched to
`run_sensor_loop` in `sensors.rs`.
"""

from vortex.airflow_shim import BaseOperator


class BaseSensorOperator(BaseOperator):
    """Base class for sensors — polls a condition at intervals.

    Args:
        task_id:        Unique identifier for this task within the DAG.
        poke_interval:  Seconds between condition checks. Default: 30.
        timeout:        Maximum seconds to wait before failing. Default: 3600.
        mode:           "poke"        — worker is held while waiting.
                        "reschedule"  — worker is released between pokes
                                        (requires scheduler support).
        dag:            Parent DAG instance (passed through to BaseOperator).
    """

    def __init__(self, task_id, poke_interval=30, timeout=3600, mode="poke", dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.poke_interval = poke_interval
        self.timeout = timeout
        self.mode = mode  # "poke" or "reschedule"
        self.sensor_type = "base"

    def to_dict(self):
        """Serialize to a dict compatible with VORTEX's SensorConfig JSON schema."""
        d = super().to_dict()
        d["sensor_type"] = self.sensor_type
        d["poke_interval_secs"] = self.poke_interval   # Rust field name
        d["poke_interval"] = self.poke_interval         # Python-facing alias
        d["timeout_secs"] = self.timeout                # Rust field name
        d["timeout"] = self.timeout                     # Python-facing alias
        d["mode"] = self.mode
        return d


class FileSensor(BaseSensorOperator):
    """Wait for a file to appear at a given path.

    Args:
        task_id:   Unique task identifier.
        filepath:  Absolute path to the file that must exist before continuing.
        dag:       Parent DAG instance.
    """

    def __init__(self, task_id, filepath, dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.filepath = filepath
        self.sensor_type = "file"

    def to_dict(self):
        d = super().to_dict()
        d["filepath"] = self.filepath
        # Mirror into the nested `config` dict that SensorConfig.config expects.
        d.setdefault("config", {})["filepath"] = self.filepath
        return d


class HttpSensor(BaseSensorOperator):
    """Wait for an HTTP endpoint to return a specific status code.

    Args:
        task_id:         Unique task identifier.
        endpoint:        Full URL to poll (e.g. "http://api.example.com/health").
        method:          HTTP method. Default: "GET" (only GET is used by the
                         Rust implementation, which uses curl).
        expected_status: HTTP status code that signals success. Default: 200.
        dag:             Parent DAG instance.
    """

    def __init__(self, task_id, endpoint, method="GET", expected_status=200, dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.endpoint = endpoint
        self.method = method
        self.expected_status = expected_status
        self.sensor_type = "http"

    def to_dict(self):
        d = super().to_dict()
        d["endpoint"] = self.endpoint
        d["method"] = self.method
        d["expected_status"] = self.expected_status
        # Mirror into nested config for Rust SensorConfig.config parsing.
        d.setdefault("config", {}).update({
            "endpoint": self.endpoint,
            "method": self.method,
            "expected_status": self.expected_status,
        })
        return d


class ExternalTaskSensor(BaseSensorOperator):
    """Wait for a task in another DAG to reach the 'Success' state.

    Args:
        task_id:          Unique task identifier.
        external_dag_id:  ID of the upstream DAG to watch.
        external_task_id: ID of the task inside that DAG to watch.
        dag:              Parent DAG instance.
    """

    def __init__(self, task_id, external_dag_id, external_task_id, dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.external_dag_id = external_dag_id
        self.external_task_id = external_task_id
        self.sensor_type = "external_task"

    def to_dict(self):
        d = super().to_dict()
        d["external_dag_id"] = self.external_dag_id
        d["external_task_id"] = self.external_task_id
        d.setdefault("config", {}).update({
            "external_dag_id": self.external_dag_id,
            "external_task_id": self.external_task_id,
        })
        return d


class SqlSensor(BaseSensorOperator):
    """Wait for a SQL query to return at least one row.

    Args:
        task_id:  Unique task identifier.
        conn_id:  Connection string / path to the SQLite database file.
        sql:      SQL SELECT statement whose result set must be non-empty.
        dag:      Parent DAG instance.
    """

    def __init__(self, task_id, conn_id, sql, dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.conn_id = conn_id
        self.sql = sql
        self.sensor_type = "sql"

    def to_dict(self):
        d = super().to_dict()
        d["conn_id"] = self.conn_id
        d["sql"] = self.sql
        d.setdefault("config", {}).update({
            "conn_id": self.conn_id,
            "sql": self.sql,
        })
        return d
