#![allow(dead_code)]
// Legacy Scheduler Migration
//
// Parsers for TWS (Tivoli Workload Scheduler) and Autosys JIL definitions,
// migration CLI tooling, and Airflow-to-Vortex transpilation helpers.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::info;

// ─── Unified Migration Types ──────────────────────────────────

/// Source scheduler type being migrated from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SourceScheduler {
    Airflow,
    Tws,
    Autosys,
}

/// A parsed job/task from any legacy scheduler format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationJob {
    pub source: SourceScheduler,
    pub job_name: String,
    pub job_type: String,
    pub command: Option<String>,
    pub schedule: Option<String>,
    pub dependencies: Vec<String>,
    pub conditions: Vec<String>,
    pub resources: HashMap<String, String>,
    pub notifications: Vec<MigrationNotification>,
    pub properties: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationNotification {
    pub event: String,
    pub method: String,
    pub target: String,
}

/// Result of a migration conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub source: SourceScheduler,
    pub source_file: String,
    pub jobs_parsed: usize,
    pub jobs_converted: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub vortex_dag: VortexDagDef,
}

/// Vortex DAG definition output from migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VortexDagDef {
    pub dag_id: String,
    pub description: String,
    pub schedule: Option<String>,
    pub tasks: Vec<VortexTaskDef>,
    pub default_args: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VortexTaskDef {
    pub task_id: String,
    pub operator: String,
    pub config: Value,
    pub dependencies: Vec<String>,
    pub retries: u32,
    pub retry_delay_secs: u64,
}

// ─── TWS (Tivoli Workload Scheduler) Parser ───────────────────

