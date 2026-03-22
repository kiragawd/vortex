use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstTask {
    pub task_id: String,
    pub operator_type: String,
    pub python_callable: Option<String>,
    pub bash_command: Option<String>,
    pub raw_kwargs: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstDag {
    pub dag_id: String,
    pub schedule_interval: Option<String>,
    pub tasks: Vec<AstTask>,
    pub edges: Vec<(String, String)>,
    pub source_file: String,
}

pub fn parse_airflow_file<P: AsRef<Path>>(path: P) -> Result<Vec<AstDag>> {
    let path_ref = path.as_ref();
    let source = std::fs::read_to_string(path_ref)
        .with_context(|| format!("Failed to read DAG file: {}", path_ref.display()))?;
    parse_airflow_source(
        &source,
        path_ref
            .to_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "<in-memory>".to_string()),
    )
}

pub fn parse_airflow_source(source: &str, source_file: String) -> Result<Vec<AstDag>> {
    let dag_ctor = Regex::new(r#"DAG\s*\((?P<args>[^\)]*)\)"#)?;
    let dag_id_re = Regex::new(r#"dag_id\s*=\s*[\"'](?P<id>[^\"']+)[\"']"#)?;
    let schedule_re = Regex::new(r#"schedule_interval\s*=\s*[\"'](?P<s>[^\"']+)[\"']"#)?;

    let task_re = Regex::new(
        r#"(?m)^(?P<var>\w+)\s*=\s*(?P<op>\w+Operator)\s*\((?P<args>[^\)]*)\)"#,
    )?;
    let task_id_re = Regex::new(r#"task_id\s*=\s*[\"'](?P<id>[^\"']+)[\"']"#)?;
    let bash_re = Regex::new(r#"bash_command\s*=\s*[\"'](?P<cmd>[^\"']+)[\"']"#)?;
    let callable_re = Regex::new(r#"python_callable\s*=\s*(?P<c>\w+)"#)?;
    let dep_re = Regex::new(r#"(?m)^(?P<l>\w+)\s*>>\s*(?P<r>\w+)\s*$"#)?;

    let mut dag_id = "default_dag".to_string();
    let mut schedule_interval = None;

    if let Some(m) = dag_ctor.captures(source) {
        let args = m.name("args").map(|x| x.as_str()).unwrap_or_default();
        if let Some(id_cap) = dag_id_re.captures(args) {
            dag_id = id_cap["id"].to_string();
        }
        if let Some(s_cap) = schedule_re.captures(args) {
            schedule_interval = Some(s_cap["s"].to_string());
        }
    }

    let mut tasks: Vec<AstTask> = Vec::new();
    let mut var_to_task: HashMap<String, String> = HashMap::new();

    for cap in task_re.captures_iter(source) {
        let var = cap["var"].to_string();
        let operator_type = cap["op"].to_string();
        let args = cap["args"].to_string();

        let task_id = task_id_re
            .captures(&args)
            .and_then(|c| c.name("id").map(|m| m.as_str().to_string()))
            .unwrap_or_else(|| var.clone());

        let bash_command = bash_re
            .captures(&args)
            .and_then(|c| c.name("cmd").map(|m| m.as_str().to_string()));
        let python_callable = callable_re
            .captures(&args)
            .and_then(|c| c.name("c").map(|m| m.as_str().to_string()));

        let mut raw_kwargs = HashMap::new();
        raw_kwargs.insert("raw_args".to_string(), args.clone());

        tasks.push(AstTask {
            task_id: task_id.clone(),
            operator_type,
            python_callable,
            bash_command,
            raw_kwargs,
        });
        var_to_task.insert(var, task_id);
    }

    let mut edges = Vec::new();
    for cap in dep_re.captures_iter(source) {
        let left_var = cap["l"].to_string();
        let right_var = cap["r"].to_string();
        if let (Some(left), Some(right)) = (var_to_task.get(&left_var), var_to_task.get(&right_var)) {
            edges.push((left.clone(), right.clone()));
        }
    }

    let dag = AstDag {
        dag_id,
        schedule_interval,
        tasks,
        edges,
        source_file,
    };
    validate_dag(&dag)?;
    Ok(vec![dag])
}

pub fn validate_dag(dag: &AstDag) -> Result<()> {
    let mut task_ids = HashSet::new();
    for t in &dag.tasks {
        if !task_ids.insert(t.task_id.clone()) {
            return Err(anyhow!("Duplicate task_id found: {}", t.task_id));
        }
    }

    for (up, down) in &dag.edges {
        if !task_ids.contains(up) {
            return Err(anyhow!("Dependency references unknown upstream task: {}", up));
        }
        if !task_ids.contains(down) {
            return Err(anyhow!("Dependency references unknown downstream task: {}", down));
        }
    }

    Ok(())
}
