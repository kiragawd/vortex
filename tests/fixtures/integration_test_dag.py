from vortex import DAG
from vortex.operators import BashOperator, PythonOperator
from datetime import datetime, timedelta
import os

def task_2_func():
    print("Python task 2 executed successfully")

if __name__ == "__main__":
    task_2_func()

def task_3_func():
    secret = os.environ.get("MY_TEST_SECRET")
    if secret == "top_secret_value":
        print("Secret verified successfully")
    else:
        print(f"Secret verification failed. Found: {secret}")
        raise ValueError("Secret not found or incorrect")

if __name__ == "__main__":
    import sys
    if len(sys.argv) > 1:
        if sys.argv[1] == "task_2": task_2_func()
        if sys.argv[1] == "task_3": task_3_func()

with DAG(
    dag_id="integration_test_dag",
    start_date=datetime(2026, 1, 1),
    schedule_interval=None,
    catchup=False
) as dag:
    
    t1 = BashOperator(
        task_id="task_1",
        bash_command="echo 'Bash task 1 executed'"
    )
    
    t2 = PythonOperator(
        task_id="task_2",
        python_callable=task_2_func
    )
    
    t3 = PythonOperator(
        task_id="task_3",
        python_callable=task_3_func
    )
    
    t1 >> t2 >> t3