/// Parse a TWS job stream definition file.
pub fn parse_tws_definition(content: &str, source_file: &str) -> Result<Vec<MigrationJob>> {
    let mut jobs = Vec::new();
    let mut current_job: Option<MigrationJob> = None;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // TWS job definition starts with job name
        if trimmed.starts_with("SCHEDULE") || trimmed.starts_with("JOBS") || trimmed == "END" {
            // Job stream header/footer — skip
            continue;
        }

        // Check indentation on the ORIGINAL line (before trim)
        let is_indented = raw_line.starts_with(' ') || raw_line.starts_with('\t');

        if !is_indented && trimmed.contains(' ') {
            // New job definition: "jobname  DOCOMMAND ..."
            if let Some(job) = current_job.take() {
                jobs.push(job);
            }

            let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
            let job_name = parts[0].trim().to_string();
            let rest = parts.get(1).map(|s| s.trim()).unwrap_or("");

            current_job = Some(MigrationJob {
                source: SourceScheduler::Tws,
                job_name,
                job_type: detect_tws_job_type(rest),
                command: extract_tws_command(rest),
                schedule: None,
                dependencies: Vec::new(),
                conditions: Vec::new(),
                resources: HashMap::new(),
                notifications: Vec::new(),
                properties: HashMap::new(),
            });
        } else if let Some(ref mut job) = current_job {
            // Continuation lines with job properties
            if let Some(val) = trimmed.strip_prefix("DESCRIPTION") {
                job.properties.insert("description".to_string(), Value::String(val.trim().trim_matches('"').to_string()));
            } else if let Some(val) = trimmed.strip_prefix("FOLLOWS") {
                let deps: Vec<String> = val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                job.dependencies.extend(deps);
            } else if let Some(val) = trimmed.strip_prefix("AT") {
                job.schedule = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("OPENS") {
                job.resources.insert("opens".to_string(), val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("RECOVERY") {
                job.properties.insert("recovery".to_string(), Value::String(val.trim().to_string()));
            } else if let Some(val) = trimmed.strip_prefix("PRIORITY") {
                job.properties.insert("priority".to_string(), Value::String(val.trim().to_string()));
            }
        }
    }

    if let Some(job) = current_job {
        jobs.push(job);
    }

    info!(source = %source_file, job_count = jobs.len(), "TWS definition parsed");
    Ok(jobs)
}

fn detect_tws_job_type(rest: &str) -> String {
    if rest.contains("DOCOMMAND") || rest.contains("SCRIPTNAME") {
        "command".to_string()
    } else if rest.contains("FTP") {
        "file_transfer".to_string()
    } else if rest.contains("RECOVERY") {
        "recovery".to_string()
    } else {
        "generic".to_string()
    }
}

fn extract_tws_command(rest: &str) -> Option<String> {
    // Extract command from DOCOMMAND "..." or SCRIPTNAME "..."
    if let Some(pos) = rest.find("DOCOMMAND") {
        let after = &rest[pos + 9..].trim();
        extract_quoted(after)
    } else if let Some(pos) = rest.find("SCRIPTNAME") {
        let after = &rest[pos + 10..].trim();
        extract_quoted(after)
    } else {
        None
    }
}

fn extract_quoted(s: &str) -> Option<String> {
    if s.starts_with('"') {
        s[1..].find('"').map(|end| s[1..end + 1].to_string())
    } else {
        s.split_whitespace().next().map(String::from)
    }
}

// ─── Autosys JIL Parser ──────────────────────────────────────

/// Parse an Autosys JIL (Job Information Language) file.
pub fn parse_autosys_jil(content: &str, source_file: &str) -> Result<Vec<MigrationJob>> {
    let mut jobs = Vec::new();
    let mut current_job: Option<MigrationJob> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("/*") {
            continue;
        }

        // JIL job definition: "insert_job: job_name  job_type: CMD"
        if line.starts_with("insert_job:") {
            if let Some(job) = current_job.take() {
                jobs.push(job);
            }

            let rest = &line["insert_job:".len()..].trim();
            let parts: Vec<&str> = rest.splitn(2, "job_type:").collect();
            let job_name = parts[0].trim().to_string();
            let job_type = parts.get(1).map(|s| s.trim().to_string()).unwrap_or("CMD".to_string());

            current_job = Some(MigrationJob {
                source: SourceScheduler::Autosys,
                job_name,
                job_type: job_type.to_lowercase(),
                command: None,
                schedule: None,
                dependencies: Vec::new(),
                conditions: Vec::new(),
                resources: HashMap::new(),
                notifications: Vec::new(),
                properties: HashMap::new(),
            });
        } else if let Some(ref mut job) = current_job {
            // JIL attribute lines: "attribute: value"
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "command" => { job.command = Some(value.to_string()); }
                    "machine" => { job.resources.insert("machine".to_string(), value.to_string()); }
                    "owner" => { job.resources.insert("owner".to_string(), value.to_string()); }
                    "permission" => { job.resources.insert("permission".to_string(), value.to_string()); }
                    "date_conditions" => { job.properties.insert("date_conditions".to_string(), Value::String(value.to_string())); }
                    "days_of_week" => { job.schedule = Some(format!("days_of_week:{}", value)); }
                    "start_times" => {
                        let existing = job.schedule.clone().unwrap_or_default();
                        job.schedule = Some(format!("{},start_times:{}", existing, value));
                    }
                    "condition" => {
                        // Parse conditions like "s(job1) & s(job2)"
                        let deps = parse_autosys_conditions(value);
                        job.dependencies.extend(deps);
                        job.conditions.push(value.to_string());
                    }
                    "box_name" => { job.properties.insert("box_name".to_string(), Value::String(value.to_string())); }
                    "description" => { job.properties.insert("description".to_string(), Value::String(value.to_string())); }
                    "std_out_file" => { job.properties.insert("stdout".to_string(), Value::String(value.to_string())); }
                    "std_err_file" => { job.properties.insert("stderr".to_string(), Value::String(value.to_string())); }
                    "max_run_alarm" => { job.properties.insert("max_run_alarm".to_string(), Value::String(value.to_string())); }
                    "alarm_if_fail" => { job.properties.insert("alarm_if_fail".to_string(), Value::String(value.to_string())); }
                    "profile" => { job.resources.insert("profile".to_string(), value.to_string()); }
                    "n_retrys" => { job.properties.insert("retries".to_string(), Value::String(value.to_string())); }
                    other => { job.properties.insert(other.to_string(), Value::String(value.to_string())); }
                }
            }
        }
    }

    if let Some(job) = current_job {
        jobs.push(job);
    }

    info!(source = %source_file, job_count = jobs.len(), "Autosys JIL parsed");
    Ok(jobs)
}

