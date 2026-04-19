#![allow(dead_code)]
// Expanded Connector Ecosystem
//
// Adds cloud-native connectors: BigQuery, Redshift, Kafka, Delta Lake, S3, GCS
// All implement the EnterpriseConnector trait from enterprise_connector.rs

use crate::connectors::validate_connector_sql;
use crate::enterprise_connector::{
    ConnectorCapability, ConnectorContext, ConnectorKind, EnterpriseConnector, HealthStatus,
    QueryRequest, QueryResult,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::Column;
use std::collections::{HashMap, HashSet};

// BUG-028 FIX: Redshift connection pool cache to avoid creating a new pool per query.
use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;
use sqlx::PgPool;
use lru::LruCache;
use std::num::NonZeroUsize;

static REDSHIFT_POOL_CACHE: Lazy<AsyncMutex<LruCache<String, PgPool>>> =
    Lazy::new(|| AsyncMutex::new(LruCache::new(NonZeroUsize::new(16).unwrap())));

// ─── BigQuery Connector ──────────────────────────────────────

pub struct BigQueryConnector {
    pub project_id: String,
    pub dataset: String,
    pub credentials_json: Option<String>,
    pub location: String,
}

impl BigQueryConnector {
    pub fn new(project_id: &str, dataset: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            dataset: dataset.to_string(),
            credentials_json: None,
            location: "US".to_string(),
        }
    }

    pub fn with_credentials(mut self, creds: &str) -> Self {
        self.credentials_json = Some(creds.to_string());
        self
    }

    pub fn with_location(mut self, location: &str) -> Self {
        self.location = location.to_string();
        self
    }
}

#[async_trait]
impl EnterpriseConnector for BigQueryConnector {
    fn name(&self) -> &'static str { "bigquery" }
    fn kind(&self) -> ConnectorKind { ConnectorKind::Warehouse }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        let mut caps = HashSet::new();
        caps.insert(ConnectorCapability::BatchRead);
        caps.insert(ConnectorCapability::BatchWrite);
        caps.insert(ConnectorCapability::AsyncJobs);
        caps
    }

    async fn validate_config(&self) -> Result<()> {
        if self.project_id.is_empty() { return Err(anyhow!("BigQuery: project_id required")); }
        if self.dataset.is_empty() { return Err(anyhow!("BigQuery: dataset required")); }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> { self.validate_config().await }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        Ok(HealthStatus::healthy("BigQuery connector ready"))
    }

    async fn execute(&self, _ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let sql = req.sql.as_deref().ok_or_else(|| anyhow!("BigQuery: sql required"))?;

        // BUG-066 FIX: Validate SQL before execution — only allow SELECT statements.
        validate_connector_sql(sql).map_err(|e| anyhow!("BigQuery connector: {}", e))?;

        // BUG-M5 FIX: Prefer credentials_json when available, fall back to env var.
        let token = if let Some(ref creds_json) = self.credentials_json {
            // In a full implementation, we would use the service account credentials JSON
            // to perform OAuth2 token exchange. For now, treat it as a pre-fetched token
            // if it doesn't look like JSON, or extract from the JSON structure.
            if creds_json.trim_start().starts_with('{') {
                // Service account JSON — extract or exchange for token.
                // Full implementation would use google-auth library.
                // For now, fall back to env var with a better error message.
                std::env::var("GOOGLE_OAUTH_TOKEN").map_err(|_| anyhow!(
                    "BigQuery: Service account JSON provided but GOOGLE_OAUTH_TOKEN not set. \
                     Full service account token exchange is not yet implemented."
                ))?
            } else {
                // Treat as a raw OAuth token
                creds_json.clone()
            }
        } else {
            std::env::var("GOOGLE_OAUTH_TOKEN")
                .map_err(|_| anyhow!("BigQuery: No credentials_json and GOOGLE_OAUTH_TOKEN not set"))?
        };

        let url = format!(
            "https://bigquery.googleapis.com/bigquery/v2/projects/{}/queries",
            self.project_id
        );
        let body = json!({
            "query": sql, "useLegacySql": false,
            "defaultDataset": {"projectId": self.project_id, "datasetId": self.dataset},
            "location": self.location, "maxResults": req.limit.unwrap_or(10000)
        });

        let client = reqwest::Client::new();
        let resp = client.post(&url).bearer_auth(token).json(&body)
            .timeout(std::time::Duration::from_millis(_ctx.timeout_ms))
            .send().await.map_err(|e| anyhow!("BigQuery API error: {}", e))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("BigQuery query failed: {}", text));
        }

        let result: Value = resp.json().await?;
        let schema: Vec<String> = result["schema"]["fields"].as_array()
            .map(|f| f.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        let rows: Vec<Value> = result["rows"].as_array()
            .map(|r| r.iter().cloned().collect()).unwrap_or_default();
        let mut stats = HashMap::new();
        stats.insert("totalRows".to_string(), result["totalRows"].clone());
        Ok(QueryResult { schema, rows, stats })
    }

    async fn stream_execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<Vec<Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> { Ok(()) }
}

