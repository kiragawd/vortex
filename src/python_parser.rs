use tracing::debug;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyDict, PyTuple};
use pyo3::exceptions::PyRuntimeError;
use anyhow::{Result, anyhow};
use crate::scheduler::Dag;
use std::ffi::CString;



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
        locals.set_item("ds", "1970-01-01")?;
        locals.set_item("execution_date", "1970-01-01T00:00:00Z")?;
        
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
            if let Some(du) = dag_dict.get_item("is_dynamic")? {
                if let Ok(b) = du.extract::<bool>() {
                    dag.is_dynamic = b;
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

                // Phase 2: Set pool if specified
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