/// Parse Autosys condition strings like "s(job1) & s(job2)" to extract dependency job names.
// NOTE: Negation operators (!s, n) are not yet supported. Negated conditions
// will be treated as positive dependencies, which may produce incorrect DAGs.
// See TASK-20 in tasks.md for tracking.
fn parse_autosys_conditions(condition: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut remaining = condition;
    while let Some(pos) = remaining.find('(') {
        if let Some(end) = remaining[pos..].find(')') {
            let dep_name = &remaining[pos + 1..pos + end];
            if !dep_name.is_empty() {
                deps.push(dep_name.to_string());
            }
            remaining = &remaining[pos + end + 1..];
        } else {
            break;
        }
    }
    deps
}

// ─── Migration Converter ──────────────────────────────────────

/// Convert parsed legacy jobs into a Vortex DAG definition.
pub fn convert_to_vortex_dag(jobs: &[MigrationJob], dag_id: &str) -> MigrationResult {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut tasks = Vec::new();

    let source = jobs.first().map(|j| j.source.clone()).unwrap_or(SourceScheduler::Airflow);

    for job in jobs {
        match convert_job(job) {
            Ok((task, job_warnings)) => {
                tasks.push(task);
                warnings.extend(job_warnings);
            }
            Err(e) => {
                errors.push(format!("Job '{}': {}", job.job_name, e));
            }
        }
    }

    // Resolve dependencies — remap job names to task IDs
    let valid_ids: std::collections::HashSet<String> = tasks.iter().map(|t| t.task_id.clone()).collect();
    for task in &mut tasks {
        let tid = task.task_id.clone();
        let sanitized_deps: Vec<String> = task.dependencies.iter().map(|d| sanitize_id(d)).collect();
        task.dependencies = sanitized_deps.into_iter().filter(|dep| {
            if valid_ids.contains(dep.as_str()) {
                true
            } else {
                warnings.push(format!(
                    "Task '{}': dependency '{}' not found in job set, removed",
                    tid, dep
                ));
                false
            }
        }).collect();
    }

    // Extract schedule from first job that has one
    let schedule = jobs.iter().find_map(|j| j.schedule.clone());

    MigrationResult {
        source: source.clone(),
        source_file: String::new(),
        jobs_parsed: jobs.len(),
        jobs_converted: tasks.len(),
        warnings,
        errors,
        vortex_dag: VortexDagDef {
            dag_id: dag_id.to_string(),
            description: format!("Migrated from {:?}", source),
            schedule,
            tasks,
            default_args: HashMap::new(),
        },
    }
}

fn convert_job(job: &MigrationJob) -> Result<(VortexTaskDef, Vec<String>)> {
    let mut warnings = Vec::new();
    let task_id = sanitize_id(&job.job_name);

    let (operator, config) = match job.job_type.as_str() {
        "cmd" | "command" | "shell" | "bash" => {
            let command = job.command.clone().unwrap_or_else(|| {
                warnings.push(format!("Job '{}': no command specified", job.job_name));
                "echo 'NO COMMAND'".to_string()
            });
            ("ShellOperator".to_string(), serde_json::json!({"command": command}))
        }
        "file_transfer" | "ft" | "ftp" => {
            ("FileTransferOperator".to_string(), serde_json::json!({
                "source": job.resources.get("source").unwrap_or(&String::new()),
                "destination": job.resources.get("destination").unwrap_or(&String::new()),
                "original_job": job.job_name,
            }))
        }
        "box" => {
            warnings.push(format!("Job '{}': Autosys BOX converted to task group", job.job_name));
            ("TaskGroupOperator".to_string(), serde_json::json!({"group": true}))
        }
        other => {
            warnings.push(format!("Job '{}': unknown type '{}', defaulting to ShellOperator", job.job_name, other));
            let config = serde_json::json!({
                "command": job.command.clone().unwrap_or("echo 'migrated'".to_string()),
                "original_type": other,
            });
            ("ShellOperator".to_string(), config)
        }
    };

    let retries = job.properties.get("retries")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Ok((VortexTaskDef {
        task_id,
        operator,
        config,
        dependencies: job.dependencies.clone(),
        retries,
        retry_delay_secs: 60,
    }, warnings))
}

