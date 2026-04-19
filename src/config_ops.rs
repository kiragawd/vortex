#![allow(dead_code)]
// Configuration Management & Operational Tooling
//
// Environment-scoped configuration, feature flags, operational
// health checks, and administrative tooling.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

// ─── Environment Configuration ────────────────────────────────

/// A named configuration environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub description: String,
    pub variables: HashMap<String, ConfigValue>,
    pub inherits_from: Option<String>,
    pub locked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Typed configuration value with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValue {
    pub value: Value,
    pub secret: bool,
    pub description: String,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    Default,
    File,
    Environment,
    Vault,
    Override,
}

/// Manages environment-scoped configurations.
pub struct ConfigManager {
    environments: Arc<RwLock<HashMap<String, Environment>>>,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            environments: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new environment.
    pub async fn create_environment(&self, name: &str, description: &str, inherits_from: Option<String>) -> Result<Environment> {
        let mut envs = self.environments.write().await;
        if envs.contains_key(name) {
            return Err(anyhow!("Environment '{}' already exists", name));
        }

        if let Some(ref parent) = inherits_from {
            if !envs.contains_key(parent) {
                return Err(anyhow!("Parent environment '{}' not found", parent));
            }
        }

        let now = Utc::now();
        let env = Environment {
            name: name.to_string(),
            description: description.to_string(),
            variables: HashMap::new(),
            inherits_from,
            locked: false,
            created_at: now,
            updated_at: now,
        };
        envs.insert(name.to_string(), env.clone());
        info!(env = %name, "Environment created");
        Ok(env)
    }

    /// Set a configuration value in an environment.
    pub async fn set_value(&self, env_name: &str, key: &str, value: ConfigValue) -> Result<()> {
        let mut envs = self.environments.write().await;
        let env = envs.get_mut(env_name)
            .ok_or_else(|| anyhow!("Environment '{}' not found", env_name))?;
        if env.locked {
            return Err(anyhow!("Environment '{}' is locked", env_name));
        }
        env.variables.insert(key.to_string(), value);
        env.updated_at = Utc::now();
        Ok(())
    }

    /// Get a resolved configuration value (with inheritance).
    pub async fn get_value(&self, env_name: &str, key: &str) -> Option<ConfigValue> {
        let envs = self.environments.read().await;
        self.resolve_value(&envs, env_name, key, 0)
    }

    fn resolve_value(&self, envs: &HashMap<String, Environment>, env_name: &str, key: &str, depth: u32) -> Option<ConfigValue> {
        if depth > 10 {
            tracing::warn!(depth = depth, "Config inheritance depth limit exceeded — possible circular reference");
            return None;
        }
        let env = envs.get(env_name)?;
        if let Some(val) = env.variables.get(key) {
            return Some(val.clone());
        }
        if let Some(ref parent) = env.inherits_from {
            return self.resolve_value(envs, parent, key, depth + 1);
        }
        None
    }

    /// List all environments.
    pub async fn list_environments(&self) -> Vec<Environment> {
        self.environments.read().await.values().cloned().collect()
    }

    /// Lock an environment (prevent changes).
    pub async fn lock_environment(&self, env_name: &str) -> Result<()> {
        let mut envs = self.environments.write().await;
        let env = envs.get_mut(env_name)
            .ok_or_else(|| anyhow!("Environment '{}' not found", env_name))?;
        env.locked = true;
        info!(env = %env_name, "Environment locked");
        Ok(())
    }

    /// Export environment as a flat key-value map (with inheritance resolved).
    pub async fn export_environment(&self, env_name: &str) -> Result<HashMap<String, Value>> {
        let envs = self.environments.read().await;
        if !envs.contains_key(env_name) {
            return Err(anyhow!("Environment '{}' not found", env_name));
        }

        // BUG-060 & BUG-063: Walk full inheritance chain with cycle detection.
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = Some(env_name.to_string());

        while let Some(ref name) = current {
            if !visited.insert(name.clone()) {
                return Err(anyhow!("Circular inheritance detected at environment '{}'", name));
            }
            chain.push(name.clone());
            current = envs.get(name.as_str()).and_then(|e| e.inherits_from.clone());
        }

        // Merge from most-ancestral to most-specific (last in chain = most ancestral).
        let mut result = HashMap::new();
        for name in chain.iter().rev() {
            if let Some(env) = envs.get(name.as_str()) {
                for (k, v) in &env.variables {
                    if !v.secret {
                        result.insert(k.clone(), v.value.clone());
                    }
                }
            }
        }

        Ok(result)
    }
}

