use vortex::airflow_ast_parser::parse_airflow_source;
use vortex::dag_codegen::{
    assert_graph_equivalence, generate_rust_dag_source, validate_generated_rust_source,
};

#[test]
fn test_static_parser_extracts_tasks_and_edges() {
    let src = r#"
from airflow import DAG
from airflow.operators.bash import BashOperator

dag = DAG(dag_id="demo", schedule_interval="@daily")
t1 = BashOperator(task_id="a", bash_command="echo a")
t2 = BashOperator(task_id="b", bash_command="echo b")
t1 >> t2
"#;

    let dags = parse_airflow_source(src, "inline.py".to_string()).expect("parser should succeed");
    assert_eq!(dags.len(), 1);
    let dag = &dags[0];
    assert_eq!(dag.dag_id, "demo");
    assert_eq!(dag.tasks.len(), 2);
    assert_eq!(dag.edges.len(), 1);
}

#[test]
fn test_codegen_emits_build_dag_function() {
    let src = r#"
from airflow import DAG
from airflow.operators.bash import BashOperator
dag = DAG(dag_id="demo_codegen", schedule_interval="@hourly")
t1 = BashOperator(task_id="a", bash_command="echo hi")
"#;
    let dags = parse_airflow_source(src, "inline_codegen.py".to_string()).expect("parse");
    let (generated, summary) = generate_rust_dag_source(&dags[0]);
    assert!(generated.contains("pub fn build_dag() -> Dag"));
    assert!(generated.contains("dag.add_task"));
    validate_generated_rust_source(&generated).expect("generated source should parse");
    assert_graph_equivalence(&dags[0], &generated).expect("graph should be equivalent");
    assert_eq!(summary.dag_id, "demo_codegen");
}
