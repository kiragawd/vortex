"""
Airflow-compatible example DAG for VORTEX.

This file demonstrates how to migrate an existing Airflow DAG to VORTEX with
zero code changes — just swap the import line.

  Airflow:  from airflow import DAG
            from airflow.operators.bash import BashOperator
            from airflow.operators.python import PythonOperator
            from airflow.operators.dummy import DummyOperator

  VORTEX:   from vortex import DAG, BashOperator, PythonOperator, DummyOperator
"""

from vortex import DAG, BashOperator, PythonOperator, DummyOperator
from datetime import datetime


def process_data():
    print("Processing data...")
    return "done"


with DAG(
    dag_id="airflow_compat_example",
    description="Example DAG using Airflow-compatible syntax",
    schedule_interval="@daily",
    start_date=datetime(2026, 1, 1),
    catchup=False,
    tags=["example", "airflow-compat"],
) as dag:

    start = DummyOperator(task_id="start", dag=dag)

    fetch = BashOperator(
        task_id="fetch_data",
        bash_command="echo 'Fetching data...'",
        dag=dag,
    )

    process = PythonOperator(
        task_id="process_data",
        python_callable=process_data,
        dag=dag,
    )

    end = DummyOperator(task_id="end", dag=dag)

    start >> fetch >> process >> end
