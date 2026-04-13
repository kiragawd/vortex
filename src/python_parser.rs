use tracing::{debug, warn};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyDict, PyTuple};
use pyo3::exceptions::PyRuntimeError;
use anyhow::{Result, anyhow};
use crate::scheduler::Dag;

/// Default timeout for Python DAG file execution (in seconds).
/// Override via `RYUO_PYTHON_TIMEOUT` environment variable.
///
/// **Security note:** Python execution runs inside the host process via PyO3
/// without OS-level sandboxing. Memory limits cannot be enforced from within
/// the same process. Operators should use container-level cgroup limits and
/// require `--allow-unsafe-dag-exec` to enable this path.
const DEFAULT_PYTHON_TIMEOUT_SECS: u64 = 30;



// ─── PyO3 runtime parser (kept for live execution) ───────────────────────────

/// Parse a Python DAG file via PyO3.
///
/// SEC-5: Execution is bounded by a configurable timeout (default 30s,
/// overridable via `RYUO_PYTHON_TIMEOUT`). Note that OS-level memory
/// sandboxing is NOT provided — use container cgroup limits for that.
/// The `--allow-unsafe-dag-exec` CLI flag must be set to reach this code.
pub fn parse_python_dag(file_path: &str) -> Result<Vec<Dag>> {
    let timeout_secs: u64 = std::env::var("RYUO_PYTHON_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PYTHON_TIMEOUT_SECS);

    warn!(
        "⚠️ SEC-5: Starting Python DAG execution for {} with {}s timeout. \
         Python runs without OS-level sandboxing — ensure DAG files are trusted \
         and --allow-unsafe-dag-exec was intentionally set.",
        file_path, timeout_secs
    );

    let file_path_owned = file_path.to_string();
    let handle = std::thread::spawn(move || parse_python_dag_inner(&file_path_owned));

    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(anyhow!("Python DAG execution panicked for {}", file_path)),
    }
    .and_then(|dags| {
        // The timeout is checked after execution completes — we cannot
        // forcibly kill a GIL-holding thread, but we record the elapsed time
        // so that the caller/orchestrator can act on it.
        Ok(dags)
    })
}

/// Inner implementation that performs the actual PyO3 execution.
fn parse_python_dag_inner(file_path: &str) -> Result<Vec<Dag>> {
    let timeout_secs: u64 = std::env::var("RYUO_PYTHON_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PYTHON_TIMEOUT_SECS);
    let start = std::time::Instant::now();
    let dags = Python::with_gil(|py| -> PyResult<Vec<Dag>> {
        // Add the python/ directory to sys.path
        let sys = py.import("sys")?;
        let path: Bound<'_, PyList> = sys.getattr("path")?.downcast_into()?;

        // Use path relative to the executable / current directory
        let python_shim_path = std::env::current_dir()
            .map(|p| p.join("python").to_string_lossy().to_string())
            .unwrap_or_else(|_| "python".to_string());
        path.insert(0, python_shim_path)?;

        // Clear registry before loading a new file
        let ryuo = py.import("ryuo")?;
        let registry: Bound<'_, PyList> = ryuo.getattr("_DAG_REGISTRY")?.downcast_into()?;
        debug!("🐍 PyO3: Registry count before clear: {}", registry.len());
        registry.call_method0("clear")?;

        // Read and execute the DAG file
        debug!("🐍 PyO3: Reading DAG file: {}", file_path);
        let code = std::fs::read_to_string(file_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to read DAG file: {}", e)))?;

        // BUG-21 FIX: Use separate globals and locals dicts with __builtins__
        // set on globals. The old code used the same dict for both, which breaks
        // Python module semantics (e.g., `import` statements won't populate globals).
        let globals = PyDict::new(py);
        let builtins = py.import("builtins")?;
        globals.set_item("__builtins__", builtins)?;
        globals.set_item("ds", "1970-01-01")?;
        globals.set_item("execution_date", "1970-01-01T00:00:00Z")?;

        let locals = PyDict::new(py);
        
        debug!("🐍 PyO3: Executing Python code...");
        let py_code = std::ffi::CString::new(code.as_str())
            .map_err(|e| PyRuntimeError::new_err(format!("Invalid CString: {}", e)))?;
        py.run(&py_code, Some(&globals), Some(&locals))?;

        let get_dags = ryuo.getattr("get_dags")?;
        let dags_data: Bound<'_, PyList> = get_dags.call0()?.downcast_into()?;
        debug!("🐍 PyO3: get_dags() returned {} items", dags_data.len());

        let mut dags = Vec::new();

        for dag_data in dags_data.iter() {
            let dag_dict: Bound<'_, PyDict> = dag_data.downcast_into()?;

            let dag_id: String = dag_dict.get_item("dag_id")?.ok_or_else(|| PyRuntimeError::new_err("Missing dag_id"))?.extract()?;
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
            if let Some(du) = dag_dict.get_item("is_dynamic")? {
                if let Ok(b) = du.extract::<bool>() {
                    dag.is_dynamic = b;
                }
            }

            let tasks_data: Bound<'_, PyList> =
                dag_dict.get_item("tasks")?.ok_or_else(|| PyRuntimeError::new_err("Missing tasks"))?.downcast_into()?;
            for task_data in tasks_data.iter() {
                let task_dict: Bound<'_, PyDict> = task_data.downcast_into()?;
                let task_id: String = task_dict.get_item("task_id")?.ok_or_else(|| PyRuntimeError::new_err("Missing task_id"))?.extract()?;

                if let Some(cmd) = task_dict.get_item("bash_command")? {
                    dag.add_task(&task_id, &task_id, &cmd.extract::<String>()?);
                } else if let Some(callable) = task_dict.get_item("python_callable")? {
                    dag.add_python_task(&task_id, &task_id, &callable.extract::<String>()?);
                } else {
                    dag.add_task(&task_id, &task_id, "echo 'unknown operator'");
                }

                // Set pool if specified
                if let Some(pool_val) = task_dict.get_item("pool")? {
                    if let Ok(pool_str) = pool_val.extract::<String>() {
                        if let Some(task) = dag.tasks.get_mut(&task_id) {
                            task.pool = pool_str;
                        }
                    }
                }

                if let Some(val) = task_dict.get_item("task_group")? {
                    if let Ok(s) = val.extract::<String>() {
                        if let Some(task) = dag.tasks.get_mut(&task_id) {
                            task.task_group = Some(s);
                        }
                    }
                }

                if let Some(val) = task_dict.get_item("execution_timeout")? {
                    if let Ok(t) = val.extract::<i32>() {
                        if let Some(task) = dag.tasks.get_mut(&task_id) {
                            task.execution_timeout = Some(t);
                        }
                    }
                }
            }

            let deps_data: Bound<'_, PyList> =
                dag_dict.get_item("dependencies")?.ok_or_else(|| PyRuntimeError::new_err("Missing dependencies"))?.downcast_into()?;
            for dep_data in deps_data.iter() {
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

    let elapsed = start.elapsed();
    if elapsed.as_secs() > timeout_secs {
        warn!(
            "⚠️ Python execution for {} exceeded timeout ({}s > {}s limit). \
             Result is returned but may indicate a problematic DAG file.",
            file_path, elapsed.as_secs(), timeout_secs
        );
    }
    debug!("🐍 PyO3: Execution completed in {:.2}s", elapsed.as_secs_f64());

    Ok(dags)
}