// ─── Feature Flags ────────────────────────────────────────────

/// A feature flag definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub rollout_percentage: u8,
    pub allowed_environments: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Feature flag manager.
pub struct FeatureFlagManager {
    flags: Arc<RwLock<HashMap<String, FeatureFlag>>>,
}

impl FeatureFlagManager {
    pub fn new() -> Self {
        Self {
            flags: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a feature flag.
    pub async fn create_flag(&self, name: &str, description: &str) -> Result<FeatureFlag> {
        let mut flags = self.flags.write().await;
        if flags.contains_key(name) {
            return Err(anyhow!("Feature flag '{}' already exists", name));
        }
        let now = Utc::now();
        let flag = FeatureFlag {
            name: name.to_string(),
            description: description.to_string(),
            enabled: false,
            rollout_percentage: 0,
            allowed_environments: Vec::new(),
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
        };
        flags.insert(name.to_string(), flag.clone());
        info!(flag = %name, "Feature flag created");
        Ok(flag)
    }

    /// Check if a feature flag is enabled for a given environment.
    pub async fn is_enabled(&self, name: &str, environment: &str) -> bool {
        let flags = self.flags.read().await;
        match flags.get(name) {
            Some(flag) => {
                if !flag.enabled { return false; }
                if flag.allowed_environments.is_empty() { return true; }
                flag.allowed_environments.contains(&environment.to_string())
            }
            None => false,
        }
    }

    /// Toggle a feature flag.
    pub async fn toggle(&self, name: &str, enabled: bool) -> Result<()> {
        let mut flags = self.flags.write().await;
        let flag = flags.get_mut(name)
            .ok_or_else(|| anyhow!("Feature flag '{}' not found", name))?;
        flag.enabled = enabled;
        flag.updated_at = Utc::now();
        info!(flag = %name, enabled, "Feature flag toggled");
        Ok(())
    }

    /// Set rollout percentage.
    pub async fn set_rollout(&self, name: &str, percentage: u8) -> Result<()> {
        if percentage > 100 {
            return Err(anyhow!("Percentage must be 0-100"));
        }
        let mut flags = self.flags.write().await;
        let flag = flags.get_mut(name)
            .ok_or_else(|| anyhow!("Feature flag '{}' not found", name))?;
        flag.rollout_percentage = percentage;
        flag.updated_at = Utc::now();
        Ok(())
    }

    /// List all feature flags.
    pub async fn list_flags(&self) -> Vec<FeatureFlag> {
        self.flags.read().await.values().cloned().collect()
    }
}

// ─── Operational Health Checks ────────────────────────────────

/// Health check target type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    Database,
    Scheduler,
    Workers,
    GrpcSwarm,
    DiskSpace,
    Memory,
    QueueDepth,
    Custom(String),
}

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub check_type: HealthCheckType,
    pub healthy: bool,
    pub message: String,
    pub latency_ms: u64,
    pub checked_at: DateTime<Utc>,
}

/// System-wide health report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealthReport {
    pub overall_healthy: bool,
    pub checks: Vec<HealthCheckResult>,
    pub generated_at: DateTime<Utc>,
}

/// Operational health checker.
pub struct HealthChecker {
    checks: Arc<RwLock<Vec<HealthCheckResult>>>,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            checks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Run a specific health check (stub — real implementations would probe services).
    pub async fn run_check(&self, check_type: HealthCheckType) -> HealthCheckResult {
        let result = HealthCheckResult {
            check_type: check_type.clone(),
            healthy: true,
            message: format!("{:?} check passed", check_type),
            latency_ms: 1,
            checked_at: Utc::now(),
        };

        let mut checks = self.checks.write().await;
        checks.push(result.clone());
        // Keep last 1000 results
        if checks.len() > 1000 {
            let drain_count = checks.len() - 1000;
            checks.drain(0..drain_count);
        }
        result
    }

