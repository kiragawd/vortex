#![allow(dead_code)]
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ConnectorKind {
    Database,
    Warehouse,
    Api,
    Transformation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ConnectorCapability {
    Transactions,
    BatchRead,
    BatchWrite,
    StreamingRead,
    AsyncJobs,
    ArrowZeroCopy,
    PushdownPredicates,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_backoff_ms: 250,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthContext {
    pub token: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorContext {
    pub request_id: String,
    pub timeout_ms: u64,
    pub retry_policy: RetryPolicy,
    pub auth: AuthContext,
    pub tags: HashMap<String, String>,
}

impl Default for ConnectorContext {
    fn default() -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            timeout_ms: 30_000,
            retry_policy: RetryPolicy::default(),
            auth: AuthContext::default(),
            tags: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryRequest {
    pub sql: Option<String>,
    pub endpoint: Option<String>,
    pub params: Value,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryResult {
    pub schema: Vec<String>,
    pub rows: Vec<Value>,
    pub stats: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub details: String,
}

impl HealthStatus {
    pub fn healthy(details: impl Into<String>) -> Self {
        Self {
            healthy: true,
            details: details.into(),
        }
    }
}

#[async_trait]
pub trait EnterpriseConnector: Send + Sync {
    fn name(&self) -> &'static str;
    fn kind(&self) -> ConnectorKind;
    fn capabilities(&self) -> HashSet<ConnectorCapability>;

    async fn validate_config(&self) -> Result<()>;
    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()>;
    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus>;
    async fn execute(&self, _ctx: &ConnectorContext, _req: QueryRequest) -> Result<QueryResult>;
    async fn stream_execute(
        &self,
        _ctx: &ConnectorContext,
        _req: QueryRequest,
    ) -> Result<Vec<Value>>;
    async fn close(&self) -> Result<()>;
}

// NOTE (BUG-084): Uses std::sync::RwLock intentionally — all critical sections are
// sub-microsecond (HashMap insert/lookup with no await), so blocking is acceptable
// and avoids the overhead of tokio::sync::RwLock's async mutex.
#[derive(Default)]
pub struct ConnectorRegistry {
    inner: RwLock<HashMap<String, Arc<dyn EnterpriseConnector>>>,
}

impl ConnectorRegistry {
    pub fn register(&self, connector: Arc<dyn EnterpriseConnector>) -> Result<()> {
        let name = connector.name().to_string();
        let mut lock = self
            .inner
            .write()
            .map_err(|_| anyhow!("Connector registry lock poisoned"))?;
        if lock.contains_key(&name) {
            return Err(anyhow!("Connector {} already exists", name));
        }
        lock.insert(name, connector);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn EnterpriseConnector>> {
        let lock = self
            .inner
            .read()
            .map_err(|_| anyhow!("Connector registry lock poisoned"))?;
        lock.get(name)
            .cloned()
            .ok_or_else(|| anyhow!("Connector {} not found", name))
    }

    pub fn list(&self) -> Result<Vec<String>> {
        let lock = self
            .inner
            .read()
            .map_err(|_| anyhow!("Connector registry lock poisoned"))?;
        Ok(lock.keys().cloned().collect())
    }
}