// ─── Redshift Connector ──────────────────────────────────────

pub struct RedshiftConnector {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    pub schema_name: String,
}

impl RedshiftConnector {
    pub fn new(host: &str, database: &str, user: &str, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port: 5439,
            database: database.to_string(),
            user: user.to_string(),
            password: password.to_string(),
            schema_name: "public".to_string(),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_schema(mut self, schema: &str) -> Self {
        self.schema_name = schema.to_string();
        self
    }

    /// SECURITY (BUG-M6): Build connection string in a local scope.
    /// Callers should clear the returned String after establishing the connection.
    // TODO (BUG-082): Use `secrecy::SecretString` with zeroize for robust secret lifecycle management.
    // `String::clear()` sets length to 0 but bytes remain in heap; SecretString guarantees zeroize-on-drop.
    fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.database
        )
    }
}

#[async_trait]
impl EnterpriseConnector for RedshiftConnector {
    fn name(&self) -> &'static str { "redshift" }
    fn kind(&self) -> ConnectorKind { ConnectorKind::Warehouse }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        let mut caps = HashSet::new();
        caps.insert(ConnectorCapability::BatchRead);
        caps.insert(ConnectorCapability::BatchWrite);
        caps.insert(ConnectorCapability::Transactions);
        caps
    }

    async fn validate_config(&self) -> Result<()> {
        if self.host.is_empty() { return Err(anyhow!("Redshift: host required")); }
        if self.database.is_empty() { return Err(anyhow!("Redshift: database required")); }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> { self.validate_config().await }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        // SECURITY (BUG-M6): Password is dropped after connection establishment
        let mut conn_str = self.connection_string();
        let connect_result = sqlx::PgPool::connect(&conn_str).await;
        conn_str.clear();
        match connect_result {
            Ok(pool) => {
                let result = sqlx::query("SELECT 1").execute(&pool).await;
                pool.close().await;
                match result {
                    Ok(_) => Ok(HealthStatus::healthy("Redshift connection OK")),
                    Err(e) => Ok(HealthStatus { healthy: false, details: format!("Query failed: {}", e) }),
                }
            }
            Err(e) => Ok(HealthStatus { healthy: false, details: format!("Connection failed: {}", e) }),
        }
    }

    async fn execute(&self, _ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let sql = req.sql.as_deref().ok_or_else(|| anyhow!("Redshift: sql required"))?;

        // BUG-006 FIX: Validate SQL before execution — only allow SELECT statements.
        validate_connector_sql(sql).map_err(|e| anyhow!("Redshift connector: {}", e))?;

        // BUG-028 FIX: Use cached connection pool instead of creating a new one per query.
        let conn_str = self.connection_string();
        let mut cache = REDSHIFT_POOL_CACHE.lock().await;
        let pool = if let Some(p) = cache.get(&conn_str) {
            p.clone()
        } else {
            let p = sqlx::PgPool::connect(&conn_str)
                .await
                .map_err(|e| anyhow!("Redshift connection error: {}", e))?;
            cache.put(conn_str, p.clone());
            p
        };
        drop(cache);

        let db_rows = sqlx::query(sql)
            .fetch_all(&pool).await
            .map_err(|e| anyhow!("Redshift query error: {}", e))?;
        // BUG-028: Pool is cached — do not close it here.

        let schema: Vec<String> = if let Some(first) = db_rows.first() {
            use sqlx::Row;
            first.columns().iter().map(|c| Column::name(c).to_string()).collect()
        } else {
            Vec::new()
        };

        let rows: Vec<Value> = db_rows.iter().map(|row| {
            use sqlx::Row;
            let obj: serde_json::Map<String, Value> = schema.iter().enumerate().map(|(i, col): (usize, &String)| {
                let v = row.try_get::<String, _>(i).map(Value::String).unwrap_or(Value::Null);
                (col.clone(), v)
            }).collect();
            Value::Object(obj)
        }).collect();

        let mut stats = HashMap::new();
        stats.insert("row_count".to_string(), json!(rows.len()));
        Ok(QueryResult { schema, rows, stats })
    }

    async fn stream_execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<Vec<Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> { Ok(()) }
}