    /// Run all standard health checks and produce a report.
    pub async fn full_health_report(&self) -> SystemHealthReport {
        let standard_checks = vec![
            HealthCheckType::Database,
            HealthCheckType::Scheduler,
            HealthCheckType::Workers,
            HealthCheckType::GrpcSwarm,
            HealthCheckType::DiskSpace,
            HealthCheckType::Memory,
            HealthCheckType::QueueDepth,
        ];

        let mut results = Vec::new();
        for check_type in standard_checks {
            results.push(self.run_check(check_type).await);
        }

        let overall = results.iter().all(|r| r.healthy);
        SystemHealthReport {
            overall_healthy: overall,
            checks: results,
            generated_at: Utc::now(),
        }
    }

    /// Get check history for a specific type.
    pub async fn check_history(&self, check_type: Option<HealthCheckType>) -> Vec<HealthCheckResult> {
        let checks = self.checks.read().await;
        match check_type {
            Some(ct) => checks.iter().filter(|c| c.check_type == ct).cloned().collect(),
            None => checks.clone(),
        }
    }
}

// ─── Administrative Tooling ───────────────────────────────────

/// Maintenance window definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id: String,
    pub description: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub suppress_alerts: bool,
    pub pause_scheduling: bool,
    pub created_by: String,
}

/// Operations manager for administrative tasks.
pub struct OpsManager {
    maintenance_windows: Arc<RwLock<Vec<MaintenanceWindow>>>,
}

