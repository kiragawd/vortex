from vortex import BaseOperator

class BashOperator(BaseOperator):
    def __init__(self, task_id, bash_command, **kwargs):
        super().__init__(task_id, **kwargs)
        self.bash_command = bash_command

    def to_dict(self):
        d = super().to_dict()
        d["bash_command"] = self.bash_command
        return d
