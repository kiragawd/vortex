#![allow(dead_code)]
// incident.rs — Incident Management Integrations
// PagerDuty, Opsgenie, Datadog incident triggers
//
// Dispatches incident alerts with rich context when critical pipeline
// events occur (failures, SLA breaches, prolonged outages).

use anyhow::{Result, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

// ── Incident Data Types ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentAlert {
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub source: String,
    pub dag_id: String,
    pub task_id: Option<String>,
    pub run_id: String,
    pub team_id: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub details: serde_json::Value,
    pub dedup_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Error,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::Error => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
            Self::Info => write!(f, "info"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentProviderConfig {
    pub id: String,
    pub team_id: Option<String>,
    pub provider: IncidentProviderType,
    pub name: String,
    pub config: serde_json::Value,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IncidentProviderType {
    PagerDuty,
    Opsgenie,
    Datadog,
    Webhook,
}

// ── Incident Provider Trait ────────────────────────────────────────

#[async_trait]
pub trait IncidentProvider: Send + Sync {
    /// Send an incident alert.
    async fn trigger(&self, alert: &IncidentAlert) -> Result<String>;

    /// Acknowledge an incident.
    async fn acknowledge(&self, dedup_key: &str) -> Result<()>;

    /// Resolve an incident.
    async fn resolve(&self, dedup_key: &str) -> Result<()>;

    /// Provider name.
    fn name(&self) -> &str;
}

// ── PagerDuty Integration ──────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct PagerDutyConfig {
    pub routing_key: String,
}

pub struct PagerDutyProvider {
    config: PagerDutyConfig,
    http_client: reqwest::Client,
}

impl PagerDutyProvider {
    pub fn new(config: PagerDutyConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl IncidentProvider for PagerDutyProvider {
    async fn trigger(&self, alert: &IncidentAlert) -> Result<String> {
        let payload = serde_json::json!({
            "routing_key": self.config.routing_key,
            "event_action": "trigger",
            "dedup_key": alert.dedup_key,
            "payload": {
                "summary": alert.title,
                "severity": alert.severity.to_string(),
                "source": alert.source,
                "component": alert.dag_id,
                "group": alert.team_id,
                "timestamp": alert.timestamp.to_rfc3339(),
                "custom_details": {
                    "dag_id": alert.dag_id,
                    "task_id": alert.task_id,
                    "run_id": alert.run_id,
                    "description": alert.description,
                    "details": alert.details,
                }
            }
        });

        let resp = self.http_client
            .post("https://events.pagerduty.com/v2/enqueue")
            .json(&payload)
            .send()
            .await
            .context("PagerDuty trigger failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("PagerDuty trigger failed: {}", body);
        }

        info!("🚨 PagerDuty incident triggered: {}", alert.title);
        Ok(alert.dedup_key.clone())
    }

    async fn acknowledge(&self, dedup_key: &str) -> Result<()> {
        let payload = serde_json::json!({
            "routing_key": self.config.routing_key,
            "event_action": "acknowledge",
            "dedup_key": dedup_key,
        });

        self.http_client
            .post("https://events.pagerduty.com/v2/enqueue")
            .json(&payload)
            .send()
            .await
            .context("PagerDuty acknowledge failed")?;

        Ok(())
    }

    async fn resolve(&self, dedup_key: &str) -> Result<()> {
        let payload = serde_json::json!({
            "routing_key": self.config.routing_key,
            "event_action": "resolve",
            "dedup_key": dedup_key,
        });

        self.http_client
            .post("https://events.pagerduty.com/v2/enqueue")
            .json(&payload)
            .send()
            .await
            .context("PagerDuty resolve failed")?;

        info!("✅ PagerDuty incident resolved: {}", dedup_key);
        Ok(())
    }

    fn name(&self) -> &str {
        "pagerduty"
    }
}

// ── Opsgenie Integration ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct OpsgenieConfig {
    pub api_key: String,
    pub api_url: Option<String>,
}

pub struct OpsgenieProvider {
    config: OpsgenieConfig,
    http_client: reqwest::Client,
}

impl OpsgenieProvider {
    pub fn new(config: OpsgenieConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl IncidentProvider for OpsgenieProvider {
    async fn trigger(&self, alert: &IncidentAlert) -> Result<String> {
        let api_url = self.config.api_url.as_deref().unwrap_or("https://api.opsgenie.com");
        let priority = match alert.severity {
            Severity::Critical => "P1",
            Severity::Error => "P2",
            Severity::Warning => "P3",
            Severity::Info => "P4",
        };

        let payload = serde_json::json!({
            "message": alert.title,
            "alias": alert.dedup_key,
            "description": alert.description,
            "priority": priority,
            "source": alert.source,
            "tags": [&alert.dag_id, "ryuo"],
            "details": {
                "dag_id": alert.dag_id,
                "task_id": alert.task_id,
                "run_id": alert.run_id,
                "team_id": alert.team_id,
            },
            "entity": alert.dag_id,
        });

        let resp = self.http_client
            .post(format!("{}/v2/alerts", api_url))
            .header("Authorization", format!("GenieKey {}", self.config.api_key))
            .json(&payload)
            .send()
            .await
            .context("Opsgenie trigger failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Opsgenie trigger failed: {}", body);
        }

        info!("🚨 Opsgenie alert triggered: {}", alert.title);
        Ok(alert.dedup_key.clone())
    }

    async fn acknowledge(&self, dedup_key: &str) -> Result<()> {
        let api_url = self.config.api_url.as_deref().unwrap_or("https://api.opsgenie.com");
        self.http_client
            .post(format!("{}/v2/alerts/{}/acknowledge", api_url, dedup_key))
            .header("Authorization", format!("GenieKey {}", self.config.api_key))
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("Opsgenie acknowledge failed")?;
        Ok(())
    }

    async fn resolve(&self, dedup_key: &str) -> Result<()> {
        let api_url = self.config.api_url.as_deref().unwrap_or("https://api.opsgenie.com");
        self.http_client
            .post(format!("{}/v2/alerts/{}/close", api_url, dedup_key))
            .header("Authorization", format!("GenieKey {}", self.config.api_key))
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("Opsgenie resolve failed")?;
        info!("✅ Opsgenie alert resolved: {}", dedup_key);
        Ok(())
    }

    fn name(&self) -> &str {
        "opsgenie"
    }
}

// ── Datadog Integration ────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct DatadogConfig {
    pub api_key: String,
    pub site: Option<String>,
}

pub struct DatadogProvider {
    config: DatadogConfig,
    http_client: reqwest::Client,
}

impl DatadogProvider {
    pub fn new(config: DatadogConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl IncidentProvider for DatadogProvider {
    async fn trigger(&self, alert: &IncidentAlert) -> Result<String> {
        let site = self.config.site.as_deref().unwrap_or("datadoghq.com");
        let alert_type = match alert.severity {
            Severity::Critical | Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };

        let payload = serde_json::json!({
            "title": alert.title,
            "text": alert.description,
            "alert_type": alert_type,
            "source_type_name": "ryuo",
            "aggregation_key": alert.dedup_key,
            "tags": [
                format!("dag_id:{}", alert.dag_id),
                format!("run_id:{}", alert.run_id),
                format!("severity:{}", alert.severity),
                "source:ryuo".to_string(),
            ],
        });

        let resp = self.http_client
            .post(format!("https://api.{}/api/v1/events", site))
            .header("DD-API-KEY", &self.config.api_key)
            .json(&payload)
            .send()
            .await
            .context("Datadog event trigger failed")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Datadog event trigger failed: {}", body);
        }

        info!("🚨 Datadog event triggered: {}", alert.title);
        Ok(alert.dedup_key.clone())
    }

    async fn acknowledge(&self, _dedup_key: &str) -> Result<()> {
        // Datadog Events API doesn't have acknowledge — handled in Datadog UI
        Ok(())
    }

    async fn resolve(&self, dedup_key: &str) -> Result<()> {
        // Send a resolution event
        let site = self.config.site.as_deref().unwrap_or("datadoghq.com");
        let payload = serde_json::json!({
            "title": format!("Resolved: {}", dedup_key),
            "text": "Incident resolved automatically by Ryuo.",
            "alert_type": "success",
            "source_type_name": "ryuo",
            "aggregation_key": dedup_key,
            "tags": ["source:ryuo", "resolved:true"],
        });

        self.http_client
            .post(format!("https://api.{}/api/v1/events", site))
            .header("DD-API-KEY", &self.config.api_key)
            .json(&payload)
            .send()
            .await
            .context("Datadog resolve event failed")?;

        info!("✅ Datadog incident resolved: {}", dedup_key);
        Ok(())
    }

    fn name(&self) -> &str {
        "datadog"
    }
}

// ── Incident Manager ───────────────────────────────────────────────

/// Manages configured incident providers and dispatches alerts.
pub struct IncidentManager {
    providers: Vec<(Option<String>, Arc<dyn IncidentProvider>)>, // (team_id, provider)
}

impl IncidentManager {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn add_provider(&mut self, team_id: Option<String>, provider: Arc<dyn IncidentProvider>) {
        info!("🚨 Registered incident provider: {} (team: {:?})", provider.name(), team_id);
        self.providers.push((team_id, provider));
    }

    /// Dispatch an alert to all matching providers (matching team_id or global).
    pub async fn dispatch(&self, alert: &IncidentAlert) {
        for (team_id, provider) in &self.providers {
            // Send to global providers (no team_id) and team-specific ones
            if team_id.is_none() || team_id.as_deref() == alert.team_id.as_deref() {
                if let Err(e) = provider.trigger(alert).await {
                    warn!("Incident provider '{}' failed to trigger: {}", provider.name(), e);
                }
            }
        }
    }

    /// Create a standard alert for a task failure.
    pub fn task_failure_alert(
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        team_id: Option<&str>,
        error: &str,
    ) -> IncidentAlert {
        IncidentAlert {
            severity: Severity::Error,
            title: format!("Task failed: {}.{}", dag_id, task_id),
            description: format!("Task '{}' in DAG '{}' (run: {}) failed: {}", task_id, dag_id, run_id, error),
            source: "ryuo-scheduler".to_string(),
            dag_id: dag_id.to_string(),
            task_id: Some(task_id.to_string()),
            run_id: run_id.to_string(),
            team_id: team_id.map(|s| s.to_string()),
            timestamp: chrono::Utc::now(),
            details: serde_json::json!({"error": error}),
            dedup_key: format!("ryuo-{}-{}-{}", dag_id, task_id, run_id),
        }
    }

    /// Create a standard alert for an SLA breach.
    pub fn sla_breach_alert(
        dag_id: &str,
        run_id: &str,
        team_id: Option<&str>,
        sla_seconds: u64,
        elapsed_seconds: u64,
    ) -> IncidentAlert {
        IncidentAlert {
            severity: Severity::Warning,
            title: format!("SLA breach: {} exceeded {}s", dag_id, sla_seconds),
            description: format!(
                "DAG '{}' (run: {}) has exceeded its SLA of {}s (elapsed: {}s)",
                dag_id, run_id, sla_seconds, elapsed_seconds
            ),
            source: "ryuo-scheduler".to_string(),
            dag_id: dag_id.to_string(),
            task_id: None,
            run_id: run_id.to_string(),
            team_id: team_id.map(|s| s.to_string()),
            timestamp: chrono::Utc::now(),
            details: serde_json::json!({"sla_seconds": sla_seconds, "elapsed_seconds": elapsed_seconds}),
            dedup_key: format!("ryuo-sla-{}-{}", dag_id, run_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_failure_alert() {
        let alert = IncidentManager::task_failure_alert("my_dag", "task_1", "run-123", Some("team-a"), "Connection timeout");
        assert_eq!(alert.severity.to_string(), "error");
        assert!(alert.title.contains("task_1"));
        assert!(alert.dedup_key.contains("my_dag"));
    }

    #[test]
    fn test_sla_breach_alert() {
        let alert = IncidentManager::sla_breach_alert("etl_dag", "run-456", None, 3600, 4200);
        assert_eq!(alert.severity.to_string(), "warning");
        assert!(alert.title.contains("3600"));
    }
}
