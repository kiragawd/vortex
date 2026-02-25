from vortex import DAG
from vortex.operators.bash import BashOperator
from vortex.operators.python import PythonOperator

def my_python_func():
    print("Executing Python function!")

with DAG(dag_id="example_python_dag") as dag:
    t1 = BashOperator(
        task_id="print_date",
        bash_command="date"
    )

    t2 = BashOperator(
        task_id="sleep_task",
        bash_command="sleep 2"
    )

    t3 = PythonOperator(
        task_id="python_task",
        python_callable=my_python_func
    )

    t4 = BashOperator(
        task_id="echo_done",
        bash_command="'DAG finished successfully'"
    )

    t1 >> t2
    t1 >> t3
    t2 >> t4
    t3 >> t4
