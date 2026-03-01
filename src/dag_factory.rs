use crate::scheduler::Dag;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct DagConfig {
    pub id: String,
    pub schedule_interval: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_max_active_runs")]
    pub max_active_runs: i32,
    #[serde(default)]
    pub catchup: bool,
    pub sla_seconds: Option<u64>,
    pub tasks: Vec<TaskConfig>,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

fn default_max_active_runs() -> i32 {
    1
}

#[derive(Debug, Deserialize)]
pub struct TaskConfig {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub task_type: String,
    pub command: Option<String>,
    pub code: Option<String>,
    pub sensor_config: Option<serde_json::Value>,
    #[serde(default)]
    pub dependencies: Vec<String>, // Upstream dependencies
    pub task_group: Option<String>,
}

pub fn parse_dag_file<P: AsRef<Path>>(path: P) -> Result<Vec<Dag>> {
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read DAG file: {:?}", path.as_ref()))?;
    
    let path_str = path.as_ref().to_string_lossy().to_string();
    let config: DagConfig = if path_str.ends_with(".json") {
        serde_json::from_str(&content).context("Failed to parse JSON")?
    } else if path_str.ends_with(".yaml") || path_str.ends_with(".yml") {
        serde_yaml::from_str(&content).context("Failed to parse YAML")?
    } else {
        anyhow::bail!("Unsupported file extension");
    };

    let mut dag = Dag::new(&config.id);
    if let Some(schedule) = config.schedule_interval {
        dag.set_schedule(&schedule);
    }
    dag.timezone = config.timezone;
    dag.max_active_runs = config.max_active_runs;
    dag.catchup = config.catchup;
    dag.is_dynamic = false;
    dag.sla_seconds = config.sla_seconds;

    for task in config.tasks {
        let name = task.name.unwrap_or_else(|| task.id.clone());
        let mut t = match task.task_type.as_str() {
            "bash" => {
                let cmd = task.command.unwrap_or_default();
                dag.add_task(&task.id, &name, &cmd);
                dag.tasks.get_mut(&task.id).unwrap()
            },
            "python" => {
                let code = task.code.unwrap_or_else(|| task.command.clone().unwrap_or_default());
                dag.add_python_task(&task.id, &name, &code);
                dag.tasks.get_mut(&task.id).unwrap()
            },
            "sensor" => {
                let cfg = task.sensor_config.unwrap_or_else(|| serde_json::json!({}));
                dag.add_sensor_task(&task.id, &name, cfg);
                dag.tasks.get_mut(&task.id).unwrap()
            },
            _ => {
                let cmd = task.command.unwrap_or_default();
                dag.add_task(&task.id, &name, &cmd);
                dag.tasks.get_mut(&task.id).unwrap()
            }
        };
        t.task_group = task.task_group.clone();
        
        for up in task.dependencies {
            dag.add_dependency(&up, &task.id);
        }
    }

    Ok(vec![dag])
}
