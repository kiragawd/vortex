# python/vortex/task_group.py
from . import context

class TaskGroup:
    """A context manager to group tasks visually and logically."""
    def __init__(self, group_id: str):
        self.group_id = group_id
        
    def __enter__(self):
        context._CURRENT_TASK_GROUP = self
        return self
        
    def __exit__(self, exc_type, exc_val, exc_tb):
        context._CURRENT_TASK_GROUP = None
