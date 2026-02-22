from vortex import Dag, Task

def create_dag():
    dag = Dag(id="stress_test_dag")
    
    # Pillar 3: Secrets test
    # We simulate secret requirement by adding a task that expects an env var
    t1 = Task(id="secret_check", name="Secret Check", command="if [ -z \"$STRESS_TEST_SECRET\" ]; then echo 'Secret missing'; exit 1; else echo 'Secret present'; fi")
    dag.add_task(t1)
    
    # Pillar 1 & 2: Complex dependencies
    prev_tasks = [t1]
    for i in range(1, 4):
        layer = []
        for j in range(3):
            t = Task(id=f"task_{i}_{j}", name=f"Task {i}-{j}", command=f"sleep 2 && echo 'Layer {i} Task {j} complete'")
            dag.add_task(t)
            for pt in prev_tasks:
                dag.add_dependency(pt.id, t.id)
            layer.append(t)
        prev_tasks = layer
        
    final = Task(id="final", name="Final Task", command="echo 'Stress test complete'")
    dag.add_task(final)
    for pt in prev_tasks:
        dag.add_dependency(pt.id, final.id)
        
    return dag

# VORTEX looks for a function or global that returns Dags
dags = [create_dag()]
