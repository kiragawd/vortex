import unittest
from vortex import DAG, BashOperator, PythonOperator, DummyOperator, EmptyOperator

class TestAirflowShim(unittest.TestCase):

    def test_dag_instantiation(self):
        """1. DAG instantiation with all metadata (dag_id, schedule_interval, description)"""
        dag = DAG(
            dag_id="test_dag",
            schedule_interval="@daily",
            description="A test DAG"
        )
        self.assertEqual(dag.dag_id, "test_dag")
        self.assertEqual(dag.schedule_interval, "@daily")
        self.assertEqual(dag.description, "A test DAG")

    def test_bash_operator(self):
        """2. BashOperator created and attached to DAG, check task_id and bash_command"""
        dag = DAG(dag_id="test_bash")
        task = BashOperator(
            task_id="bash_task",
            bash_command="echo 'hello'",
            dag=dag
        )
        self.assertEqual(task.task_id, "bash_task")
        self.assertEqual(task.bash_command, "echo 'hello'")
        self.assertIn(task, dag.tasks)
        self.assertEqual(dag._task_dict["bash_task"], task)

    def test_python_operator(self):
        """3. PythonOperator with python_callable"""
        def my_func():
            return "hello"
        
        dag = DAG(dag_id="test_python")
        task = PythonOperator(
            task_id="python_task",
            python_callable=my_func,
            dag=dag
        )
        self.assertEqual(task.task_id, "python_task")
        self.assertEqual(task.python_callable, my_func)
        self.assertIn(task, dag.tasks)

    def test_dummy_operator(self):
        """4. DummyOperator"""
        dag = DAG(dag_id="test_dummy")
        task = DummyOperator(task_id="dummy_task", dag=dag)
        self.assertEqual(task.task_id, "dummy_task")
        self.assertIn(task, dag.tasks)
        
        # Also check EmptyOperator
        task2 = EmptyOperator(task_id="empty_task", dag=dag)
        self.assertEqual(task2.task_id, "empty_task")
        self.assertIn(task2, dag.tasks)

    def test_task_chain_dependency(self):
        """5. Task chain dependency (task1 >> task2 >> task3, check _upstream/_downstream)"""
        dag = DAG(dag_id="test_chain")
        t1 = DummyOperator(task_id="t1", dag=dag)
        t2 = DummyOperator(task_id="t2", dag=dag)
        t3 = DummyOperator(task_id="t3", dag=dag)
        
        t1 >> t2 >> t3
        
        self.assertIn("t2", t1._downstream)
        self.assertIn("t1", t2._upstream)
        self.assertIn("t3", t2._downstream)
        self.assertIn("t2", t3._upstream)

    def test_context_manager(self):
        """6. Context manager syntax (with DAG(...) as dag: verify dag object returned)"""
        with DAG(dag_id="test_context") as dag:
            self.assertEqual(dag.dag_id, "test_context")
            t1 = DummyOperator(task_id="t1")
            # In Airflow-style context manager, tasks usually find the DAG if it's the current context
            # Let's see if our shim supports this.
            # Looking at airflow_shim.py, it DOES NOT seem to support global context.
            # Wait, the native DAG in __init__.py DID support it.
            # But the shim DAG doesn't have the __enter__ logic to set a global context.
            # Let's check if I should add it.
        
        self.assertIsInstance(dag, DAG)

if __name__ == "__main__":
    unittest.main()
