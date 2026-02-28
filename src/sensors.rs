
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::db_trait::DatabaseBackend;

// ─── Config & Result Types ────────────────────────────────────────────────────

/// Configuration for a sensor task, serialized from the task's `config` JSON field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorConfig {
    /// Sensor kind: "file", "http", "external_task", or "sql"
    pub sensor_type: String,

    /// How long to wait between pokes (seconds). Default: 30.
    #[serde(default = "default_poke_interval")]
    pub poke_interval_secs: i32,

    /// Maximum time to wait before timing out (seconds). Default: 3600.
    #[serde(default = "default_timeout")]
    pub timeout_secs: i32,

    /// Execution mode: "poke" (hold worker) or "reschedule" (release worker between pokes).
    #[serde(default = "default_mode")]
    pub mode: String,

    /// Sensor-specific configuration (filepath, url, dag_id, etc.)
    #[serde(default)]
    pub config: serde_json::Value,
}

fn default_poke_interval() -> i32 {
    30
}

fn default_timeout() -> i32 {
    3600
}

fn default_mode() -> String {
    "poke".to_string()
}

/// Outcome of a single sensor evaluation or a full sensor loop run.
#[derive(Debug, Clone, PartialEq)]
pub enum SensorResult {
    /// The awaited condition is now true — sensor succeeded.
    ConditionMet,
    /// The condition is not yet true — keep waiting.
    Waiting,
    /// The sensor exceeded its `timeout_secs` without the condition being met.
    TimedOut,
    /// An unrecoverable error occurred during evaluation.
    Failed(String),
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Evaluate the sensor condition once. Returns `ConditionMet`, `Waiting`, or `Failed`.
///
/// This is the main dispatch function called by `run_sensor_loop` on each poke.
pub async fn evaluate_sensor(config: &SensorConfig) -> SensorResult {
    debug!(
        "🔍 Evaluating sensor type='{}' mode='{}'",
        config.sensor_type, config.mode
    );

    match config.sensor_type.as_str() {
        "file" => {
            let path = config
                .config
                .get("filepath")
                .or_else(|| config.config.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if path.is_empty() {
                return SensorResult::Failed("FileSensor: missing 'filepath' in config".into());
            }

            if check_file_sensor(path).await {
                info!("📂 FileSensor: file found at '{}'", path);
                SensorResult::ConditionMet
            } else {
                debug!("📂 FileSensor: file not yet present at '{}'", path);
                SensorResult::Waiting
            }
        }

        "http" => {
            let url = config
                .config
                .get("endpoint")
                .or_else(|| config.config.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if url.is_empty() {
                return SensorResult::Failed("HttpSensor: missing 'endpoint' in config".into());
            }

            let expected_status = config
                .config
                .get("expected_status")
                .and_then(|v| v.as_u64())
                .unwrap_or(200) as u16;

            if check_http_sensor(url, expected_status).await {
                info!("🌐 HttpSensor: endpoint '{}' returned {}", url, expected_status);
                SensorResult::ConditionMet
            } else {
                debug!(
                    "🌐 HttpSensor: endpoint '{}' did not return {} yet",
                    url, expected_status
                );
                SensorResult::Waiting
            }
        }

        "external_task" => {
            // external_task sensor requires a Db handle — callers that only use
            // `evaluate_sensor` (without a Db) will see a graceful failure here.
            SensorResult::Failed(
                "ExternalTaskSensor: use run_sensor_loop (requires Db handle)".into(),
            )
        }

        "sql" => {
            let connection_string = config
                .config
                .get("conn_id")
                .or_else(|| config.config.get("connection_string"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let query = config
                .config
                .get("sql")
                .or_else(|| config.config.get("query"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if connection_string.is_empty() || query.is_empty() {
                return SensorResult::Failed(
                    "SqlSensor: missing 'conn_id' or 'sql' in config".into(),
                );
            }

            if check_sql_sensor(connection_string, query).await {
                info!("🗄️  SqlSensor: query returned rows");
                SensorResult::ConditionMet
            } else {
                debug!("🗄️  SqlSensor: query returned no rows yet");
                SensorResult::Waiting
            }
        }

        other => SensorResult::Failed(format!("Unknown sensor_type: '{}'", other)),
    }
}

// ─── Individual Sensor Checks ─────────────────────────────────────────────────

/// Returns `true` when a file exists at `path`.
pub async fn check_file_sensor(path: &str) -> bool {
    tokio::fs::metadata(path).await.is_ok()
}

/// Returns `true` when an HTTP GET to `url` returns `expected_status`.
///
/// Uses `curl` via `tokio::process::Command` to avoid adding reqwest as a dependency.
pub async fn check_http_sensor(url: &str, expected_status: u16) -> bool {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--output",
            "/dev/null",
            "--write-out",
            "%{http_code}",
            "--max-time",
            "10",
            "--location",
            url,
        ])
        .output()
        .await;

    match output {
        Ok(out) => {
            let status_str = String::from_utf8_lossy(&out.stdout);
            let status_code: u16 = status_str.trim().parse().unwrap_or(0);
            debug!(
                "🌐 curl {} → HTTP {}  (want {})",
                url, status_code, expected_status
            );
            status_code == expected_status
        }
        Err(e) => {
            warn!("🌐 HttpSensor: curl failed for '{}': {}", url, e);
            false
        }
    }
}

/// Returns `true` when a task in another DAG has state "Success" in `task_instances`.
pub async fn check_external_task_sensor(
    db: &Arc<dyn DatabaseBackend>,
    dag_id: &str,
    task_id: &str,
) -> bool {
    // Query task_instances for the latest instance of the target task using the trait.
    match db.get_task_instances(dag_id).await {
        Ok(instances) => {
            // Find the most recent instance for the given task_id
            let latest = instances.iter()
                .filter(|ti| ti["task_id"].as_str() == Some(task_id))
                .last();

            match latest {
                Some(ti) => {
                    let state = ti["state"].as_str().unwrap_or("");
                    debug!(
                        "🔗 ExternalTaskSensor: {}/{} → state='{}'",
                        dag_id, task_id, state
                    );
                    state == "Success"
                }
                None => {
                    debug!(
                        "🔗 ExternalTaskSensor: no rows found for {}/{}",
                        dag_id, task_id
                    );
                    false
                }
            }
        }
        Err(e) => {
            warn!(
                "🔗 ExternalTaskSensor: DB error querying {}/{}: {}",
                dag_id, task_id, e
            );
            false
        }
    }
}

/// Returns `true` when the SQL `query` returns at least one row.
///
/// Uses `sqlite3` via `Command` to avoid adding a new async SQL dependency.
/// `connection_string` should be a path to the SQLite database file.
pub async fn check_sql_sensor(connection_string: &str, query: &str) -> bool {
    // Append a LIMIT to avoid fetching unbounded rows — we only care whether
    // any row exists at all.
    let bounded_query = format!("SELECT COUNT(*) FROM ({}) AS _vortex_count LIMIT 1;", query);

    let output = Command::new("sqlite3")
        .arg(connection_string)
        .arg(&bounded_query)
        .output()
        .await;

    match output {
        Ok(out) => {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!("🗄️  SqlSensor: sqlite3 error: {}", stderr.trim());
                return false;
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            let count: i64 = stdout.trim().parse().unwrap_or(0);
            debug!("🗄️  SqlSensor: row count = {}", count);
            count > 0
        }
        Err(e) => {
            warn!("🗄️  SqlSensor: failed to spawn sqlite3: {}", e);
            false
        }
    }
}

// ─── Sensor Loop ─────────────────────────────────────────────────────────────

/// Poll the sensor condition repeatedly until it is met, the timeout is reached,
/// or an unrecoverable error occurs.
///
/// In `"poke"` mode the worker is held while sleeping between pokes.
/// In `"reschedule"` mode the semantic is the same here (the rescheduling itself
/// must be handled at the scheduler layer), but a log message indicates intent.
///
/// # Arguments
/// * `config` — sensor configuration including type, intervals, and sensor-specific params
/// * `db`     — shared database handle (required for `external_task` sensor)
pub async fn run_sensor_loop(config: &SensorConfig, db: &Arc<dyn DatabaseBackend>) -> SensorResult {
    let poke_interval = Duration::from_secs(config.poke_interval_secs.max(1) as u64);
    let timeout_duration = Duration::from_secs(config.timeout_secs.max(1) as u64);
    let deadline = Instant::now() + timeout_duration;

    info!(
        "⏱️  Sensor loop started: type='{}' mode='{}' poke={}s timeout={}s",
        config.sensor_type, config.mode, config.poke_interval_secs, config.timeout_secs
    );

    if config.mode == "reschedule" {
        info!(
            "♻️  Sensor mode='reschedule': worker will signal reschedule between pokes \
             (scheduler integration required)"
        );
    }

    loop {
        // Check deadline before poking.
        if Instant::now() >= deadline {
            warn!(
                "⏰ Sensor timed out after {}s: type='{}'",
                config.timeout_secs, config.sensor_type
            );
            return SensorResult::TimedOut;
        }

        // Dispatch the appropriate check.
        let result = if config.sensor_type == "external_task" {
            let dag_id = config
                .config
                .get("external_dag_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let task_id = config
                .config
                .get("external_task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if dag_id.is_empty() || task_id.is_empty() {
                return SensorResult::Failed(
                    "ExternalTaskSensor: missing 'external_dag_id' or 'external_task_id' in config"
                        .into(),
                );
            }

            if check_external_task_sensor(db, dag_id, task_id).await {
                info!(
                    "🔗 ExternalTaskSensor: {}/{} succeeded",
                    dag_id, task_id
                );
                SensorResult::ConditionMet
            } else {
                SensorResult::Waiting
            }
        } else {
            evaluate_sensor(config).await
        };

        match result {
            SensorResult::ConditionMet => {
                info!(
                    "✅ Sensor condition met: type='{}'",
                    config.sensor_type
                );
                return SensorResult::ConditionMet;
            }

            SensorResult::Waiting => {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .unwrap_or_default();

                debug!(
                    "⏳ Sensor waiting — next poke in {}s ({:.0}s remaining): type='{}'",
                    config.poke_interval_secs,
                    remaining.as_secs_f64(),
                    config.sensor_type
                );

                // Sleep for poke_interval, but don't overshoot the deadline.
                let sleep_for = poke_interval.min(
                    deadline
                        .checked_duration_since(Instant::now())
                        .unwrap_or_default(),
                );
                sleep(sleep_for).await;
            }

            SensorResult::Failed(ref msg) => {
                error!(
                    "❌ Sensor failed: type='{}' error='{}'",
                    config.sensor_type, msg
                );
                return result;
            }

            SensorResult::TimedOut => {
                // Should not be reached inside the loop, but handle defensively.
                return SensorResult::TimedOut;
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_config_defaults() {
        let json = r#"{"sensor_type": "file", "config": {"filepath": "/tmp/x"}}"#;
        let cfg: SensorConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.poke_interval_secs, 30);
        assert_eq!(cfg.timeout_secs, 3600);
        assert_eq!(cfg.mode, "poke");
    }

    #[tokio::test]
    async fn file_sensor_missing() {
        let exists = check_file_sensor("/tmp/vortex_sensor_test_nonexistent_xyz123").await;
        assert!(!exists);
    }

    #[tokio::test]
    async fn file_sensor_present() {
        // Create a temp file, then verify the sensor sees it.
        let path = "/tmp/vortex_sensor_test_file_check";
        tokio::fs::write(path, b"ok").await.unwrap();
        let exists = check_file_sensor(path).await;
        tokio::fs::remove_file(path).await.ok();
        assert!(exists);
    }

    #[tokio::test]
    async fn evaluate_sensor_unknown_type() {
        let cfg = SensorConfig {
            sensor_type: "unknown_xyz".into(),
            poke_interval_secs: 5,
            timeout_secs: 10,
            mode: "poke".into(),
            config: serde_json::json!({}),
        };
        let result = evaluate_sensor(&cfg).await;
        assert!(matches!(result, SensorResult::Failed(_)));
    }

    #[tokio::test]
    async fn evaluate_file_sensor_waiting() {
        let cfg = SensorConfig {
            sensor_type: "file".into(),
            poke_interval_secs: 1,
            timeout_secs: 2,
            mode: "poke".into(),
            config: serde_json::json!({ "filepath": "/tmp/vortex_no_such_file_xyz9999" }),
        };
        let result = evaluate_sensor(&cfg).await;
        assert_eq!(result, SensorResult::Waiting);
    }
}
