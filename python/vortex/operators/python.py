from vortex import BaseOperator

class PythonOperator(BaseOperator):
    def __init__(self, task_id, python_callable, op_args=None, op_kwargs=None, **kwargs):
        super().__init__(task_id, **kwargs)
        self.python_callable = python_callable.__name__ if hasattr(python_callable, "__name__") else str(python_callable)
        self.op_args = op_args or []
        self.op_kwargs = op_kwargs or {}

    def to_dict(self):
        d = super().to_dict()
        d["python_callable"] = self.python_callable
        d["op_args"] = self.op_args
        d["op_kwargs"] = self.op_kwargs
        return d
