use tracing::debug;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyDict, PyTuple};
use pyo3::exceptions::PyRuntimeError;
use anyhow::{Result, anyhow};
use crate::scheduler::Dag;
use std::ffi::CString;
use std::collections::{HashMap, HashSet};
use regex::Regex;
use serde::{Deserialize, Serialize};

// ─── Structs for parsed DAG ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VortexTask {
    pub task_id: String,
    pub task_type: String, // "bash", "python", "noop"
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VortexDAG {
    pub dag_id: String,
    pub description: Option<String>,
    pub schedule_interval: Option<String>,
    pub owner: Option<String>,
    pub tasks: Vec<VortexTask>,
    pub edges: Vec<(String, String)>, // (upstream, downstream)
}

// ─── Regex-based parser ──────────────────────────────────────────────────────

/// Detect whether a Python DAG file was authored with the Airflow API surface.
///
/// Returns `true` when the file contains either:
///   - `from airflow import DAG`  (native Airflow)
///   - `from vortex import DAG`   (VORTEX Airflow shim)
///   - `from airflow.models import DAG`
///
/// This is used by the regex parser to decide whether to accept the file.
pub fn is_airflow_compatible_dag(content: &str) -> bool {
    let shim_re = Regex::new(
        r#"from\s+(airflow(?:\.[\w\.]+)?|vortex)\s+import\s+[^\n]*\bDAG\b"#,
    )
    .unwrap();
    shim_re.is_match(content)
}

