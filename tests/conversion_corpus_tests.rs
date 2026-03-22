use std::fs;
use std::path::Path;
use vortex::airflow_ast_parser::parse_airflow_file;
use vortex::dag_codegen::{
    assert_graph_equivalence, generate_rust_dag_source, validate_generated_rust_source,
};

#[test]
fn test_conversion_corpus_parser_smoke() {
    let dags_dir = Path::new("dags");
    assert!(dags_dir.exists(), "dags directory should exist");

    let mut parsed_count = 0usize;
    let mut file_count = 0usize;
    let mut graph_ok_count = 0usize;

    for entry in fs::read_dir(dags_dir).expect("read dags dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("py") {
            file_count += 1;
            if let Ok(dags) = parse_airflow_file(&path) {
                parsed_count += 1;
                for dag in dags {
                    let (generated, _summary) = generate_rust_dag_source(&dag);
                    validate_generated_rust_source(&generated).expect("generated source must be valid Rust");
                    assert_graph_equivalence(&dag, &generated)
                        .expect("generated DAG graph must match parsed DAG graph");
                    graph_ok_count += 1;
                }
            }
        }
    }

    assert!(file_count > 0, "at least one python dag must exist");
    assert_eq!(
        parsed_count, file_count,
        "strict corpus threshold failed: all DAG files must parse"
    );
    assert!(
        graph_ok_count >= file_count,
        "graph equivalence threshold failed for corpus"
    );
}