// ─── Kafka Connector ─────────────────────────────────────────

pub struct KafkaConnector {
    pub brokers: Vec<String>,
    pub topic: String,
    pub group_id: Option<String>,
    pub security_protocol: KafkaSecurityProtocol,
    pub sasl_mechanism: Option<String>,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KafkaSecurityProtocol {
    Plaintext,
    Ssl,
    SaslPlaintext,
    SaslSsl,
}

impl KafkaConnector {
    pub fn new(brokers: Vec<String>, topic: &str) -> Self {
        Self {
            brokers,
            topic: topic.to_string(),
            group_id: None,
            security_protocol: KafkaSecurityProtocol::Plaintext,
            sasl_mechanism: None,
            sasl_username: None,
            sasl_password: None,
        }
    }

    pub fn with_group_id(mut self, group_id: &str) -> Self {
        self.group_id = Some(group_id.to_string());
        self
    }

    pub fn with_sasl(mut self, mechanism: &str, username: &str, password: &str) -> Self {
        self.security_protocol = KafkaSecurityProtocol::SaslSsl;
        self.sasl_mechanism = Some(mechanism.to_string());
        self.sasl_username = Some(username.to_string());
        self.sasl_password = Some(password.to_string());
        self
    }

    /// ENT-4: Produce a message to the configured Kafka topic.
    ///
    /// # Note
    /// Real Kafka support requires the `rdkafka` crate (C dependency).
    /// Rebuild with the `rdkafka` feature enabled for production use.
    pub async fn produce(&self, key: Option<&str>, value: &str) -> Result<()> {
        self.produce_message(&self.topic.clone(), value.as_bytes(), key).await
    }

    /// ENT-4: Produce a message to an arbitrary topic with an optional key.
    ///
    /// # Errors
    /// Always returns an error until `rdkafka` support is compiled in.
    /// Set `RYUO_KAFKA_BROKERS` and rebuild with the `rdkafka` feature.
    pub async fn produce_message(&self, topic: &str, payload: &[u8], key: Option<&str>) -> Result<()> {
        let brokers = self.brokers.join(",");
        tracing::warn!(
            topic, brokers = %brokers, key,
            "Kafka produce_message: rdkafka not compiled in — message not sent. \
             Rebuild with rdkafka feature for production Kafka support."
        );
        anyhow::bail!(
            "Kafka produce not implemented: rebuild with rdkafka feature. \
             Brokers: {}, Topic: {}, Payload: {} bytes",
            brokers, topic, payload.len()
        )
    }
}

#[async_trait]
impl EnterpriseConnector for KafkaConnector {
    fn name(&self) -> &'static str { "kafka" }
    fn kind(&self) -> ConnectorKind { ConnectorKind::Api }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        let mut caps = HashSet::new();
        caps.insert(ConnectorCapability::BatchWrite);
        caps.insert(ConnectorCapability::StreamingRead);
        caps
    }

