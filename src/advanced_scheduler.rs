#![allow(dead_code)]
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::db_trait::DatabaseBackend;

// ──────────────────────── Dataset-Triggered Scheduling ────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub producer_dag_id: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetEvent {
    pub dataset_id: String,
    pub source_dag_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_run_id: Option<String>,
    pub event_type: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetTrigger {
    pub id: String,
    pub dag_id: String,
    pub dataset_ids: Vec<String>,
    pub condition: TriggerCondition,
    pub min_interval_seconds: Option<i32>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TriggerCondition {
    All,  // All datasets must be updated
    Any,  // Any dataset update triggers
}

impl TriggerCondition {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "any" => TriggerCondition::Any,
            _ => TriggerCondition::All,
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            TriggerCondition::All => "all",
            TriggerCondition::Any => "any",
        }
    }
}

/// The dataset-aware scheduler evaluates triggers when dataset events occur.
pub struct DatasetScheduler {
    db: Arc<dyn DatabaseBackend>,
}

impl DatasetScheduler {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Record a dataset update event and check if any triggers should fire.
    pub async fn record_dataset_event(&self, event: &DatasetEvent) -> Result<Vec<String>> {
        // Store the event
        self.db.insert_dataset_event(event).await?;

        // Check all triggers that reference this dataset
        let triggers = self.db.get_dataset_triggers_for_dataset(&event.dataset_id).await?;
        let mut triggered_dags = Vec::new();

        for trigger in &triggers {
            let trigger_id = trigger.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let dag_id = trigger.get("dag_id").and_then(|v| v.as_str()).unwrap_or("");
            let condition = trigger.get("condition").and_then(|v| v.as_str()).unwrap_or("all");
            let dataset_ids: Vec<String> = trigger.get("dataset_ids")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();

            let should_trigger = match condition {
                "any" => true, // Any dataset update triggers
                _ => {
                    // Check if ALL required datasets have been updated
                    self.db.check_all_datasets_updated(&dataset_ids, trigger_id).await.unwrap_or(false)
                }
            };

            if should_trigger {
                info!(dag_id = %dag_id, trigger_id = %trigger_id, "dataset_trigger_fired");
                triggered_dags.push(dag_id.to_string());
            }
        }
        Ok(triggered_dags)
    }
}

// ──────────────────────── Cross-DAG Dependencies ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDagDependency {
    pub downstream_dag_id: String,
    pub upstream_dag_id: String,
    pub upstream_task_id: Option<String>,
    pub condition: String,
}

/// Evaluates cross-DAG dependencies before a DAG can start.
pub struct CrossDagResolver {
    db: Arc<dyn DatabaseBackend>,
}

impl CrossDagResolver {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Check if all upstream dependencies are satisfied for a given DAG.
    pub async fn dependencies_met(&self, dag_id: &str) -> Result<bool> {
        let deps = self.db.get_cross_dag_dependencies(dag_id).await?;
        if deps.is_empty() {
            return Ok(true);
        }

        for dep in &deps {
            let upstream_dag = dep.get("upstream_dag_id").and_then(|v| v.as_str()).unwrap_or("");
            let upstream_task = dep.get("upstream_task_id").and_then(|v| v.as_str());
            let condition = dep.get("condition").and_then(|v| v.as_str()).unwrap_or("success");

            let satisfied = self.db.check_upstream_completed(upstream_dag, upstream_task, condition).await?;
            if !satisfied {
                warn!(dag_id = %dag_id, upstream = %upstream_dag, "cross_dag_dependency_not_met");
                return Ok(false);
            }
        }
        Ok(true)
    }
}

// ──────────────────────── Dynamic Task Mapping ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMapTemplate {
    pub dag_id: String,
    pub task_id: String,
    pub map_type: MapType,
    pub map_source: String,
    pub max_map_length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MapType {
    Expand,  // Fan-out: one task becomes many
    Reduce,  // Fan-in: many tasks merge into one
}

impl MapType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "reduce" => MapType::Reduce,
            _ => MapType::Expand,
        }
    }
}

/// Expand a mapped task into concrete task instances.
pub fn expand_mapped_task(
    template: &TaskMapTemplate,
    map_values: &[serde_json::Value],
) -> Vec<(String, serde_json::Value)> {
    let max_len = template.max_map_length as usize;
    let values = if map_values.len() > max_len {
        warn!(
            task = %template.task_id,
            count = map_values.len(),
            max = max_len,
            "task_map_truncated"
        );
        &map_values[..max_len]
    } else {
        map_values
    };

    values.iter().enumerate().map(|(i, val)| {
        let mapped_id = format!("{}__map_{}", template.task_id, i);
        (mapped_id, val.clone())
    }).collect()
}

/// ENT-9: Scheduler integration for dynamic task mapping.
///
/// NOTE: Full implementation requires DB-layer methods:
///   - `db.get_mappable_tasks(dag_id)` → Vec of tasks with `expand_kwargs`
///   - `db.create_mapped_task_instance(dag_id, task_id, run_id, map_index, config)`
/// Once those are added to `DatabaseBackend`, replace the TODO body below.
pub struct DynamicTaskScheduler {
    db: Arc<dyn DatabaseBackend>,
}

impl DynamicTaskScheduler {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Expand mapped tasks for a DAG run and persist the resulting instances.
    /// Returns the number of mapped task instances created.
    pub async fn schedule_dynamic_tasks(&self, dag_id: &str, _run_id: &str) -> anyhow::Result<usize> {
        // TODO(ENT-9): implement when DatabaseBackend gains:
        //   get_mappable_tasks(dag_id) and create_mapped_task_instance(...)
        //
        // Pseudocode:
        //   let mapped_tasks = self.db.get_mappable_tasks(dag_id).await?;
        //   for task in mapped_tasks {
        //       if let Some(config) = &task.expand_kwargs {
        //           let expanded = expand_mapped_task(&template, &values);
        //           for (idx, cfg) in expanded.iter().enumerate() {
        //               self.db.create_mapped_task_instance(dag_id, &task.task_id, run_id, idx, cfg).await?;
        //           }
        //       }
        //   }
        warn!(dag_id = %dag_id, "ENT-9: schedule_dynamic_tasks is a stub — DB methods not yet implemented");
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_condition_from_str() {
        assert_eq!(TriggerCondition::from_str("all"), TriggerCondition::All);
        assert_eq!(TriggerCondition::from_str("any"), TriggerCondition::Any);
        assert_eq!(TriggerCondition::from_str("ALL"), TriggerCondition::All);
    }

    #[test]
    fn test_expand_mapped_task() {
        let template = TaskMapTemplate {
            dag_id: "dag1".into(),
            task_id: "process".into(),
            map_type: MapType::Expand,
            map_source: "xcom:list_files".into(),
            max_map_length: 100,
        };
        let values = vec![
            serde_json::json!("file1.csv"),
            serde_json::json!("file2.csv"),
            serde_json::json!("file3.csv"),
        ];
        let expanded = expand_mapped_task(&template, &values);
        assert_eq!(expanded.len(), 3);
        assert_eq!(expanded[0].0, "process__map_0");
        assert_eq!(expanded[2].1, "file3.csv");
    }

    #[test]
    fn test_expand_mapped_task_truncation() {
        let template = TaskMapTemplate {
            dag_id: "dag1".into(),
            task_id: "t".into(),
            map_type: MapType::Expand,
            map_source: "inline".into(),
            max_map_length: 2,
        };
        let values = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let expanded = expand_mapped_task(&template, &values);
        assert_eq!(expanded.len(), 2); // Truncated to max
    }
}
