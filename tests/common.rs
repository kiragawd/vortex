/// Common test utilities for VORTEX tests
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::NamedTempFile;

/// Create a temporary test database
pub fn create_test_db() -> (PathBuf, NamedTempFile) {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let db_path = temp_file.path().to_path_buf();
    (db_path, temp_file)
}

/// Test database path for isolated tests
pub fn get_test_db_path(test_name: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("vortex_test_{}.db", test_name));
    path
}

/// Cleanup test database file
pub fn cleanup_test_db(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

/// Generate a random test worker ID
pub fn generate_test_worker_id() -> String {
    use uuid::Uuid;
    format!("test-worker-{}", Uuid::new_v4())
}

/// Generate a random test DAG ID
pub fn generate_test_dag_id() -> String {
    use uuid::Uuid;
    format!("test-dag-{}", Uuid::new_v4())
}

/// Generate a random test task ID
pub fn generate_test_task_id() -> String {
    use uuid::Uuid;
    format!("test-task-{}", Uuid::new_v4())
}

/// Generate a random test task instance ID
pub fn generate_test_task_instance_id() -> String {
    use uuid::Uuid;
    format!("test-ti-{}", Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_generation() {
        let worker_id = generate_test_worker_id();
        assert!(worker_id.contains("test-worker-"));
        
        let dag_id = generate_test_dag_id();
        assert!(dag_id.contains("test-dag-"));
    }
}