/// Sanitize a job name to a valid Vortex task ID.
fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase()
}

// ─── Code Generator ───────────────────────────────────────────

/// Generate Vortex Rust DAG code from a VortexDagDef.
pub fn generate_rust_dag_code(dag: &VortexDagDef) -> String {
    let mut code = String::new();
    code.push_str(&format!("// Auto-generated Vortex DAG: {}\n", dag.dag_id));
    code.push_str(&format!("// {}\n\n", dag.description));
    code.push_str("use serde_json::json;\n\n");
    code.push_str(&format!("pub fn create_dag() -> serde_json::Value {{\n"));
    code.push_str(&format!("    json!({{\n"));
    code.push_str(&format!("        \"dag_id\": \"{}\",\n", dag.dag_id));
    if let Some(ref schedule) = dag.schedule {
        code.push_str(&format!("        \"schedule\": \"{}\",\n", schedule));
    }
    code.push_str("        \"tasks\": [\n");

    for (i, task) in dag.tasks.iter().enumerate() {
        code.push_str("            {\n");
        code.push_str(&format!("                \"task_id\": \"{}\",\n", task.task_id));
        code.push_str(&format!("                \"operator\": \"{}\",\n", task.operator));
        code.push_str(&format!("                \"config\": {},\n", serde_json::to_string_pretty(&task.config).unwrap_or("{}".into())));
        code.push_str(&format!("                \"dependencies\": {:?},\n", task.dependencies));
        code.push_str(&format!("                \"retries\": {}\n", task.retries));
        code.push_str("            }");
        if i < dag.tasks.len() - 1 { code.push(','); }
        code.push('\n');
    }

    code.push_str("        ]\n");
    code.push_str("    }})\n");
    code.push_str("}\n");
    code
}

/// Generate a Python-compatible DAG definition for Vortex Python SDK.
pub fn generate_python_dag_code(dag: &VortexDagDef) -> String {
    let mut code = String::new();
    code.push_str(&format!("# Auto-generated Vortex DAG: {}\n", dag.dag_id));
    code.push_str(&format!("# {}\n\n", dag.description));
    code.push_str("from vortex import DAG, ShellOperator\n\n");
    code.push_str(&format!("with DAG(\"{}\", description=\"{}\") as dag:\n", dag.dag_id, dag.description));

    for task in &dag.tasks {
        let var_name = &task.task_id;
        match task.operator.as_str() {
            "ShellOperator" => {
                let cmd = task.config.get("command").and_then(|v| v.as_str()).unwrap_or("echo 'task'");
                code.push_str(&format!("    {} = ShellOperator(task_id=\"{}\", command=\"{}\")\n", var_name, task.task_id, cmd));
            }
            _ => {
                code.push_str(&format!("    # {} (operator: {})\n", task.task_id, task.operator));
                code.push_str(&format!("    {} = ShellOperator(task_id=\"{}\", command=\"echo '{}'\")\n", var_name, task.task_id, task.task_id));
            }
        }
    }

    // Add dependencies
    code.push('\n');
    for task in &dag.tasks {
        for dep in &task.dependencies {
            code.push_str(&format!("    {} >> {}\n", dep, task.task_id));
        }
    }

    code
}

// ─── Migration Report ─────────────────────────────────────────

