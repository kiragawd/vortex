from vortex import DAG
from vortex.operators.bash import BashOperator

with DAG(dag_id="scheduled_test", schedule_interval="*/10 * * * * *") as dag:
    t1 = BashOperator(task_id="hello", bash_command="echo 'Scheduled hello from Vortex!'")
