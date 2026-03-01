from vortex import DAG
from vortex.operators.bash import BashOperator
import random

# Use a fixed seed so the DAG structure remains consistent across parsing runs
random.seed(42)

with DAG(dag_id="complex_100_job_dag") as dag:
    tasks = []
    
    # Create 100 tasks
    for i in range(100):
        # Random sleep to simulate work and make the UI more dynamic when running
        sleep_time = random.uniform(0.1, 2.0)
        t = BashOperator(
            task_id=f"task_{i}",
            bash_command=f"echo 'Executing task {i}' && sleep {sleep_time:.2f}"
        )
        tasks.append(t)

    # Build a complex dependency graph
    
    # Root task
    root_task = tasks[0]
    
    # Layer 1: Initial fan-out from root (Tasks 1-9)
    for i in range(1, 10):
        root_task >> tasks[i]

    # Layer 2: Deeper fan-out (Tasks 10-29)
    for i in range(10, 30):
        # Pick 1 or 2 random parents from Layer 1
        num_parents = random.randint(1, 2)
        parents = random.sample(range(1, 10), num_parents)
        for p in parents:
            tasks[p] >> tasks[i]

    # Layer 3: Complex mesh (Tasks 30-69)
    for i in range(30, 70):
        # Pick 1 to 3 random parents from previous layers (including this one, but strictly earlier tasks)
        num_parents = random.randint(1, 3)
        parents = random.sample(range(10, i), num_parents)
        for p in parents:
            tasks[p] >> tasks[i]

    # Layer 4: Fan-in starting (Tasks 70-89)
    for i in range(70, 90):
        # Pick 2 to 5 random parents mostly from Layer 3
        num_parents = random.randint(2, 5)
        parents = random.sample(range(40, i), num_parents)
        for p in parents:
            tasks[p] >> tasks[i]

    # Layer 5: Further fan-in (Tasks 90-98)
    for i in range(90, 99):
        # Pick 3 to 8 random parents from Layer 4
        num_parents = random.randint(3, 8)
        parents = random.sample(range(70, i), num_parents)
        for p in parents:
            tasks[p] >> tasks[i]

    # Final task: Sink node (Task 99)
    final_task = tasks[99]
    for i in range(90, 99):
        tasks[i] >> final_task
