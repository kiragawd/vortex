use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::db::Db;

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

/// XComStore provides inter-task data passing backed by the VORTEX SQLite database.
pub struct XComStore {
    db: Arc<Db>,
}

impl XComStore {
    /// Create a new XComStore wrapping the shared database handle.
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Push a value into XCom for the given dag/task/run/key.
    ///
    /// - Values larger than [`XCOM_MAX_VALUE_BYTES`] are rejected.
    /// - If the same (dag_id, task_id, run_id, key) already exists it is replaced.
    pub fn xcom_push(
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
            return Err(anyhow!(msg));
        }

        let timestamp = Utc::now().to_rfc3339();
        let conn = self.db.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO task_xcom (dag_id, task_id, run_id, key, value, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(dag_id, task_id, run_id, key)
             DO UPDATE SET value = excluded.value, timestamp = excluded.timestamp",
            params![dag_id, task_id, run_id, key, value, timestamp],
        )?;

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
    pub fn xcom_pull(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        key: &str,
    ) -> Result<Option<String>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT value FROM task_xcom
             WHERE dag_id = ?1 AND task_id = ?2 AND run_id = ?3 AND key = ?4
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![dag_id, task_id, run_id, key], |row| {
            row.get::<_, String>(0)
        })?;

        if let Some(row) = rows.next() {
            let value = row?;
            info!(
                dag_id = dag_id,
                task_id = task_id,
                run_id = run_id,
                key = key,
                "XCom pull hit"
            );
            Ok(Some(value))
        } else {
            info!(
                dag_id = dag_id,
                task_id = task_id,
                run_id = run_id,
                key = key,
                "XCom pull miss (no entry found)"
            );
            Ok(None)
        }
    }

    /// Pull all XCom entries for a given dag run.
    ///
    /// Useful for surfacing all inter-task outputs in the UI or for auditing.
    pub fn xcom_pull_all(&self, dag_id: &str, run_id: &str) -> Result<Vec<XComEntry>> {
        let conn = self.db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT dag_id, task_id, run_id, key, value, timestamp
             FROM task_xcom
             WHERE dag_id = ?1 AND run_id = ?2
             ORDER BY timestamp ASC",
        )?;

        let rows = stmt.query_map(params![dag_id, run_id], |row| {
            Ok(XComEntry {
                dag_id: row.get::<_, String>(0)?,
                task_id: row.get::<_, String>(1)?,
                run_id: row.get::<_, String>(2)?,
                key: row.get::<_, String>(3)?,
                value: row.get::<_, String>(4)?,
                timestamp: row.get::<_, String>(5)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }

        info!(
            dag_id = dag_id,
            run_id = run_id,
            count = entries.len(),
            "XCom pull_all complete"
        );
        Ok(entries)
    }

    /// Delete all XCom entries for a given dag run.
    ///
    /// Should be called after a run completes to free storage.
    pub fn xcom_delete(&self, dag_id: &str, run_id: &str) -> Result<()> {
        let conn = self.db.conn.lock().unwrap();
        let deleted = conn.execute(
            "DELETE FROM task_xcom WHERE dag_id = ?1 AND run_id = ?2",
            params![dag_id, run_id],
        )?;

        if deleted > 0 {
            info!(
                dag_id = dag_id,
                run_id = run_id,
                deleted_rows = deleted,
                "XCom entries deleted after run cleanup"
            );
        } else {
            info!(
                dag_id = dag_id,
                run_id = run_id,
                "XCom delete: no entries found for run (already clean)"
            );
        }
        Ok(())
    }
}
