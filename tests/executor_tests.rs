use std::collections::HashMap;
use vortex::executor::{TaskExecutor, ExecutionResult};

#[tokio::test]
async fn test_execute_bash_success() {
    let result = TaskExecutor::execute_bash("test_bash_1", "echo hello", HashMap::new(), None).await;
    assert!(result.success);
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
}

#[tokio::test]
async fn test_execute_bash_env_vars() {
    let mut env_vars = HashMap::new();
    env_vars.insert("VORTEX_VAR".to_string(), "power".to_string());
    let result = TaskExecutor::execute_bash("test_bash_2", "echo $VORTEX_VAR", env_vars, None).await;
    assert!(result.success);
    assert_eq!(result.stdout.trim(), "power");
}

#[tokio::test]
async fn test_execute_bash_fail() {
    let result = TaskExecutor::execute_bash("test_bash_3", "ls /non_existent_directory_vortex", HashMap::new(), None).await;
    assert!(!result.success);
    assert_ne!(result.exit_code, 0);
}

/// TEST-3 fix: Validates real timeout behavior by using a short timeout (2s)
/// against a long-running command (sleep 10). Verifies the task is killed
/// and returns the correct timeout error indicators.
#[tokio::test]
async fn test_execute_bash_timeout() {
    let start = std::time::Instant::now();
    let result = TaskExecutor::execute_bash(
        "test_bash_4",
        "sleep 10",
        HashMap::new(),
        Some(2), // 2-second timeout against a 10-second command
    )
    .await;

    // Must not succeed — the task should be killed by timeout
    assert!(!result.success, "Timed-out task must not report success");
    assert_eq!(result.exit_code, -2, "Exit code must be -2 for timeout");
    assert!(
        result.stderr.contains("timed out"),
        "Stderr must mention timeout, got: {}",
        result.stderr,
    );
    // Ensure we didn't wait the full 10s — the timeout must have fired early
    assert!(
        start.elapsed().as_secs() < 5,
        "Timeout should fire well before the command finishes",
    );
}

#[tokio::test]
async fn test_execute_python_success() {
    let result = TaskExecutor::execute_python("test_py_1", "print('hello')", HashMap::new(), None).await;
    assert!(result.success);
    assert_eq!(result.stdout.trim(), "hello");
}

#[tokio::test]
async fn test_execute_python_env_vars() {
    let mut env_vars = HashMap::new();
    env_vars.insert("PY_VAR".to_string(), "vortex_python".to_string());
    let result = TaskExecutor::execute_python("test_py_2", "import os; print(os.environ['PY_VAR'])", env_vars, None).await;
    assert!(result.success);
    assert_eq!(result.stdout.trim(), "vortex_python");
}

#[tokio::test]
async fn test_execute_python_exception() {
    let result = TaskExecutor::execute_python("test_py_3", "raise Exception('boom')", HashMap::new(), None).await;
    assert!(!result.success);
    assert!(result.stderr.contains("Exception: boom"));
}

#[tokio::test]
async fn test_execute_python_multi_print() {
    let result = TaskExecutor::execute_python("test_py_4", "print('line1')\nprint('line2')", HashMap::new(), None).await;
    assert!(result.success);
    assert_eq!(result.stdout, "line1\nline2\n");
}
