use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::db_trait::DatabaseBackend;

/// Maximum allowed size for an XCom value (64KB).
pub const XCOM_MAX_VALUE_BYTES: usize = 65536;

/// DDL for the XCom table. Called by `db.rs` during `init()`.
pub const XCOM_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS task_xcom (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dag_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    UNIQUE(dag_id, task_id, run_id, key)
)";

/// A single XCom entry — one key/value pair produced by a task during a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XComEntry {
    pub dag_id: String,
    pub task_id: String,
    pub run_id: String,
    pub key: String,
    pub value: String,
    pub timestamp: String,
}

/// XComStore provides inter-task data passing backed by the VORTEX database.
pub struct XComStore {
    db: Arc<dyn DatabaseBackend>,
}

impl XComStore {
    /// Create a new XComStore wrapping the shared database handle.
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Push a value into XCom for the given dag/task/run/key.
    ///
    /// - Values larger than [`XCOM_MAX_VALUE_BYTES`] are rejected.
    /// - If the same (dag_id, task_id, run_id, key) already exists it is replaced.
    pub async fn xcom_push(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        key: &str,
        value: String,
    ) -> Result<()> {
        if value.len() > XCOM_MAX_VALUE_BYTES {
            let msg = format!(
                "XCom value too large: {} bytes (max {}). dag_id={dag_id} task_id={task_id} key={key}",
                value.len(),
                XCOM_MAX_VALUE_BYTES
            );
            warn!("{}", msg);
            return Err(anyhow::anyhow!(msg));
        }

        self.db.xcom_push(dag_id, task_id, run_id, key, &value).await?;

        info!(
            dag_id = dag_id,
            task_id = task_id,
            run_id = run_id,
            key = key,
            "XCom push successful"
        );
        Ok(())
    }

    /// Pull a single XCom value for the given dag/task/run/key.
    ///
    /// Returns `None` if no matching entry exists.
    pub async fn xcom_pull(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        key: &str,
    ) -> Result<Option<String>> {
        let result = self.db.xcom_pull(dag_id, task_id, run_id, key).await?;
        if result.is_some() {
            info!(
                dag_id = dag_id,
                task_id = task_id,
                run_id = run_id,
                key = key,
                "XCom pull hit"
            );
        } else {
            info!(
                dag_id = dag_id,
                task_id = task_id,
                run_id = run_id,
                key = key,
                "XCom pull miss (no entry found)"
            );
        }
        Ok(result)
    }

    /// Pull all XCom entries for a given dag run.
    ///
    /// Useful for surfacing all inter-task outputs in the UI or for auditing.
    pub async fn xcom_pull_all(&self, dag_id: &str, run_id: &str) -> Result<Vec<serde_json::Value>> {
        let entries = self.db.xcom_pull_all(dag_id, run_id).await?;
        info!(
            dag_id = dag_id,
            run_id = run_id,
            count = entries.len(),
            "XCom pull_all complete"
        );
        Ok(entries)
    }
}