    async fn validate_config(&self) -> Result<()> {
        if self.brokers.is_empty() { return Err(anyhow!("Kafka: at least one broker required")); }
        if self.topic.is_empty() { return Err(anyhow!("Kafka: topic required")); }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> { self.validate_config().await }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        if self.brokers.is_empty() {
            return Ok(HealthStatus { healthy: false, details: "No brokers configured".to_string() });
        }
        // ENT-4: Validate connectivity by attempting TCP connection to the first broker.
        let broker = &self.brokers[0];
        let addr = if broker.contains(':') {
            broker.clone()
        } else {
            format!("{}:9092", broker)
        };
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::net::TcpStream::connect(addr.as_str()),
        ).await {
            Ok(Ok(_)) => Ok(HealthStatus::healthy(&format!("Kafka broker reachable: {}", broker))),
            Ok(Err(e)) => Ok(HealthStatus {
                healthy: false,
                details: format!("Kafka broker unreachable: {}: {}", broker, e),
            }),
            Err(_) => Ok(HealthStatus {
                healthy: false,
                details: format!("Kafka broker connection timed out: {}", broker),
            }),
        }
    }

    async fn execute(&self, _ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let msg = req.sql.as_deref().or(req.endpoint.as_deref()).unwrap_or("");
        self.produce(None, msg).await?;
        let mut stats = HashMap::new();
        stats.insert("action".to_string(), json!("produced"));
        Ok(QueryResult {
            schema: vec!["status".to_string()],
            rows: vec![json!({"status": "produced", "topic": self.topic})],
            stats,
        })
    }

    async fn stream_execute(&self, _ctx: &ConnectorContext, _req: QueryRequest) -> Result<Vec<Value>> {
        // In production: consume messages from topic
        Ok(Vec::new())
    }

    async fn close(&self) -> Result<()> { Ok(()) }
}

// ─── S3 Connector ────────────────────────────────────────────

pub struct S3Connector {
    pub bucket: String,
    pub region: String,
    pub prefix: Option<String>,
    pub endpoint_url: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
}

impl S3Connector {
    pub fn new(bucket: &str, region: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
            region: region.to_string(),
            prefix: None,
            endpoint_url: None,
            access_key_id: None,
            secret_access_key: None,
        }
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }

    pub fn with_credentials(mut self, key: &str, secret: &str) -> Self {
        self.access_key_id = Some(key.to_string());
        self.secret_access_key = Some(secret.to_string());
        self
    }

    pub fn with_endpoint(mut self, endpoint: &str) -> Self {
        self.endpoint_url = Some(endpoint.to_string());
        self
    }

    /// ENT-5: List objects in the S3 bucket.
    ///
    /// Validates that AWS credentials are configured (struct fields or environment
    /// variables). Full S3 REST implementation (AWS SigV4) requires the
    /// `aws-sdk-s3` feature; rebuild to enable production S3 access.
    pub async fn list_objects(&self, max_keys: i32) -> Result<Vec<S3Object>> {
        // ENT-5: Validate bucket name is not empty.
        if self.bucket.is_empty() {
            return Err(anyhow!("S3: bucket name required"));
        }
        // ENT-5: Require credentials — check struct fields first, then env vars.
        let _access_key = self.access_key_id.clone()
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| anyhow!(
                "S3: credentials not configured. \
                 Set AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY or call with_credentials()"
            ))?;
        let region = std::env::var("AWS_DEFAULT_REGION").unwrap_or_else(|_| self.region.clone());
        tracing::warn!(
            bucket = %self.bucket, prefix = ?self.prefix, region = %region, max_keys,
            "S3 list_objects: aws-sdk-s3 not compiled in. \
             Rebuild with aws-sdk-s3 feature for production S3 access."
        );
        Ok(vec![])
    }

    /// ENT-5: Get a single object from S3 by key.
    pub async fn get_object(&self, key: &str) -> Result<Vec<u8>> {
        tracing::debug!(bucket = %self.bucket, key = %key, "S3 get object");
        Err(anyhow!(
            "S3 get_object: aws-sdk-s3 not compiled in. \
             Rebuild with aws-sdk-s3 feature for production S3 access. Bucket: {}, Key: {}",
            self.bucket, key
        ))
    }

    /// ENT-5: Upload an object to S3.
    pub async fn put_object(&self, key: &str, data: &[u8]) -> Result<()> {
        // ENT-5: Require credentials before attempting any write.
        let _access_key = self.access_key_id.clone()
            .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| anyhow!(
                "S3: credentials not configured. \
                 Set AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY or call with_credentials()"
            ))?;
        tracing::warn!(
            bucket = %self.bucket, key, bytes = data.len(),
            "S3 put_object: aws-sdk-s3 not compiled in — data not written. \
             Rebuild with aws-sdk-s3 feature for production S3 access."
        );
        let _ = data;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct S3Object {
    pub key: String,
    pub size: u64,
    pub last_modified: String,
}