impl OpsManager {
    pub fn new() -> Self {
        Self {
            maintenance_windows: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Schedule a maintenance window.
    pub async fn schedule_maintenance(&self, window: MaintenanceWindow) -> Result<()> {
        if window.end <= window.start {
            return Err(anyhow!("Maintenance window end must be after start"));
        }
        let mut windows = self.maintenance_windows.write().await;
        info!(id = %window.id, desc = %window.description, "Maintenance window scheduled");
        windows.push(window);
        Ok(())
    }

    /// Check if currently in a maintenance window.
    pub async fn is_in_maintenance(&self) -> Option<MaintenanceWindow> {
        let now = Utc::now();
        let windows = self.maintenance_windows.read().await;
        windows.iter().find(|w| now >= w.start && now < w.end).cloned()
    }

    /// List all maintenance windows.
    pub async fn list_windows(&self) -> Vec<MaintenanceWindow> {
        self.maintenance_windows.read().await.clone()
    }

    /// Cancel a maintenance window.
    pub async fn cancel_maintenance(&self, id: &str) -> Result<()> {
        let mut windows = self.maintenance_windows.write().await;
        let before = windows.len();
        windows.retain(|w| w.id != id);
        if windows.len() == before {
            return Err(anyhow!("Maintenance window '{}' not found", id));
        }
        info!(id = %id, "Maintenance window cancelled");
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_environment_crud() {
        let mgr = ConfigManager::new();
        let env = mgr.create_environment("production", "Prod env", None).await.unwrap();
        assert_eq!(env.name, "production");

        mgr.set_value("production", "db_host", ConfigValue {
            value: Value::String("db.prod.internal".to_string()),
            secret: false,
            description: "Database host".to_string(),
            source: ConfigSource::File,
        }).await.unwrap();

        let val = mgr.get_value("production", "db_host").await.unwrap();
        assert_eq!(val.value, Value::String("db.prod.internal".to_string()));
    }

    #[tokio::test]
    async fn test_config_inheritance() {
        let mgr = ConfigManager::new();
        mgr.create_environment("base", "Base config", None).await.unwrap();
        mgr.set_value("base", "log_level", ConfigValue {
            value: Value::String("info".to_string()),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::Default,
        }).await.unwrap();

        mgr.create_environment("staging", "Staging", Some("base".to_string())).await.unwrap();

        // Should inherit from base
        let val = mgr.get_value("staging", "log_level").await.unwrap();
        assert_eq!(val.value, Value::String("info".to_string()));

        // Override in staging
        mgr.set_value("staging", "log_level", ConfigValue {
            value: Value::String("debug".to_string()),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::Override,
        }).await.unwrap();

        let val = mgr.get_value("staging", "log_level").await.unwrap();
        assert_eq!(val.value, Value::String("debug".to_string()));
    }

    #[tokio::test]
    async fn test_locked_environment() {
        let mgr = ConfigManager::new();
        mgr.create_environment("prod", "Prod", None).await.unwrap();
        mgr.lock_environment("prod").await.unwrap();

        let result = mgr.set_value("prod", "key", ConfigValue {
            value: Value::Bool(true),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::Override,
        }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_feature_flags() {
        let mgr = FeatureFlagManager::new();
        mgr.create_flag("new_ui", "New UI feature").await.unwrap();

        assert!(!mgr.is_enabled("new_ui", "production").await);

        mgr.toggle("new_ui", true).await.unwrap();
        assert!(mgr.is_enabled("new_ui", "production").await);

        let flags = mgr.list_flags().await;
        assert_eq!(flags.len(), 1);
    }

    #[tokio::test]
    async fn test_feature_flag_environment_restriction() {
        let mgr = FeatureFlagManager::new();
        let mut flag = mgr.create_flag("beta_feature", "Beta").await.unwrap();
        mgr.toggle("beta_feature", true).await.unwrap();

        // Set allowed environments via direct mutation (in prod would use an API)
        {
            let mut flags = mgr.flags.write().await;
            let f = flags.get_mut("beta_feature").unwrap();
            f.allowed_environments = vec!["staging".to_string()];
        }

        assert!(mgr.is_enabled("beta_feature", "staging").await);
        assert!(!mgr.is_enabled("beta_feature", "production").await);
    }

    #[tokio::test]
    async fn test_health_checker() {
        let checker = HealthChecker::new();
        let result = checker.run_check(HealthCheckType::Database).await;
        assert!(result.healthy);

        let report = checker.full_health_report().await;
        assert!(report.overall_healthy);
        assert_eq!(report.checks.len(), 7);
    }

    #[tokio::test]
    async fn test_maintenance_windows() {
        let ops = OpsManager::new();
        let start = Utc::now() - chrono::Duration::hours(1);
        let end = Utc::now() + chrono::Duration::hours(1);

        ops.schedule_maintenance(MaintenanceWindow {
            id: "mw-1".to_string(),
            description: "DB upgrade".to_string(),
            start,
            end,
            suppress_alerts: true,
            pause_scheduling: true,
            created_by: "admin".to_string(),
        }).await.unwrap();

        let active = ops.is_in_maintenance().await;
        assert!(active.is_some());
        assert_eq!(active.unwrap().id, "mw-1");

        ops.cancel_maintenance("mw-1").await.unwrap();
        assert!(ops.is_in_maintenance().await.is_none());
    }

    #[tokio::test]
    async fn test_invalid_maintenance_window() {
        let ops = OpsManager::new();
        let result = ops.schedule_maintenance(MaintenanceWindow {
            id: "bad".to_string(),
            description: "".to_string(),
            start: Utc::now(),
            end: Utc::now() - chrono::Duration::hours(1),
            suppress_alerts: false,
            pause_scheduling: false,
            created_by: "admin".to_string(),
        }).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_export_environment_masks_secrets() {
        let mgr = ConfigManager::new();
        mgr.create_environment("prod", "Prod", None).await.unwrap();
        mgr.set_value("prod", "db_host", ConfigValue {
            value: Value::String("db.prod".to_string()),
            secret: false,
            description: "".to_string(),
            source: ConfigSource::File,
        }).await.unwrap();
        mgr.set_value("prod", "db_password", ConfigValue {
            value: Value::String("supersecret".to_string()),
            secret: true,
            description: "".to_string(),
            source: ConfigSource::Vault,
        }).await.unwrap();

        let exported = mgr.export_environment("prod").await.unwrap();
        assert!(exported.contains_key("db_host"));
        assert!(!exported.contains_key("db_password")); // secrets excluded
    }
}
