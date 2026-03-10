/// Integration tests for the VORTEX regex-based Python DAG parser.
/// Run with: PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 cargo test --test parser_tests

use vortex::python_parser::parse_python_dag;

// ─── 1. Simple DAG – single task, no dependencies ────────────────────────────
#[test]
fn test_parse_simple_dag_one_task_no_deps() {
    let src = r#"
from vortex import DAG, BashOperator

with DAG("simple_dag") as dag:
    t1 = BashOperator(
        task_id="hello",
        bash_command="echo hello"
    )
"#;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file, src.as_bytes()).unwrap();
    let temp_path = temp_file.path();
    let dags = parse_python_dag(temp_path.to_str().unwrap()).expect("should parse");
    let dag = &dags[0];
    assert_eq!(dag.id, "simple_dag");
    assert_eq!(dag.tasks.len(), 1);
    
    let task = dag.tasks.get("hello").unwrap();
    assert_eq!(task.task_type, "bash");
    assert!(dag.dependencies.is_empty());
}

// ─── 2. Three tasks with chain dependencies ───────────────────────────────────
#[test]
fn test_parse_dag_chain_deps() {
    let src = r#"
from vortex import DAG, BashOperator

with DAG("chain_dag") as dag:
    t1 = BashOperator(task_id="step1", bash_command="echo 1")
    t2 = BashOperator(task_id="step2", bash_command="echo 2")
    t3 = BashOperator(task_id="step3", bash_command="echo 3")

    t1 >> t2 >> t3
"#;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file, src.as_bytes()).unwrap();
    let temp_path = temp_file.path();
    let dags = parse_python_dag(temp_path.to_str().unwrap()).expect("should parse");
    let dag = &dags[0];
    assert_eq!(dag.id, "chain_dag");
    assert_eq!(dag.tasks.len(), 3);

    let has_edge = |u: &str, d: &str| dag.dependencies.contains(&(u.to_string(), d.to_string()));
    assert!(has_edge("step1", "step2"), "expected t1 >> t2");
    assert!(has_edge("step2", "step3"), "expected t2 >> t3");
}

// ─── 3. BashOperator – bash_command extraction ───────────────────────────────
#[test]
fn test_parse_bash_operator() {
    let src = r#"
from vortex import DAG, BashOperator

with DAG("bash_dag") as dag:
    cleanup = BashOperator(
        task_id="cleanup",
        bash_command="rm -rf /tmp/work"
    )
"#;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file, src.as_bytes()).unwrap();
    let temp_path = temp_file.path();
    let dags = parse_python_dag(temp_path.to_str().unwrap()).expect("should parse");
    let dag = &dags[0];
    let task = dag.tasks.get("cleanup").expect("task not found");
    // Under PyO3 parser, we just store the command string as bash_command.
    // In our new parser everything is just bash_command or python_callable strings in dag.tasks map
    assert_eq!(task.command, "rm -rf /tmp/work");
}

// ─── 4. PythonOperator – python_callable extraction ──────────────────────────
#[test]
fn test_parse_python_operator() {
    let src = r#"
from vortex import DAG, PythonOperator

def my_func():
    pass

with DAG("py_dag") as dag:
    run_py = PythonOperator(
        task_id="run_py",
        python_callable=my_func
    )
"#;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file, src.as_bytes()).unwrap();
    let temp_path = temp_file.path();
    let dags = parse_python_dag(temp_path.to_str().unwrap()).expect("should parse");
    let dag = &dags[0];
    let task = dag.tasks.get("run_py").expect("task not found");
    assert_eq!(task.task_type, "python");
    assert_eq!(task.command, "my_func");
}

// ─── 5. DummyOperator ────────────────────────────────────────────────────────
#[test]
fn test_parse_dummy_operator() {
    let src = r#"
from vortex import DAG, DummyOperator

with DAG("dummy_dag") as dag:
    start = DummyOperator(task_id="start")
    end   = DummyOperator(task_id="end")
    start >> end
"#;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file, src.as_bytes()).unwrap();
    let temp_path = temp_file.path();
    let dags = parse_python_dag(temp_path.to_str().unwrap()).expect("should parse");
    let dag = &dags[0];
    let mut types = Vec::new();
    for task in dag.tasks.values() {
        types.push(task.task_type.as_str());
    }
    assert!(types.iter().all(|&t| t == "bash"), "unknown operators fallback to bash echo 'unknown operator'");
    assert_eq!(dag.tasks.len(), 2);
}

#[test]
fn test_cyclic_dependency_extracted() {
    // A >> B >> A forms a cycle
    let src = r#"
from vortex import DAG, BashOperator

with DAG("cycle_dag") as dag:
    a = BashOperator(task_id="a", bash_command="echo a")
    b = BashOperator(task_id="b", bash_command="echo b")

    a >> b
    b >> a
"#;
    let mut temp_file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file, src.as_bytes()).unwrap();
    let temp_path = temp_file.path();
    let dags = parse_python_dag(temp_path.to_str().unwrap()).expect("should parse");
    let dag = &dags[0];
    // PyO3 parser extracts edges but scheduler::Dag::add_dependency actively drops ones that cycle.
    assert!(dag.dependencies.contains(&("a".to_string(), "b".to_string())), "First edge should be added");
    assert!(!dag.dependencies.contains(&("b".to_string(), "a".to_string())), "Cycle-creating edge should be dropped");
}

// ─── 7. schedule_interval extraction ─────────────────────────────────────────
#[test]
fn test_schedule_interval_extraction() {
    let daily_src = r#"
from vortex import DAG, BashOperator

with DAG("daily_dag", schedule_interval="@daily") as dag:
    t = BashOperator(task_id="t", bash_command="echo hi")
"#;
    let mut temp_file_daily = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file_daily, daily_src.as_bytes()).unwrap();
    let temp_path_daily = temp_file_daily.path();
    let dags = parse_python_dag(temp_path_daily.to_str().unwrap()).expect("should parse");
    assert_eq!(dags[0].schedule_interval.as_deref(), Some("@daily"));

    let cron_src = r#"
from vortex import DAG, BashOperator

with DAG("cron_dag", schedule_interval="0 6 * * *") as dag:
    t = BashOperator(task_id="t", bash_command="echo hi")
"#;
    let mut temp_file_cron = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file_cron, cron_src.as_bytes()).unwrap();
    let temp_path_cron = temp_file_cron.path();
    let dags2 = parse_python_dag(temp_path_cron.to_str().unwrap()).expect("should parse");
    assert_eq!(dags2[0].schedule_interval.as_deref(), Some("0 6 * * *"));
}

// ─── 8. Malformed / empty file returns error ──────────────────────────────────
#[test]
fn test_malformed_empty_file_returns_error() {
    let mut temp_file_empty = tempfile::NamedTempFile::new().unwrap();
    let result_empty = parse_python_dag(temp_file_empty.path().to_str().unwrap());
    assert!(result_empty.is_err() || result_empty.unwrap().is_empty(), "empty file should error or return no DAGs");

    // No dag_id
    let mut temp_file_no_dag = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut temp_file_no_dag, b"x = 1 + 1\nprint(x)").unwrap();
    let result_no_dag = parse_python_dag(temp_file_no_dag.path().to_str().unwrap());
    if let Ok(dags) = &result_no_dag {
        if !dags.is_empty() {
             println!("UNEXPECTED DAGS: {:#?}", dags);
        }
    }
    assert!(result_no_dag.is_err() || result_no_dag.unwrap().is_empty(), "missing dag_id should error or return no DAGs");
}
