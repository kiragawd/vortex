# python/vortex/airflow_shim.py
#
# Airflow Compatibility Shim for VORTEX
# ──────────────────────────────────────
# Mimics the Airflow API surface so that existing Airflow DAG files can be
# imported and parsed by VORTEX with zero or minimal code changes.
#
# Supported:
#   - DAG  (context-manager, metadata, task registry)
#   - BaseOperator  (>>, << dependency syntax, set_upstream/set_downstream)
#   - BashOperator, PythonOperator, DummyOperator, EmptyOperator


class DAG:
    """Airflow-compatible DAG class for VORTEX."""

    def __init__(
        self,
        dag_id,
        description=None,
        schedule_interval=None,
        start_date=None,
        catchup=False,
        tags=None,
        owner=None,
        **kwargs,
    ):
        from vortex import _DAG_REGISTRY
        self.dag_id = dag_id
        self.description = description
        self.schedule_interval = schedule_interval
        self.start_date = start_date
        self.catchup = catchup
        self.tags = tags or []
        self.owner = owner
        self.tasks = []
        self._task_dict = {}
        _DAG_REGISTRY.append(self)

    def __enter__(self):
        from . import context
        context._CURRENT_DAG = self
        return self

    def __exit__(self, *args):
        from . import context
        context._CURRENT_DAG = None

    def __repr__(self):
        return f"<DAG: {self.dag_id}>"

    def to_dict(self):
        """Export DAG structure to a dictionary for VORTEX parser."""
        return {
            "dag_id": self.dag_id,
            "description": self.description,
            "schedule_interval": self.schedule_interval,
            "catchup": self.catchup,
            "tasks": [t.to_dict() for t in self.tasks],
            "dependencies": self.get_dependencies(),
        }

    def get_dependencies(self):
        """Extract edges from tasks."""
        deps = []
        for task in self.tasks:
            for downstream_id in task._downstream:
                deps.append((task.task_id, downstream_id))
        return deps


class BaseOperator:
    """Airflow-compatible base operator with dependency-chain support."""

    def __init__(self, task_id, dag=None, pool="default", execution_timeout=None, **kwargs):
        self.task_id = task_id
        self.pool = pool
        self.execution_timeout = execution_timeout
        from . import context
        self.dag = dag or context._CURRENT_DAG
        self.task_group = context._CURRENT_TASK_GROUP
        self._upstream = []
        self._downstream = []
        if self.dag is not None:
            self.dag.tasks.append(self)
            self.dag._task_dict[task_id] = self

    # ── Dependency syntax ──────────────────────────────────────────────────

    def __rshift__(self, other):
        """Support  task1 >> task2  syntax (task1 is upstream of task2)."""
        if isinstance(other, BaseOperator):
            self._downstream.append(other.task_id)
            other._upstream.append(self.task_id)
        return other

    def __lshift__(self, other):
        """Support  task1 << task2  syntax (task1 is downstream of task2)."""
        other >> self
        return other

    def set_upstream(self, task):
        """Explicit: make *task* upstream of self."""
        task >> self

    def set_downstream(self, task):
        """Explicit: make *task* downstream of self."""
        self >> task

    def __repr__(self):
        return f"<{self.__class__.__name__}: {self.task_id}>"

    def to_dict(self):
        """Export Task structure to a dictionary for VORTEX parser."""
        d = {
            "task_id": self.task_id,
            "type": self.__class__.__name__,
            "upstream_task_ids": self._upstream,
            "downstream_task_ids": self._downstream,
            "pool": self.pool,
        }
        if self.task_group:
            d["task_group"] = self.task_group.group_id
        if self.execution_timeout:
            if hasattr(self.execution_timeout, "total_seconds"):
                d["execution_timeout"] = int(self.execution_timeout.total_seconds())
            else:
                d["execution_timeout"] = int(self.execution_timeout)
        if hasattr(self, "bash_command"):
            d["bash_command"] = self.bash_command
        if hasattr(self, "python_callable"):
            # Handle both function objects and strings
            if hasattr(self.python_callable, "__name__"):
                d["python_callable"] = self.python_callable.__name__
            else:
                d["python_callable"] = str(self.python_callable)
        return d


class BashOperator(BaseOperator):
    """Runs a shell command."""

    def __init__(self, task_id, bash_command, dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.bash_command = bash_command


class PythonOperator(BaseOperator):
    """Calls a Python callable."""

    def __init__(
        self,
        task_id,
        python_callable,
        op_args=None,
        op_kwargs=None,
        dag=None,
        **kwargs,
    ):
        super().__init__(task_id=task_id, dag=dag, **kwargs)
        self.python_callable = python_callable
        self.op_args = op_args or []
        self.op_kwargs = op_kwargs or {}


class DummyOperator(BaseOperator):
    """No-op placeholder operator (matches Airflow's DummyOperator)."""

    def __init__(self, task_id, dag=None, **kwargs):
        super().__init__(task_id=task_id, dag=dag, **kwargs)


class EmptyOperator(DummyOperator):
    """Alias for DummyOperator — matches Airflow 2.4+ rename."""

    pass
