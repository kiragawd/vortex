use crate::airflow_ast_parser::AstDag;
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationSummary {
    pub dag_id: String,
    pub generated_file: String,
    pub converted_tasks: usize,
    pub placeholder_tasks: usize,
}

pub fn generate_rust_dag_source(dag: &AstDag) -> (String, GenerationSummary) {
    generate_rust_dag_source_with_overrides(dag, &HashMap::new())
}

/// Generates a Rust source file that builds a [`Dag`] from the parsed Airflow AST.
///
/// `python_overrides` maps task IDs to translated command strings for PythonOperator
/// tasks. Tasks without an override emit a failing error stub so the unresolved
/// operator is immediately visible at runtime.
pub fn generate_rust_dag_source_with_overrides(
    dag: &AstDag,
    python_overrides: &HashMap<String, String>,
) -> (String, GenerationSummary) {
    let mut source = String::new();
    source.push_str("use ryuo::scheduler::Dag;\n\n");
    source.push_str("pub fn build_dag() -> Dag {\n");
    source.push_str(&format!("    let mut dag = Dag::new(\"{}\");\n", dag.dag_id));
    if let Some(schedule) = &dag.schedule_interval {
        source.push_str(&format!("    dag.set_schedule(\"{}\");\n", schedule));
    }

    let mut converted = 0usize;
    let mut placeholders = 0usize;

    for t in &dag.tasks {
        if t.operator_type.contains("Bash") {
            let cmd = t.bash_command.clone().unwrap_or_default().replace('"', "\\\"");
            source.push_str(&format!(
                "    dag.add_task(\"{}\", \"{}\", \"{}\");\n",
                t.task_id, t.task_id, cmd
            ));
            converted += 1;
        } else if t.operator_type.contains("Python") {
            if let Some(translated) = python_overrides.get(&t.task_id) {
                let payload = translated.replace('"', "\\\"");
                source.push_str(&format!(
                    "    dag.add_python_task(\"{}\", \"{}\", \"{}\");\n",
                    t.task_id, t.task_id, payload
                ));
                converted += 1;
            } else {
                placeholders += 1;
                source.push_str(&format!(
                    "    // ERROR: unresolved PythonOperator — run migration with --python-overrides\n"
                ));
                source.push_str(&format!(
                    "    dag.add_python_task(\"{}\", \"{}\", \"echo 'ERROR: Unresolved PythonOperator for task {}. Run migration with --python-overrides to provide implementation.' && exit 1\");\n",
                    t.task_id, t.task_id, t.task_id
                ));
            }
        } else {
            placeholders += 1;
            source.push_str(&format!(
                "    // unsupported operator {}; preserve as placeholder\n",
                t.operator_type
            ));
            source.push_str(&format!(
                "    dag.add_task(\"{}\", \"{}\", \"echo unsupported operator {}\");\n",
                t.task_id, t.task_id, t.operator_type
            ));
        }
    }

    for (up, down) in &dag.edges {
        source.push_str(&format!("    dag.add_dependency(\"{}\", \"{}\");\n", up, down));
    }

    source.push_str("    dag\n}\n");

    (
        source,
        GenerationSummary {
            dag_id: dag.dag_id.clone(),
            generated_file: format!("{}_generated.rs", dag.dag_id),
            converted_tasks: converted,
            placeholder_tasks: placeholders,
        },
    )
}

pub fn write_generated_dag<P: AsRef<Path>>(out_dir: P, dag: &AstDag) -> Result<GenerationSummary> {
    let (summary, _) = write_generated_dag_with_overrides(out_dir, dag, &HashMap::new())?;
    Ok(summary)
}

pub fn write_generated_dag_with_overrides<P: AsRef<Path>>(
    out_dir: P,
    dag: &AstDag,
    python_overrides: &HashMap<String, String>,
) -> Result<(GenerationSummary, String)> {
    let out_dir_ref = out_dir.as_ref();
    std::fs::create_dir_all(out_dir_ref)
        .with_context(|| format!("Failed to create output dir: {}", out_dir_ref.display()))?;

    let (source, summary) = generate_rust_dag_source_with_overrides(dag, python_overrides);
    let file_path = out_dir_ref.join(&summary.generated_file);
    std::fs::write(&file_path, source)
        .with_context(|| format!("Failed to write generated file: {}", file_path.display()))?;
    let generated = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Failed to read generated file: {}", file_path.display()))?;
    Ok((summary, generated))
}

pub fn write_migration_report<P: AsRef<Path>>(out_dir: P, summaries: &[GenerationSummary]) -> Result<()> {
    let report_path = out_dir.as_ref().join("migration_report.json");
    let content = serde_json::to_string_pretty(summaries)?;
    std::fs::write(&report_path, content)
        .with_context(|| format!("Failed to write migration report: {}", report_path.display()))?;
    Ok(())
}

pub fn validate_generated_rust_source(source: &str) -> Result<()> {
    syn::parse_file(source).map_err(|e| anyhow!("Generated Rust syntax is invalid: {}", e))?;
    Ok(())
}

pub fn extract_generated_edges(source: &str) -> Result<Vec<(String, String)>> {
    let edge_re = Regex::new(
        r#"dag\.add_dependency\(\"(?P<up>[^\"]+)\",\s*\"(?P<down>[^\"]+)\"\)"#,
    )?;
    let mut edges = Vec::new();
    for cap in edge_re.captures_iter(source) {
        edges.push((cap["up"].to_string(), cap["down"].to_string()));
    }
    Ok(edges)
}

pub fn assert_graph_equivalence(dag: &AstDag, generated_source: &str) -> Result<()> {
    let mut expected = dag.edges.clone();
    let mut actual = extract_generated_edges(generated_source)?;
    expected.sort();
    actual.sort();
    if expected != actual {
        return Err(anyhow!(
            "Graph mismatch for DAG {}: expected {:?}, actual {:?}",
            dag.dag_id,
            expected,
            actual
        ));
    }
    Ok(())
}
