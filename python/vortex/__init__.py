# Global list to store defined DAGs
_DAG_REGISTRY = []

class Dag:
    def __init__(self, id, schedule_interval=None, timezone='UTC', max_active_runs=1, catchup=False, sla_seconds=None):
        self.dag_id = id
        self.schedule_interval = schedule_interval
        self.timezone = timezone
        self.max_active_runs = max_active_runs
        self.catchup = catchup
        self.sla_seconds = sla_seconds
        self.tasks = []
        self.dependencies = []
        
        # Detect if this DAG is generated dynamically (contains loops or comprehensions)
        self.is_dynamic = False
        try:
            import inspect
            import ast
            frame = inspect.currentframe().f_back
            filename = frame.f_code.co_filename
            if filename and getattr(filename, 'endswith', lambda x: False)('.py'):
                with open(filename, 'r') as f:
                    tree = ast.parse(f.read())
                    for node in ast.walk(tree):
                        if isinstance(node, (ast.For, ast.While, ast.ListComp, ast.DictComp, ast.SetComp, ast.GeneratorExp)):
                            self.is_dynamic = True
                            break
        except Exception:
            pass

        _DAG_REGISTRY.append(self)

    def add_task(self, task):
        self.tasks.append(task)

    def add_dependency(self, upstream_id, downstream_id):
        self.dependencies.append((upstream_id, downstream_id))

    def to_dict(self):
        return {
            "dag_id": self.dag_id,
            "schedule_interval": self.schedule_interval,
            "timezone": self.timezone,
            "max_active_runs": self.max_active_runs,
            "catchup": self.catchup,
            "is_dynamic": self.is_dynamic,
            "sla_seconds": self.sla_seconds,
            "tasks": [t.to_dict() for t in self.tasks],
            "dependencies": self.dependencies
        }

class Task:
    def __init__(self, id, name=None, command="", task_type="bash", config=None):
        self.id = id
        self.task_id = id
        self.name = name or id
        self.command = command
        self.task_type = task_type
        self.config = config or {}

    def to_dict(self):
        d = {
            "task_id": self.task_id,
            "name": self.name,
            "command": self.command,
            "task_type": self.task_type,
            "config": self.config
        }
        if self.task_type == "bash":
            d["bash_command"] = self.command
        elif self.task_type == "python":
            d["python_callable"] = self.command
        return d

def get_dags():
    return [dag.to_dict() for dag in _DAG_REGISTRY]

# Airflow Compatibility Shim (Imported at bottom to avoid circularity)
from .airflow_shim import (
    DAG,
    BaseOperator,
    BashOperator,
    PythonOperator,
    DummyOperator,
    EmptyOperator,
)
from .task_group import TaskGroup
