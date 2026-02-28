from vortex import DAG, BashOperator

with DAG('dynamic_loops') as dag:
    for i in range(5):
        BashOperator(task_id=f'task_{i}', bash_command=f'echo {i}')

