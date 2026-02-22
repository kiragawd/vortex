use pyo3::prelude::*;
use pyo3::types::{PyList, PyDict, PyTuple};
use pyo3::exceptions::PyRuntimeError;
use anyhow::{Result, anyhow};
use crate::scheduler::Dag;
use std::ffi::CString;

pub fn parse_python_dag(file_path: &str) -> Result<Vec<Dag>> {
    let dags = Python::with_gil(|py| -> PyResult<Vec<Dag>> {
        // Add the python/ directory to sys.path
        let sys = py.import("sys")?;
        let path: Bound<'_, PyList> = sys.getattr("path")?.downcast_into()?;
        
        // Use absolute path for the python shim
        let python_shim_path = "/Users/ashwin/vortex/python";
        path.insert(0, python_shim_path)?;

        // Read and execute the DAG file
        let code = std::fs::read_to_string(file_path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to read DAG file: {}", e)))?;
        
        let locals = PyDict::new(py);
        let py_code = CString::new(code)
            .map_err(|e| PyRuntimeError::new_err(format!("NulError: {}", e)))?;
        py.run(&py_code, None, Some(&locals))?;

        // Import vortex and get dags
        let vortex = py.import("vortex")?;
        let get_dags = vortex.getattr("get_dags")?;
        let dags_data: Bound<'_, PyList> = get_dags.call0()?.downcast_into()?;

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

            let tasks_data: Bound<'_, PyList> = dag_dict.get_item("tasks")?.unwrap().downcast_into()?;
            for task_data in tasks_data {
                let task_dict: Bound<'_, PyDict> = task_data.downcast_into()?;
                let task_id: String = task_dict.get_item("task_id")?.unwrap().extract()?;
                
                // For now, we only support BashOperator or treat others as simple commands
                let command = if let Some(cmd) = task_dict.get_item("bash_command")? {
                    cmd.extract()?
                } else if let Some(callable) = task_dict.get_item("python_callable")? {
                    format!("echo 'Executing Python callable: {}'", callable.extract::<String>()?)
                } else {
                    "echo 'unknown operator'".to_string()
                };

                dag.add_task(&task_id, &task_id, &command);
            }

            let deps_data: Bound<'_, PyList> = dag_dict.get_item("dependencies")?.unwrap().downcast_into()?;
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
