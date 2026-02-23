from vortex import DAG
from vortex.operators import PythonOperator
from datetime import datetime
import time

def failing_func():
    print("Attempting failing task...")
    raise RuntimeError("Intentional failure for retry test")

if __name__ == "__main__":
    failing_func()

with DAG(
    dag_id="retry_test_dag",
    start_date=datetime(2026, 1, 1),
    schedule_interval=None,
    catchup=False,
    default_args={
        "retries": 2,
        "retry_delay": 1
    }
) as dag:
    
    t1 = PythonOperator(
        task_id="failing_task",
        python_callable=failing_func
    )
