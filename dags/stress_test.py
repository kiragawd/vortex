from vortex import DAG
from vortex.operators.bash import BashOperator

# A small stress-test DAG using the Airflow-compatible shim so VORTEX can parse it
with DAG(dag_id="stress_test_dag") as dag:
    # Secrets test — task will fail if env var not set
    t1 = BashOperator(task_id="secret_check", bash_command="if [ -z \"$STRESS_TEST_SECRET\" ]; then echo 'Secret missing'; exit 1; else echo 'Secret present'; fi")

    # Complex layered dependencies
    prev_tasks = [t1]
    for i in range(1, 4):
        layer = []
        for j in range(3):
            t = BashOperator(task_id=f"task_{i}_{j}", bash_command=f"sleep 2 && echo 'Layer {i} Task {j} complete'")
            for pt in prev_tasks:
                pt >> t
            layer.append(t)
        prev_tasks = layer

    final = BashOperator(task_id="final", bash_command="echo 'Stress test complete'")
    for pt in prev_tasks:
        pt >> final

