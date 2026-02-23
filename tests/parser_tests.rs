/// Integration tests for the VORTEX regex-based Python DAG parser.
/// Run with: PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --test parser_tests

use vortex::python_parser::parse_dag_file;

// ─── 1. Simple DAG – single task, no dependencies ────────────────────────────
#[test]
fn test_parse_simple_dag_one_task_no_deps() {
    let src = r#"
from vortex import DAG
from vortex.operators.bash import BashOperator

with DAG("simple_dag") as dag:
    t1 = BashOperator(
        task_id="hello",
        bash_command="echo hello"
    )
"#;
    let dag = parse_dag_file(src).expect("should parse");
    assert_eq!(dag.dag_id, "simple_dag");
    assert_eq!(dag.tasks.len(), 1);
    assert_eq!(dag.tasks[0].task_id, "hello");
    assert_eq!(dag.tasks[0].task_type, "bash");
    assert!(dag.edges.is_empty());
}

// ─── 2. Three tasks with chain dependencies ───────────────────────────────────
#[test]
fn test_parse_dag_chain_deps() {
    let src = r#"
from vortex import DAG
from vortex.operators.bash import BashOperator

with DAG("chain_dag") as dag:
    t1 = BashOperator(task_id="step1", bash_command="echo 1")
    t2 = BashOperator(task_id="step2", bash_command="echo 2")
    t3 = BashOperator(task_id="step3", bash_command="echo 3")

    t1 >> t2 >> t3
"#;
    let dag = parse_dag_file(src).expect("should parse");
    assert_eq!(dag.dag_id, "chain_dag");
    assert_eq!(dag.tasks.len(), 3);

    let has_edge = |u: &str, d: &str| dag.edges.contains(&(u.to_string(), d.to_string()));
    assert!(has_edge("t1", "t2"), "expected t1 >> t2");
    assert!(has_edge("t2", "t3"), "expected t2 >> t3");
}

// ─── 3. BashOperator – bash_command extraction ───────────────────────────────
#[test]
fn test_parse_bash_operator() {
    let src = r#"
with DAG("bash_dag") as dag:
    cleanup = BashOperator(
        task_id="cleanup",
        bash_command="rm -rf /tmp/work"
    )
"#;
    let dag = parse_dag_file(src).expect("should parse");
    let task = dag.tasks.iter().find(|t| t.task_id == "cleanup").expect("task not found");
    assert_eq!(task.task_type, "bash");
    assert_eq!(
        task.config["bash_command"].as_str().unwrap(),
        "rm -rf /tmp/work"
    );
}

// ─── 4. PythonOperator – python_callable extraction ──────────────────────────
#[test]
fn test_parse_python_operator() {
    let src = r#"
def my_func():
    pass

with DAG("py_dag") as dag:
    run_py = PythonOperator(
        task_id="run_py",
        python_callable=my_func
    )
"#;
    let dag = parse_dag_file(src).expect("should parse");
    let task = dag.tasks.iter().find(|t| t.task_id == "run_py").expect("task not found");
    assert_eq!(task.task_type, "python");
    assert_eq!(
        task.config["python_callable"].as_str().unwrap(),
        "my_func"
    );
}

// ─── 5. DummyOperator ────────────────────────────────────────────────────────
#[test]
fn test_parse_dummy_operator() {
    let src = r#"
with DAG("dummy_dag") as dag:
    start = DummyOperator(task_id="start")
    end   = DummyOperator(task_id="end")
    start >> end
"#;
    let dag = parse_dag_file(src).expect("should parse");
    let types: Vec<&str> = dag.tasks.iter().map(|t| t.task_type.as_str()).collect();
    assert!(types.iter().all(|&t| t == "noop"), "all tasks should be noop");
    assert_eq!(dag.tasks.len(), 2);
}

// ─── 6. Cyclic dependency detection ──────────────────────────────────────────
#[test]
fn test_cyclic_dependency_detected() {
    // A >> B >> A forms a cycle
    let src = r#"
with DAG("cycle_dag") as dag:
    a = BashOperator(task_id="a", bash_command="echo a")
    b = BashOperator(task_id="b", bash_command="echo b")

    a >> b
    b >> a
"#;
    let result = parse_dag_file(src);
    assert!(result.is_err(), "should detect cycle");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("Cyclic"), "error should mention cycle: {}", msg);
}

// ─── 7. schedule_interval extraction ─────────────────────────────────────────
#[test]
fn test_schedule_interval_extraction() {
    let daily_src = r#"
with DAG("daily_dag", schedule_interval="@daily") as dag:
    t = BashOperator(task_id="t", bash_command="echo hi")
"#;
    let dag = parse_dag_file(daily_src).expect("should parse");
    assert_eq!(dag.schedule_interval.as_deref(), Some("@daily"));

    let cron_src = r#"
with DAG("cron_dag", schedule_interval="0 6 * * *") as dag:
    t = BashOperator(task_id="t", bash_command="echo hi")
"#;
    let dag2 = parse_dag_file(cron_src).expect("should parse");
    assert_eq!(dag2.schedule_interval.as_deref(), Some("0 6 * * *"));
}

// ─── 8. Malformed / empty file returns error ──────────────────────────────────
#[test]
fn test_malformed_empty_file_returns_error() {
    // Completely empty
    let result_empty = parse_dag_file("");
    assert!(result_empty.is_err(), "empty file should error");

    // No dag_id
    let result_no_dag = parse_dag_file("x = 1 + 1\nprint(x)");
    assert!(result_no_dag.is_err(), "missing dag_id should error");
}
