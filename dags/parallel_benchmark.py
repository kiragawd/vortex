from vortex import DAG
from vortex.operators.bash import BashOperator

with DAG(dag_id="parallel_benchmark") as dag:
    t1 = BashOperator(
        task_id="t1",
        bash_command="echo 'Vortex engine warm-up...'"
    )

    t2 = BashOperator(
        task_id="t2",
        bash_command="sleep 1 && echo 'Ingestion A complete'"
    )

    t3 = BashOperator(
        task_id="t3",
        bash_command="sleep 1 && echo 'Ingestion B complete'"
    )

    t4 = BashOperator(
        task_id="t4",
        bash_command="sleep 1 && echo 'Ingestion C complete'"
    )

    t5 = BashOperator(
        task_id="t5",
        bash_command="echo 'All data processed. Vortex out.'"
    )
    
    t6 = BashOperator(
        task_id="t6",
        bash_command="echo 'New Task Jarvis!'"
    )

    t1 >> t2
    t1 >> t3
    t1 >> t4
    t2 >> t5
    t3 >> t5
    t4 >> t5
    t5 >> t6