/// Generate a migration report.
pub fn generate_migration_report(results: &[MigrationResult]) -> String {
    let mut report = String::new();
    report.push_str("# Vortex Migration Report\n\n");
    report.push_str(&format!("Generated: {}\n\n", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));

    let total_parsed: usize = results.iter().map(|r| r.jobs_parsed).sum();
    let total_converted: usize = results.iter().map(|r| r.jobs_converted).sum();
    let total_warnings: usize = results.iter().map(|r| r.warnings.len()).sum();
    let total_errors: usize = results.iter().map(|r| r.errors.len()).sum();

    report.push_str("## Summary\n\n");
    report.push_str(&format!("| Metric | Count |\n|--------|-------|\n"));
    report.push_str(&format!("| Files processed | {} |\n", results.len()));
    report.push_str(&format!("| Jobs parsed | {} |\n", total_parsed));
    report.push_str(&format!("| Jobs converted | {} |\n", total_converted));
    report.push_str(&format!("| Warnings | {} |\n", total_warnings));
    report.push_str(&format!("| Errors | {} |\n\n", total_errors));

    for result in results {
        report.push_str(&format!("## {} (from {:?})\n\n", result.vortex_dag.dag_id, result.source));
        report.push_str(&format!("- Jobs parsed: {}\n", result.jobs_parsed));
        report.push_str(&format!("- Jobs converted: {}\n", result.jobs_converted));

        if !result.warnings.is_empty() {
            report.push_str("\n### Warnings\n\n");
            for w in &result.warnings {
                report.push_str(&format!("- {}\n", w));
            }
        }

        if !result.errors.is_empty() {
            report.push_str("\n### Errors\n\n");
            for e in &result.errors {
                report.push_str(&format!("- {}\n", e));
            }
        }
        report.push('\n');
    }

    report
}

// ─── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_autosys_jil() {
        let jil = r#"
insert_job: ETL_EXTRACT  job_type: CMD
command: /opt/scripts/extract.sh
machine: prod-server-01
owner: etl_user
condition: s(DAILY_START)
description: Extract data from source
n_retrys: 3

