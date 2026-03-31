#![allow(dead_code)]
// lineage.rs — OpenLineage Data Lineage Emission
// Observability & Data Governance
//
// Emits OpenLineage-compliant events (https://openlineage.io/spec) during
// DAG and task execution to enable enterprise data lineage tracking.

use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn, debug};
use chrono::{DateTime, Utc};

// ── OpenLineage Data Types ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EventType {
    Start,
    Running,
    Complete,
    Fail,
    Abort,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start => write!(f, "START"),
            Self::Running => write!(f, "RUNNING"),
            Self::Complete => write!(f, "COMPLETE"),
            Self::Fail => write!(f, "FAIL"),
            Self::Abort => write!(f, "ABORT"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    #[serde(rename = "eventType")]
    pub event_type: EventType,
    #[serde(rename = "eventTime")]
    pub event_time: DateTime<Utc>,
    pub run: RunFacet,
    pub job: JobFacet,
    pub inputs: Vec<DatasetRef>,
    pub outputs: Vec<DatasetRef>,
    pub producer: String,
    #[serde(rename = "schemaURL")]
    pub schema_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunFacet {
    #[serde(rename = "runId")]
    pub run_id: String,
    #[serde(default)]
    pub facets: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFacet {
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub facets: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRef {
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub facets: serde_json::Value,
}

impl RunEvent {
    pub fn new(
        event_type: EventType,
        run_id: &str,
        dag_id: &str,
        task_id: Option<&str>,
        inputs: Vec<DatasetRef>,
        outputs: Vec<DatasetRef>,
    ) -> Self {
        let job_name = match task_id {
            Some(tid) => format!("{}.{}", dag_id, tid),
            None => dag_id.to_string(),
        };

        Self {
            event_type,
            event_time: Utc::now(),
            run: RunFacet {
                run_id: run_id.to_string(),
                facets: serde_json::json!({}),
            },
            job: JobFacet {
                namespace: "vortex".to_string(),
                name: job_name,
                facets: serde_json::json!({}),
            },
            inputs,
            outputs,
            producer: "https://github.com/vortex-engine/vortex".to_string(),
            schema_url: "https://openlineage.io/spec/2-0-2/OpenLineage.json#/definitions/RunEvent".to_string(),
        }
    }
}

// ── Lineage Emitter Trait ──────────────────────────────────────────

#[async_trait]
pub trait LineageEmitter: Send + Sync {
    /// Emit a run event to the lineage backend.
    async fn emit(&self, event: &RunEvent) -> Result<()>;

    /// Return the emitter name.
    fn name(&self) -> &str;
}

// ── HTTP Lineage Emitter (Marquez/Datakin) ─────────────────────────

pub struct HttpLineageEmitter {
    endpoint: String,
    api_key: Option<String>,
    http_client: reqwest::Client,
}

impl HttpLineageEmitter {
    pub fn new(endpoint: &str, api_key: Option<String>) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            api_key,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LineageEmitter for HttpLineageEmitter {
    async fn emit(&self, event: &RunEvent) -> Result<()> {
        let mut req = self.http_client
            .post(&format!("{}/api/v1/lineage", self.endpoint))
            .json(event);

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req.send().await.context("Failed to emit lineage event")?;

        if !resp.status().is_success() {
            warn!("Lineage emission failed with status: {}", resp.status());
        } else {
            debug!("📊 Lineage event emitted: {} for {}", event.event_type, event.job.name);
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "http"
    }
}

// ── Log Lineage Emitter (structured logs) ──────────────────────────

pub struct LogLineageEmitter;

impl LogLineageEmitter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LineageEmitter for LogLineageEmitter {
    async fn emit(&self, event: &RunEvent) -> Result<()> {
        info!(
            event_type = %event.event_type,
            run_id = %event.run.run_id,
            job = %event.job.name,
            inputs = event.inputs.len(),
            outputs = event.outputs.len(),
            "📊 OpenLineage event"
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "log"
    }
}

// ── Database Lineage Emitter ───────────────────────────────────────

pub struct DbLineageEmitter {
    db: Arc<dyn crate::db_trait::DatabaseBackend>,
}

impl DbLineageEmitter {
    pub fn new(db: Arc<dyn crate::db_trait::DatabaseBackend>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl LineageEmitter for DbLineageEmitter {
    async fn emit(&self, event: &RunEvent) -> Result<()> {
        let inputs_json = serde_json::to_string(&event.inputs)?;
        let outputs_json = serde_json::to_string(&event.outputs)?;
        let facets_json = serde_json::to_string(&event.run.facets)?;

        // Extract dag_id and task_id from job name
        let parts: Vec<&str> = event.job.name.splitn(2, '.').collect();
        let dag_id = parts[0];
        let task_id = parts.get(1).copied();

        self.db.store_lineage_event(
            &event.event_type.to_string(),
            &event.run.run_id,
            dag_id,
            task_id,
            &event.job.namespace,
            &event.job.name,
            &inputs_json,
            &outputs_json,
            &facets_json,
        ).await?;

        debug!("📊 Lineage event stored: {} for {}", event.event_type, event.job.name);
        Ok(())
    }

    fn name(&self) -> &str {
        "database"
    }
}

// ── Lineage Manager ────────────────────────────────────────────────

/// Manages multiple lineage emitters and dispatches events to all of them.
pub struct LineageManager {
    emitters: Vec<Arc<dyn LineageEmitter>>,
}

impl LineageManager {
    pub fn new() -> Self {
        Self {
            emitters: Vec::new(),
        }
    }

    pub fn add_emitter(&mut self, emitter: Arc<dyn LineageEmitter>) {
        info!("📊 Registered lineage emitter: {}", emitter.name());
        self.emitters.push(emitter);
    }

    /// Emit a run event to all registered emitters.
    pub async fn emit(&self, event: &RunEvent) {
        for emitter in &self.emitters {
            if let Err(e) = emitter.emit(event).await {
                warn!("Lineage emitter '{}' failed: {}", emitter.name(), e);
            }
        }
    }

    /// Convenience: emit a START event.
    pub async fn emit_start(&self, run_id: &str, dag_id: &str, task_id: Option<&str>) {
        let event = RunEvent::new(EventType::Start, run_id, dag_id, task_id, vec![], vec![]);
        self.emit(&event).await;
    }

    /// Convenience: emit a COMPLETE event.
    pub async fn emit_complete(&self, run_id: &str, dag_id: &str, task_id: Option<&str>, outputs: Vec<DatasetRef>) {
        let event = RunEvent::new(EventType::Complete, run_id, dag_id, task_id, vec![], outputs);
        self.emit(&event).await;
    }

    /// Convenience: emit a FAIL event.
    pub async fn emit_fail(&self, run_id: &str, dag_id: &str, task_id: Option<&str>) {
        let event = RunEvent::new(EventType::Fail, run_id, dag_id, task_id, vec![], vec![]);
        self.emit(&event).await;
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_event_creation() {
        let event = RunEvent::new(
            EventType::Start,
            "run-123",
            "my_dag",
            Some("task_1"),
            vec![DatasetRef {
                namespace: "s3".to_string(),
                name: "s3://bucket/input".to_string(),
                facets: serde_json::json!({}),
            }],
            vec![],
        );
        assert_eq!(event.job.name, "my_dag.task_1");
        assert_eq!(event.run.run_id, "run-123");
        assert!(matches!(event.event_type, EventType::Start));
    }

    #[test]
    fn test_run_event_dag_only() {
        let event = RunEvent::new(EventType::Complete, "run-456", "etl_pipeline", None, vec![], vec![]);
        assert_eq!(event.job.name, "etl_pipeline");
    }

    #[test]
    fn test_event_serialization() {
        let event = RunEvent::new(EventType::Fail, "run-789", "dag", Some("task"), vec![], vec![]);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"eventType\":\"FAIL\""));
        assert!(json.contains("\"schemaURL\""));
    }
}
