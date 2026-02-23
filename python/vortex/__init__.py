import json

# Global list to store defined DAGs
_DAG_REGISTRY = []

class DAG:
    def __init__(self, dag_id, schedule_interval=None, start_date=None,
                 timezone='UTC', max_active_runs=1, catchup=False):
        self.dag_id = dag_id
        self.schedule_interval = schedule_interval
        self.timezone = timezone
        self.max_active_runs = max_active_runs
        self.catchup = catchup
        self.tasks = []
        self.dependencies = []
        _DAG_REGISTRY.append(self)

    def __enter__(self):
        from . import context
        context._CURRENT_DAG = self
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        from . import context
        context._CURRENT_DAG = None

    def add_task(self, task):
        self.tasks.append(task)

    def add_dependency(self, upstream, downstream):
        self.dependencies.append((upstream.task_id, downstream.task_id))

    def to_dict(self):
        return {
            "dag_id": self.dag_id,
            "schedule_interval": self.schedule_interval,
            "timezone": self.timezone,
            "max_active_runs": self.max_active_runs,
            "catchup": self.catchup,
            "tasks": [t.to_dict() for t in self.tasks],
            "dependencies": self.dependencies
        }

class BaseOperator:
    def __init__(self, task_id, dag=None):
        self.task_id = task_id
        from . import context
        self.dag = dag or context._CURRENT_DAG
        if self.dag:
            self.dag.add_task(self)

    def __rshift__(self, other):
        if self.dag:
            self.dag.add_dependency(self, other)
        return other

    def to_dict(self):
        return {
            "task_id": self.task_id,
            "type": self.__class__.__name__
        }

def get_dags():
    return [dag.to_dict() for dag in _DAG_REGISTRY]

# ─── Airflow Compatibility Shim ───────────────────────────────────────────────
# Re-export the full Airflow-compatible shim classes so users can do:
#   from vortex import DAG, BashOperator, PythonOperator, DummyOperator, EmptyOperator
#
# The shim's DAG is a separate, richer class that does NOT touch _DAG_REGISTRY
# (it is used purely for static parsing / Airflow migration). The native DAG
# class above is kept for PyO3 runtime execution.

from .airflow_shim import (
    DAG,
    BaseOperator,
    BashOperator,
    PythonOperator,
    DummyOperator,
    EmptyOperator,
)

__all__ = [
    "DAG",
    "BaseOperator",
    "BashOperator",
    "PythonOperator",
    "DummyOperator",
    "EmptyOperator",
    "get_dags",
]