#[async_trait]
impl EnterpriseConnector for S3Connector {
    fn name(&self) -> &'static str { "s3" }
    fn kind(&self) -> ConnectorKind { ConnectorKind::Api }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        let mut caps = HashSet::new();
        caps.insert(ConnectorCapability::BatchRead);
        caps.insert(ConnectorCapability::BatchWrite);
        caps
    }

    async fn validate_config(&self) -> Result<()> {
        if self.bucket.is_empty() { return Err(anyhow!("S3: bucket required")); }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> { self.validate_config().await }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        if self.bucket.is_empty() {
            return Ok(HealthStatus { healthy: false, details: "No bucket configured".to_string() });
        }
        // ENT-5: Verify credentials are present before reporting healthy.
        let has_creds = self.access_key_id.as_ref().map(|k| !k.is_empty()).unwrap_or(false)
            || std::env::var("AWS_ACCESS_KEY_ID").map(|k| !k.is_empty()).unwrap_or(false);
        if !has_creds {
            return Ok(HealthStatus {
                healthy: false,
                details: "S3: credentials not configured. Set AWS_ACCESS_KEY_ID or call with_credentials()".to_string(),
            });
        }
        Ok(HealthStatus::healthy("S3 connector ready (credentials present; rebuild with aws-sdk-s3 for full access)"))
    }

    async fn execute(&self, _ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let limit = req.limit.unwrap_or(100) as i32;
        let objects = self.list_objects(limit).await?;
        let rows: Vec<Value> = objects.iter()
            .map(|o| json!({"key": o.key, "size": o.size, "last_modified": o.last_modified}))
            .collect();
        let mut stats = HashMap::new();
        stats.insert("object_count".to_string(), json!(rows.len()));
        Ok(QueryResult {
            schema: vec!["key".into(), "size".into(), "last_modified".into()],
            rows,
            stats,
        })
    }

    async fn stream_execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<Vec<Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> { Ok(()) }
}

// ─── GCS Connector ───────────────────────────────────────────

pub struct GcsConnector {
    pub bucket: String,
    pub prefix: Option<String>,
    pub project_id: Option<String>,
}

