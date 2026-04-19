#![allow(dead_code)]
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::db_trait::DatabaseBackend;

// ───────────────────────────────── Audit Log ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub event_type: String,
    pub actor: String,
    pub actor_ip: Option<String>,
    pub resource_type: String,
    pub resource_id: String,
    pub action: String,
    pub details: serde_json::Value,
    pub team_id: Option<String>,
}

/// Central audit logger that writes to the database.
pub struct AuditLogger {
    db: Arc<dyn DatabaseBackend>,
}

impl AuditLogger {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    pub async fn log(&self, entry: &AuditEntry) -> Result<()> {
        self.db.insert_audit_log(entry).await?;
        info!(
            event_type = %entry.event_type,
            actor = %entry.actor,
            resource = %format!("{}/{}", entry.resource_type, entry.resource_id),
            action = %entry.action,
            "audit"
        );
        Ok(())
    }
}

// ─────────────────────────── Approval Gate Engine ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalGate {
    pub id: String,
    pub name: String,
    pub resource_type: String,
    pub resource_pattern: String,
    pub required_approvers: i32,
    pub approver_roles: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub gate_id: String,
    pub requester: String,
    pub resource_type: String,
    pub resource_id: String,
    pub change_description: Option<String>,
    pub change_diff: serde_json::Value,
    pub status: String,
    pub approvals: serde_json::Value,
    pub rejections: serde_json::Value,
}

pub struct ApprovalEngine {
    db: Arc<dyn DatabaseBackend>,
}

impl ApprovalEngine {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Check if a resource change requires approval. Returns the matching gate if so.
    pub async fn requires_approval(
        &self,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        self.db.find_matching_approval_gate(resource_type, resource_id).await
    }

    /// Submit a new approval request.
    pub async fn submit_request(
        &self,
        gate_id: &str,
        requester: &str,
        resource_type: &str,
        resource_id: &str,
        description: Option<&str>,
        diff: &serde_json::Value,
    ) -> Result<String> {
        self.db.create_approval_request(gate_id, requester, resource_type, resource_id, description, diff).await
    }

    /// Add an approval vote to a request.
    pub async fn approve(
        &self,
        request_id: &str,
        approver: &str,
        comment: Option<&str>,
    ) -> Result<String> {
        self.db.add_approval_vote(request_id, approver, comment).await
    }

    /// Reject a request.
    pub async fn reject(
        &self,
        request_id: &str,
        rejector: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        self.db.reject_approval_request(request_id, rejector, reason).await
    }
}

// ─────────────────────────── Retention Policy Engine ──────────────────────────

pub struct RetentionEngine {
    db: Arc<dyn DatabaseBackend>,
}

impl RetentionEngine {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Run all enabled retention policies once.
    pub async fn run_policies(&self) -> Result<Vec<(String, i64)>> {
        let policies = self.db.get_retention_policies(true).await?;
        let mut results = Vec::new();
        for policy in policies {
            let name = policy.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            let table = policy.get("target_table").and_then(|v| v.as_str()).unwrap_or("");
            let days = policy.get("retention_days").and_then(|v| v.as_i64()).unwrap_or(90);
            let batch = policy.get("delete_batch_size").and_then(|v| v.as_i64()).unwrap_or(1000);
            let id = policy.get("id").and_then(|v| v.as_str()).unwrap_or("");

            let deleted = self.db.execute_retention_delete(table, days, batch).await?;
            self.db.update_retention_last_run(id).await?;
            info!(policy = %name, table = %table, deleted = deleted, "retention_policy_executed");
            results.push((name.to_string(), deleted));
        }
        Ok(results)
    }
}

// ────────────────────────── Compliance Controls ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceControl {
    pub framework: String,
    pub control_id: String,
    pub description: String,
    pub status: String,
    pub evidence: serde_json::Value,
}

pub struct ComplianceTracker {
    db: Arc<dyn DatabaseBackend>,
}

impl ComplianceTracker {
    pub fn new(db: Arc<dyn DatabaseBackend>) -> Self {
        Self { db }
    }

    /// Get all controls for a framework, or all if framework is None.
    pub async fn get_controls(&self, framework: Option<&str>) -> Result<Vec<serde_json::Value>> {
        self.db.get_compliance_controls(framework).await
    }

    /// Update the status and evidence for a control.
    pub async fn assess_control(
        &self,
        framework: &str,
        control_id: &str,
        status: &str,
        evidence: &serde_json::Value,
        assessor: &str,
    ) -> Result<()> {
        self.db.upsert_compliance_control(framework, control_id, "", status, evidence, assessor).await
    }

    /// Generate a compliance summary for a framework.
    pub async fn summary(&self, framework: &str) -> Result<serde_json::Value> {
        let controls = self.db.get_compliance_controls(Some(framework)).await?;
        let total = controls.len();
        let compliant = controls.iter().filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("compliant")).count();
        let non_compliant = controls.iter().filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("non_compliant")).count();
        let partial = controls.iter().filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("partially_compliant")).count();
        let not_assessed = total - compliant - non_compliant - partial;
        Ok(serde_json::json!({
            "framework": framework,
            "total": total,
            "compliant": compliant,
            "non_compliant": non_compliant,
            "partially_compliant": partial,
            "not_assessed": not_assessed,
            "compliance_rate": if total > 0 { (compliant as f64 / total as f64 * 100.0).round() } else { 0.0 },
        }))
    }
}

// ──────────────────────────── Secret Masking ──────────────────────────────────

/// Mask sensitive values in log output and API responses.
pub fn mask_secret(_value: &str) -> String {
    "****".to_string()
}

/// Scan a JSON value and mask any fields that look like secrets.
pub fn mask_sensitive_fields(value: &serde_json::Value) -> serde_json::Value {
    const SENSITIVE_KEYS: &[&str] = &[
        "password", "secret", "token", "api_key", "apikey",
        "access_key", "private_key", "credential", "auth",
    ];

    match value {
        serde_json::Value::Object(map) => {
            let mut masked = serde_json::Map::new();
            for (k, v) in map {
                let key_lower = k.to_lowercase();
                if SENSITIVE_KEYS.iter().any(|s| key_lower.contains(s)) {
                    if let Some(s) = v.as_str() {
                        masked.insert(k.clone(), serde_json::Value::String(mask_secret(s)));
                    } else {
                        masked.insert(k.clone(), serde_json::Value::String("****".into()));
                    }
                } else {
                    masked.insert(k.clone(), mask_sensitive_fields(v));
                }
            }
            serde_json::Value::Object(masked)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(mask_sensitive_fields).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_secret_short() {
        assert_eq!(mask_secret("ab"), "****");
    }

    #[test]
    fn test_mask_secret_long() {
        assert_eq!(mask_secret("super_secret_123"), "****");
    }

    #[test]
    fn test_mask_sensitive_fields() {
        let input = serde_json::json!({
            "username": "admin",
            "password": "hunter2",
            "nested": {
                "api_key": "abc123",
                "name": "test"
            }
        });
        let masked = mask_sensitive_fields(&input);
        assert_eq!(masked["username"], "admin");
        assert_eq!(masked["password"], "****");
        assert_eq!(masked["nested"]["api_key"], "****");
        assert_eq!(masked["nested"]["name"], "test");
    }
}
