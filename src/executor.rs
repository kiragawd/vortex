use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use std::time::Instant;
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExecutionResult {
    pub task_id: String,
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[allow(dead_code)]
pub struct TaskExecutor;

impl TaskExecutor {
    pub async fn execute_bash(
        task_id: &str,
        bash_command: &str,
        env_vars: HashMap<String, String>,
    ) -> ExecutionResult {
        let start = Instant::now();
        
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(bash_command)
            .envs(env_vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let result = timeout(Duration::from_secs(300), cmd.output()).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let success = output.status.success();
                let exit_code = output.status.code().unwrap_or(if success { 0 } else { 1 });
                ExecutionResult {
                    task_id: task_id.to_string(),
                    success,
                    exit_code,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    duration_ms,
                }
            }
            Ok(Err(e)) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to execute command: {}", e),
                duration_ms,
            },
            Err(_) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -2,
                stdout: String::new(),
                stderr: "Task timed out after 300 seconds".to_string(),
                duration_ms,
            },
        }
    }

    pub async fn execute_python(
        task_id: &str,
        python_code: &str,
        env_vars: HashMap<String, String>,
    ) -> ExecutionResult {
        let start = Instant::now();

        // Create a temporary file for the Python code
        let temp_file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(e) => {
                return ExecutionResult {
                    task_id: task_id.to_string(),
                    success: false,
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Failed to create temp file: {}", e),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        let temp_path = temp_file.path().to_path_buf();
        if let Err(e) = std::fs::write(&temp_path, python_code) {
            return ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to write to temp file: {}", e),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }

        let mut cmd = Command::new("python3");
        cmd.arg(&temp_path)
            .envs(env_vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let result = timeout(Duration::from_secs(300), cmd.output()).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // NamedTempFile will be deleted when it goes out of scope here
        
        match result {
            Ok(Ok(output)) => {
                let success = output.status.success();
                let exit_code = output.status.code().unwrap_or(if success { 0 } else { 1 });
                ExecutionResult {
                    task_id: task_id.to_string(),
                    success,
                    exit_code,
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    duration_ms,
                }
            }
            Ok(Err(e)) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to execute python: {}", e),
                duration_ms,
            },
            Err(_) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -2,
                stdout: String::new(),
                stderr: "Task timed out after 300 seconds".to_string(),
                duration_ms,
            },
        }
    }
}