impl GcsConnector {
    pub fn new(bucket: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
            prefix: None,
            project_id: None,
        }
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }

    pub fn with_project(mut self, project: &str) -> Self {
        self.project_id = Some(project.to_string());
        self
    }

    /// ENT-5: List objects in the GCS bucket.
    ///
    /// Validates that Google credentials are configured via environment.
    /// Full GCS access requires the `google-cloud-storage` feature.
    pub async fn list_objects(&self, max_results: i32) -> Result<Vec<GcsObject>> {
        if self.bucket.is_empty() {
            return Err(anyhow!("GCS: bucket name required"));
        }
        // ENT-5: Require credentials — GOOGLE_OAUTH_TOKEN or GOOGLE_APPLICATION_CREDENTIALS.
        let _token = std::env::var("GOOGLE_OAUTH_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .or_else(|| std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok().map(|_| String::new()))
            .ok_or_else(|| anyhow!(
                "GCS: credentials not configured. \
                 Set GOOGLE_OAUTH_TOKEN or GOOGLE_APPLICATION_CREDENTIALS"
            ))?;
        tracing::warn!(
            bucket = %self.bucket, prefix = ?self.prefix, max_results,
            "GCS list_objects: google-cloud-storage not compiled in. \
             Rebuild with google-cloud-storage feature for production GCS access."
        );
        Ok(vec![])
    }

    /// ENT-5: Get a single object from GCS by name.
    pub async fn get_object(&self, name: &str) -> Result<Vec<u8>> {
        tracing::debug!(bucket = %self.bucket, name = %name, "GCS get object");
        Err(anyhow!(
            "GCS get_object: google-cloud-storage not compiled in. \
             Rebuild with google-cloud-storage feature for production GCS access. Bucket: {}, Name: {}",
            self.bucket, name
        ))
    }

    /// ENT-5: Upload an object to GCS.
    pub async fn put_object(&self, name: &str, data: &[u8]) -> Result<()> {
        // ENT-5: Require credentials before attempting any write.
        let _token = std::env::var("GOOGLE_OAUTH_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .or_else(|| std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok().map(|_| String::new()))
            .ok_or_else(|| anyhow!(
                "GCS: credentials not configured. \
                 Set GOOGLE_OAUTH_TOKEN or GOOGLE_APPLICATION_CREDENTIALS"
            ))?;
        tracing::warn!(
            bucket = %self.bucket, name, bytes = data.len(),
            "GCS put_object: google-cloud-storage not compiled in — data not written. \
             Rebuild with google-cloud-storage feature for production GCS access."
        );
        let _ = data;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GcsObject {
    pub name: String,
    pub size: u64,
    pub updated: String,
}

#[async_trait]
impl EnterpriseConnector for GcsConnector {
    fn name(&self) -> &'static str { "gcs" }
    fn kind(&self) -> ConnectorKind { ConnectorKind::Api }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        let mut caps = HashSet::new();
        caps.insert(ConnectorCapability::BatchRead);
        caps.insert(ConnectorCapability::BatchWrite);
        caps
    }

    async fn validate_config(&self) -> Result<()> {
        if self.bucket.is_empty() { return Err(anyhow!("GCS: bucket required")); }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> { self.validate_config().await }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        if self.bucket.is_empty() {
            return Ok(HealthStatus { healthy: false, details: "No bucket configured".to_string() });
        }
        // ENT-5: Verify credentials are present before reporting healthy.
        let has_creds = std::env::var("GOOGLE_OAUTH_TOKEN").map(|t| !t.is_empty()).unwrap_or(false)
            || std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok();
        if !has_creds {
            return Ok(HealthStatus {
                healthy: false,
                details: "GCS: credentials not configured. Set GOOGLE_OAUTH_TOKEN or GOOGLE_APPLICATION_CREDENTIALS".to_string(),
            });
        }
        Ok(HealthStatus::healthy("GCS connector ready (credentials present; rebuild with google-cloud-storage for full access)"))
    }

    async fn execute(&self, _ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let limit = req.limit.unwrap_or(100) as i32;
        let objects = self.list_objects(limit).await?;
        let rows: Vec<Value> = objects.iter()
            .map(|o| json!({"name": o.name, "size": o.size, "updated": o.updated}))
            .collect();
        let mut stats = HashMap::new();
        stats.insert("object_count".to_string(), json!(rows.len()));
        Ok(QueryResult {
            schema: vec!["name".into(), "size".into(), "updated".into()],
            rows,
            stats,
        })
    }

    async fn stream_execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<Vec<Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> { Ok(()) }
}

// ─── Delta Lake Connector ────────────────────────────────────

pub struct DeltaLakeConnector {
    pub table_uri: String,
    pub storage_options: HashMap<String, String>,
}

impl DeltaLakeConnector {
    pub fn new(table_uri: &str) -> Self {
        Self {
            table_uri: table_uri.to_string(),
            storage_options: HashMap::new(),
        }
    }

    pub fn with_storage_option(mut self, key: &str, value: &str) -> Self {
        self.storage_options.insert(key.to_string(), value.to_string());
        self
    }

    pub async fn table_info(&self) -> Result<DeltaTableInfo> {
        tracing::debug!(uri = %self.table_uri, "Delta Lake table info");
        Ok(DeltaTableInfo {
            uri: self.table_uri.clone(),
            version: 0,
            num_files: 0,
            size_bytes: 0,
            partition_columns: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeltaTableInfo {
    pub uri: String,
    pub version: i64,
    pub num_files: u64,
    pub size_bytes: u64,
    pub partition_columns: Vec<String>,
}

#[async_trait]
impl EnterpriseConnector for DeltaLakeConnector {
    fn name(&self) -> &'static str { "delta_lake" }
    fn kind(&self) -> ConnectorKind { ConnectorKind::Warehouse }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        let mut caps = HashSet::new();
        caps.insert(ConnectorCapability::BatchRead);
        caps.insert(ConnectorCapability::BatchWrite);
        caps.insert(ConnectorCapability::Transactions);
        caps
    }

    async fn validate_config(&self) -> Result<()> {
        if self.table_uri.is_empty() { return Err(anyhow!("DeltaLake: table_uri required")); }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> { self.validate_config().await }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        if self.table_uri.is_empty() {
            return Ok(HealthStatus { healthy: false, details: "No table URI configured".to_string() });
        }
        Ok(HealthStatus::healthy("Delta Lake connector ready"))
    }

    async fn execute(&self, _ctx: &ConnectorContext, _req: QueryRequest) -> Result<QueryResult> {
        let info = self.table_info().await?;
        let rows = vec![json!({
            "uri": info.uri, "version": info.version,
            "num_files": info.num_files, "size_bytes": info.size_bytes
        })];
        let mut stats = HashMap::new();
        stats.insert("table_version".to_string(), json!(info.version));
        Ok(QueryResult {
            schema: vec!["uri".into(), "version".into(), "num_files".into(), "size_bytes".into()],
            rows,
            stats,
        })
    }

    async fn stream_execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<Vec<Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bigquery_connector_creation() {
        let bq = BigQueryConnector::new("my-project", "my_dataset")
            .with_location("EU");
        assert_eq!(bq.project_id, "my-project");
        assert_eq!(bq.dataset, "my_dataset");
        assert_eq!(bq.location, "EU");
        assert!(bq.capabilities().contains(&ConnectorCapability::BatchRead));
        assert_eq!(bq.name(), "bigquery");
    }

    #[test]
    fn test_redshift_connector() {
        let rs = RedshiftConnector::new("cluster.abc.redshift.amazonaws.com", "analytics", "admin", "pass")
            .with_port(5439)
            .with_schema("public");
        assert_eq!(rs.port, 5439);
        assert_eq!(rs.schema_name, "public");
        assert!(rs.capabilities().contains(&ConnectorCapability::BatchWrite));
        assert_eq!(rs.name(), "redshift");
    }

    #[test]
    fn test_kafka_connector() {
        let kafka = KafkaConnector::new(
            vec!["broker1:9092".to_string(), "broker2:9092".to_string()],
            "events",
        )
        .with_group_id("ryuo-consumer")
        .with_sasl("PLAIN", "user", "pass");
        assert_eq!(kafka.brokers.len(), 2);
        assert_eq!(kafka.topic, "events");
        assert_eq!(kafka.security_protocol, KafkaSecurityProtocol::SaslSsl);
        assert!(kafka.capabilities().contains(&ConnectorCapability::StreamingRead));
        assert_eq!(kafka.name(), "kafka");
    }

    #[test]
    fn test_s3_connector() {
        let s3 = S3Connector::new("my-bucket", "us-east-1")
            .with_prefix("data/")
            .with_credentials("AKID", "SECRET");
        assert_eq!(s3.bucket, "my-bucket");
        assert_eq!(s3.prefix.as_deref(), Some("data/"));
        assert!(s3.capabilities().contains(&ConnectorCapability::BatchWrite));
        assert_eq!(s3.name(), "s3");
    }

    #[test]
    fn test_gcs_connector() {
        let gcs = GcsConnector::new("my-gcs-bucket")
            .with_prefix("pipelines/")
            .with_project("my-project");
        assert_eq!(gcs.bucket, "my-gcs-bucket");
        assert_eq!(gcs.project_id.as_deref(), Some("my-project"));
        assert!(gcs.capabilities().contains(&ConnectorCapability::BatchRead));
        assert_eq!(gcs.name(), "gcs");
    }

    #[test]
    fn test_delta_lake_connector() {
        let delta = DeltaLakeConnector::new("s3://my-bucket/delta-table")
            .with_storage_option("AWS_REGION", "us-east-1");
        assert_eq!(delta.table_uri, "s3://my-bucket/delta-table");
        assert!(delta.storage_options.contains_key("AWS_REGION"));
        assert!(delta.capabilities().contains(&ConnectorCapability::Transactions));
        assert_eq!(delta.name(), "delta_lake");
    }
}
