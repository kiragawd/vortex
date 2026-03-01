from datetime import datetime
from vortex import DAG, BaseOperator, PythonOperator
import time

def slow_task():
    print("Sleeping for 15s...")
    time.sleep(15)
    print("Done!")

with DAG(
    "sla_test_dag",
    schedule_interval=None,
    catchup=False,
    sla_seconds=10
) as dag:
    t1 = PythonOperator(
        task_id="slow_task",
        python_callable=slow_task
    )