insert_job: ETL_TRANSFORM  job_type: CMD
command: /opt/scripts/transform.sh
machine: prod-server-01
condition: s(ETL_EXTRACT)
"#;
        let jobs = parse_autosys_jil(jil, "test.jil").unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_name, "ETL_EXTRACT");
        assert_eq!(jobs[0].job_type, "cmd");
        assert_eq!(jobs[0].command.as_deref(), Some("/opt/scripts/extract.sh"));
        assert_eq!(jobs[0].dependencies, vec!["DAILY_START"]);
        assert_eq!(jobs[1].dependencies, vec!["ETL_EXTRACT"]);
    }

    #[test]
    fn test_parse_autosys_box() {
        let jil = r#"
insert_job: MY_BOX  job_type: BOX
permission: gx,ge

insert_job: TASK_A  job_type: CMD
box_name: MY_BOX
command: echo "task a"
"#;
        let jobs = parse_autosys_jil(jil, "box.jil").unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_type, "box");
    }

    #[test]
    fn test_parse_tws_definition() {
        let tws = r#"
# TWS Job Stream
SCHEDULE daily_etl
extract_job DOCOMMAND "/opt/scripts/extract.sh"
  DESCRIPTION "Extract daily data"
  AT 0600
  PRIORITY 10

transform_job DOCOMMAND "/opt/scripts/transform.sh"
  FOLLOWS extract_job
  DESCRIPTION "Transform extracted data"
"#;
        let jobs = parse_tws_definition(tws, "test.tws").unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_name, "extract_job");
        assert_eq!(jobs[0].command.as_deref(), Some("/opt/scripts/extract.sh"));
        assert_eq!(jobs[1].dependencies, vec!["extract_job"]);
    }

    #[test]
    fn test_parse_autosys_conditions() {
        assert_eq!(parse_autosys_conditions("s(job1) & s(job2)"), vec!["job1", "job2"]);
        assert_eq!(parse_autosys_conditions("s(single_job)"), vec!["single_job"]);
        assert_eq!(parse_autosys_conditions(""), Vec::<String>::new());
    }

    #[test]
    fn test_convert_to_vortex_dag() {
        let jobs = vec![
            MigrationJob {
                source: SourceScheduler::Autosys,
                job_name: "ETL_EXTRACT".to_string(),
                job_type: "cmd".to_string(),
                command: Some("/opt/scripts/extract.sh".to_string()),
                schedule: Some("days_of_week:mo,tu,we,th,fr".to_string()),
                dependencies: vec![],
                conditions: vec![],
                resources: HashMap::new(),
                notifications: vec![],
                properties: HashMap::new(),
            },
            MigrationJob {
                source: SourceScheduler::Autosys,
                job_name: "ETL_TRANSFORM".to_string(),
                job_type: "cmd".to_string(),
                command: Some("/opt/scripts/transform.sh".to_string()),
                schedule: None,
                dependencies: vec!["ETL_EXTRACT".to_string()],
                conditions: vec![],
                resources: HashMap::new(),
                notifications: vec![],
                properties: [("retries".to_string(), Value::String("2".to_string()))].into(),
            },
        ];

        let result = convert_to_vortex_dag(&jobs, "migrated_etl");
        assert_eq!(result.jobs_parsed, 2);
        assert_eq!(result.jobs_converted, 2);
        assert_eq!(result.vortex_dag.tasks.len(), 2);
        assert_eq!(result.vortex_dag.tasks[0].task_id, "etl_extract");
        assert_eq!(result.vortex_dag.tasks[0].operator, "ShellOperator");
        assert_eq!(result.vortex_dag.tasks[1].retries, 2);
    }

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("MY-JOB.NAME#1"), "my_job_name_1");
        assert_eq!(sanitize_id("simple"), "simple");
        assert_eq!(sanitize_id("CamelCase"), "camelcase");
    }

    #[test]
    fn test_generate_rust_dag_code() {
        let dag = VortexDagDef {
            dag_id: "test_dag".to_string(),
            description: "Test".to_string(),
            schedule: Some("0 8 * * *".to_string()),
            tasks: vec![VortexTaskDef {
                task_id: "task1".to_string(),
                operator: "ShellOperator".to_string(),
                config: serde_json::json!({"command": "echo hello"}),
                dependencies: vec![],
                retries: 0,
                retry_delay_secs: 60,
            }],
            default_args: HashMap::new(),
        };
        let code = generate_rust_dag_code(&dag);
        assert!(code.contains("test_dag"));
        assert!(code.contains("ShellOperator"));
        assert!(code.contains("echo hello"));
    }

    #[test]
    fn test_generate_python_dag_code() {
        let dag = VortexDagDef {
            dag_id: "python_dag".to_string(),
            description: "Python test".to_string(),
            schedule: None,
            tasks: vec![
                VortexTaskDef {
                    task_id: "extract".to_string(),
                    operator: "ShellOperator".to_string(),
                    config: serde_json::json!({"command": "echo extract"}),
                    dependencies: vec![],
                    retries: 0,
                    retry_delay_secs: 60,
                },
                VortexTaskDef {
                    task_id: "load".to_string(),
                    operator: "ShellOperator".to_string(),
                    config: serde_json::json!({"command": "echo load"}),
                    dependencies: vec!["extract".to_string()],
                    retries: 0,
                    retry_delay_secs: 60,
                },
            ],
            default_args: HashMap::new(),
        };
        let code = generate_python_dag_code(&dag);
        assert!(code.contains("from vortex import DAG"));
        assert!(code.contains("extract >> load"));
    }

    #[test]
    fn test_migration_report() {
        let results = vec![MigrationResult {
            source: SourceScheduler::Autosys,
            source_file: "test.jil".to_string(),
            jobs_parsed: 5,
            jobs_converted: 4,
            warnings: vec!["Some warning".to_string()],
            errors: vec!["Some error".to_string()],
            vortex_dag: VortexDagDef {
                dag_id: "migrated".to_string(),
                description: "Test".to_string(),
                schedule: None,
                tasks: vec![],
                default_args: HashMap::new(),
            },
        }];
        let report = generate_migration_report(&results);
        assert!(report.contains("Migration Report"));
        assert!(report.contains("Jobs parsed | 5"));
        assert!(report.contains("Some warning"));
    }

    #[test]
    fn test_unknown_job_type_conversion() {
        let jobs = vec![MigrationJob {
            source: SourceScheduler::Tws,
            job_name: "CUSTOM_JOB".to_string(),
            job_type: "custom_type".to_string(),
            command: Some("echo custom".to_string()),
            schedule: None,
            dependencies: vec![],
            conditions: vec![],
            resources: HashMap::new(),
            notifications: vec![],
            properties: HashMap::new(),
        }];
        let result = convert_to_vortex_dag(&jobs, "custom_dag");
        assert_eq!(result.jobs_converted, 1);
        assert!(!result.warnings.is_empty()); // Should warn about unknown type
    }
}