/// Parse a raw Python DAG file using regex (no Python runtime required).
/// Returns a `VortexDAG` or an error.
///
/// Accepts files that define a DAG via:
///   - Native VORTEX syntax
///   - `from airflow import DAG` (Airflow native)
///   - `from vortex import DAG`  (VORTEX Airflow-compatibility shim)
pub fn parse_dag_file(content: &str) -> Result<VortexDAG> {
    if content.trim().is_empty() {
        return Err(anyhow!("DAG file is empty or malformed"));
    }

    // Phase 2.3: Check for Airflow/VORTEX shim imports before proceeding
    if !is_airflow_compatible_dag(content) {
        return Err(anyhow!("File is not a valid VORTEX/Airflow DAG (missing imports)"));
    }

    // --- dag_id ----------------------------------------------------------
    // Match: DAG("my_dag"  or  DAG('my_dag'  or  dag_id="my_dag"
    let dag_id_re = Regex::new(
        r#"(?:DAG\s*\(\s*['"]([\w\-]+)['"]|dag_id\s*=\s*['"]([\w\-]+)['"])"#,
    )
    .unwrap();

    let dag_id = dag_id_re
        .captures(content)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| anyhow!("Could not extract dag_id from DAG file"))?;

    // --- schedule_interval -----------------------------------------------
    let schedule_re = Regex::new(
        r#"schedule_interval\s*=\s*['"]([\w@\s\*/\-]+)['"]"#,
    )
    .unwrap();
    let schedule_interval = schedule_re
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string());

    // --- description / owner (optional metadata) -------------------------
    let description_re = Regex::new(
        r#"description\s*=\s*['"](.*?)['"]"#,
    )
    .unwrap();
    let description = description_re
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let owner_re = Regex::new(r#"owner\s*=\s*['"](.*?)['"]"#).unwrap();
    let owner = owner_re
        .captures(content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    // --- Tasks -----------------------------------------------------------
    let mut tasks: Vec<VortexTask> = Vec::new();

    // BashOperator
    let bash_block_re = Regex::new(
        r#"BashOperator\s*\(([\s\S]*?)\)"#,
    )
    .unwrap();
    let task_id_re = Regex::new(r#"task_id\s*=\s*['"]([\w\-]+)['"]"#).unwrap();
    let bash_cmd_re = Regex::new(r#"bash_command\s*=\s*['"](.*?)['"]"#).unwrap();

    for cap in bash_block_re.captures_iter(content) {
        let block = &cap[1];
        if let Some(tid) = task_id_re.captures(block).and_then(|c| c.get(1)) {
            let bash_command = bash_cmd_re
                .captures(block)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            tasks.push(VortexTask {
                task_id: tid.as_str().to_string(),
                task_type: "bash".to_string(),
                config: serde_json::json!({ "bash_command": bash_command }),
            });
        }
    }

    // PythonOperator
    let py_block_re = Regex::new(
        r#"PythonOperator\s*\(([\s\S]*?)\)"#,
    )
    .unwrap();
    let callable_re = Regex::new(r#"python_callable\s*=\s*([\w]+)"#).unwrap();

    for cap in py_block_re.captures_iter(content) {
        let block = &cap[1];
        if let Some(tid) = task_id_re.captures(block).and_then(|c| c.get(1)) {
            let callable = callable_re
                .captures(block)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            tasks.push(VortexTask {
                task_id: tid.as_str().to_string(),
                task_type: "python".to_string(),
                config: serde_json::json!({ "python_callable": callable }),
            });
        }
    }

    // DummyOperator
    let dummy_block_re = Regex::new(
        r#"DummyOperator\s*\(([\s\S]*?)\)"#,
    )
    .unwrap();

    for cap in dummy_block_re.captures_iter(content) {
        let block = &cap[1];
        if let Some(tid) = task_id_re.captures(block).and_then(|c| c.get(1)) {
            tasks.push(VortexTask {
                task_id: tid.as_str().to_string(),
                task_type: "noop".to_string(),
                config: serde_json::json!({}),
            });
        }
    }

    if tasks.is_empty() {
        // A file with no recognisable operators might still be a valid (stub) DAG
        // but we'll let the caller decide. Just return zero tasks.
    }

    // --- Edges (>>) ------------------------------------------------------
    // Matches lines like:  t1 >> t2   or  t1 >> t2 >> t3
    let edge_line_re = Regex::new(r#"([\w]+(?:\s*>>\s*[\w]+)+)"#).unwrap();
    let word_re = Regex::new(r#"[\w]+"#).unwrap();

    let mut edges: Vec<(String, String)> = Vec::new();

    for cap in edge_line_re.captures_iter(content) {
        let chain = &cap[1];
        let words: Vec<&str> = word_re
            .find_iter(chain)
            .map(|m| m.as_str())
            .collect();
        for pair in words.windows(2) {
            edges.push((pair[0].to_string(), pair[1].to_string()));
        }
    }

    // Deduplicate edges
    let mut seen = HashSet::new();
    edges.retain(|e| seen.insert(e.clone()));

    // --- Cycle detection -------------------------------------------------
    detect_cycles(&tasks, &edges)?;

    Ok(VortexDAG {
        dag_id,
        description,
        schedule_interval,
        owner,
        tasks,
        edges,
    })
}

/// Serialize a `VortexDAG` to a pretty-printed JSON string.
pub fn dag_to_json(dag: &VortexDAG) -> Result<String> {
    serde_json::to_string_pretty(dag).map_err(|e| anyhow!("JSON serialization error: {}", e))
}

/// DFS-based cycle detector. Returns an error if the edge list contains a cycle.
fn detect_cycles(tasks: &[VortexTask], edges: &[(String, String)]) -> Result<()> {
    // Build adjacency list from known task IDs
    let ids: HashSet<String> = tasks.iter().map(|t| t.task_id.clone()).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for id in &ids {
        adj.insert(id.clone(), Vec::new());
    }
    for (up, down) in edges {
        adj.entry(up.clone()).or_default().push(down.clone());
    }

    // Track DFS state: 0 = unvisited, 1 = in-stack, 2 = done
    let mut state: HashMap<String, u8> = ids.iter().map(|id| (id.clone(), 0u8)).collect();

    fn dfs(
        node: &str,
        adj: &HashMap<String, Vec<String>>,
        state: &mut HashMap<String, u8>,
    ) -> bool {
        if state.get(node).copied() == Some(1) {
            return true; // cycle!
        }
        if state.get(node).copied() == Some(2) {
            return false;
        }
        state.insert(node.to_string(), 1);
        if let Some(neighbors) = adj.get(node) {
            for nb in neighbors.clone() {
                if dfs(&nb, adj, state) {
                    return true;
                }
            }
        }
        state.insert(node.to_string(), 2);
        false
    }

    let nodes: Vec<String> = ids.into_iter().collect();
    for node in &nodes {
        if state.get(node).copied() == Some(0) && dfs(node, &adj, &mut state) {
            return Err(anyhow!("Cyclic dependency detected in DAG"));
        }
    }

    Ok(())
}

// ─── PyO3 runtime parser (kept for live execution) ───────────────────────────

pub fn parse_python_dag(file_path: &str) -> Result<Vec<Dag>> {
    let dags = Python::with_gil(|py| -> PyResult<Vec<Dag>> {
        // Add the python/ directory to sys.path
        let sys = py.import("sys")?;
        let path: Bound<'_, PyList> = sys.getattr("path")?.downcast_into()?;

        // Use path relative to the executable / current directory
        let python_shim_path = std::env::current_dir()
            .map(|p| p.join("python").to_string_lossy().to_string())
            .unwrap_or_else(|_| "python".to_string());
        path.insert(0, python_shim_path)?;

        // Phase 2.4: Clear registry before loading a new file
        let vortex = py.import("vortex")?;
        let registry: Bound<'_, PyList> = vortex.getattr("_DAG_REGISTRY")?.downcast_into()?;
        debug!("🐍 PyO3: Registry count before clear: {}", registry.len());
        registry.call_method0("clear")?;

        // Read and execute the DAG file
        debug!("🐍 PyO3: Reading DAG file: {}", file_path);
        let code = std::fs::read_to_string(file_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to read DAG file: {}", e)))?;

        let locals = PyDict::new(py);
        let py_code = CString::new(code)
            .map_err(|e| PyRuntimeError::new_err(format!("NulError: {}", e)))?;
        
        debug!("🐍 PyO3: Executing Python code...");
        // Use the same dict for globals and locals to support typical script behavior
        py.run(&py_code, Some(&locals), Some(&locals))?;

        let get_dags = vortex.getattr("get_dags")?;
        let dags_data: Bound<'_, PyList> = get_dags.call0()?.downcast_into()?;
        debug!("🐍 PyO3: get_dags() returned {} items", dags_data.len());

        let mut dags = Vec::new();

        for dag_data in dags_data {
            let dag_dict: Bound<'_, PyDict> = dag_data.downcast_into()?;

            let dag_id: String = dag_dict.get_item("dag_id")?.unwrap().extract()?;
            let mut dag = Dag::new(&dag_id);

            if let Some(schedule) = dag_dict.get_item("schedule_interval")? {
                if let Ok(s) = schedule.extract::<String>() {
                    dag.set_schedule(&s);
                }
            }

            // Extract Chronos v2 fields
            if let Some(tz) = dag_dict.get_item("timezone")? {
                if let Ok(s) = tz.extract::<String>() {
                    dag.timezone = s;
                }
            }

            if let Some(mar) = dag_dict.get_item("max_active_runs")? {
                if let Ok(n) = mar.extract::<i32>() {
                    dag.max_active_runs = n;
                }
            }

            if let Some(cu) = dag_dict.get_item("catchup")? {
                if let Ok(b) = cu.extract::<bool>() {
                    dag.catchup = b;
                }
            }

            let tasks_data: Bound<'_, PyList> =
                dag_dict.get_item("tasks")?.unwrap().downcast_into()?;
            for task_data in tasks_data {
                let task_dict: Bound<'_, PyDict> = task_data.downcast_into()?;
                let task_id: String = task_dict.get_item("task_id")?.unwrap().extract()?;

                if let Some(cmd) = task_dict.get_item("bash_command")? {
                    dag.add_task(&task_id, &task_id, &cmd.extract::<String>()?);
                } else if let Some(callable) = task_dict.get_item("python_callable")? {
                    dag.add_python_task(&task_id, &task_id, &callable.extract::<String>()?);
                } else {
                    dag.add_task(&task_id, &task_id, "echo 'unknown operator'");
                };
            }

            let deps_data: Bound<'_, PyList> =
                dag_dict.get_item("dependencies")?.unwrap().downcast_into()?;
            for dep_data in deps_data {
                let dep_tuple: Bound<'_, PyTuple> = dep_data.downcast_into()?;
                let upstream: String = dep_tuple.get_item(0)?.extract()?;
                let downstream: String = dep_tuple.get_item(1)?.extract()?;
                dag.add_dependency(&upstream, &downstream);
            }

            dags.push(dag);
        }

        Ok(dags)
    })
    .map_err(|e: PyErr| anyhow!("Python error: {}", e))?;

    Ok(dags)
}
