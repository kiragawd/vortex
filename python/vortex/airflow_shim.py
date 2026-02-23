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
        self.dag_id = dag_id
        self.description = description
        self.schedule_interval = schedule_interval
        self.start_date = start_date
        self.catchup = catchup
        self.tags = tags or []
        self.owner = owner
        self.tasks = []
        self._task_dict = {}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        pass

    def __repr__(self):
        return f"<DAG: {self.dag_id}>"


class BaseOperator:
    """Airflow-compatible base operator with dependency-chain support."""

    def __init__(self, task_id, dag=None, **kwargs):
        self.task_id = task_id
        self.dag = dag
        self._upstream = []
        self._downstream = []
        if dag is not None:
            dag.tasks.append(self)
            dag._task_dict[task_id] = self

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
