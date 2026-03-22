use crate::enterprise_connector::{
    ConnectorCapability, ConnectorContext, ConnectorKind, EnterpriseConnector, HealthStatus,
    QueryRequest, QueryResult,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Column, PgPool, Row};
use std::collections::HashSet;
use std::future::Future;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, sleep};

async fn with_retry<T, F, Fut>(ctx: &ConnectorContext, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let max_attempts = ctx.retry_policy.max_attempts.max(1);
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..max_attempts {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_error = Some(e);
                if attempt + 1 < max_attempts {
                    let backoff = ctx.retry_policy.base_backoff_ms * (1_u64 << attempt);
                    sleep(Duration::from_millis(backoff)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("retry failed with unknown error")))
}

fn typed_value(raw: &str) -> Value {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("null") || s.is_empty() {
        Value::Null
    } else if s.eq_ignore_ascii_case("true") {
        Value::Bool(true)
    } else if s.eq_ignore_ascii_case("false") {
        Value::Bool(false)
    } else if let Ok(v) = s.parse::<i64>() {
        json!(v)
    } else if let Ok(v) = s.parse::<f64>() {
        json!(v)
    } else {
        json!(s)
    }
}

fn auth_token(ctx: &ConnectorContext) -> Option<String> {
    if let Some(t) = &ctx.auth.token {
        return Some(t.clone());
    }
    None
}

pub struct PostgresEnterpriseConnector {
    pool: PgPool,
}

impl PostgresEnterpriseConnector {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn map_row(row: &sqlx::postgres::PgRow) -> Value {
        let mut out = serde_json::Map::new();
        for (idx, col) in row.columns().iter().enumerate() {
            let name = col.name().to_string();
            let v = if let Ok(val) = row.try_get::<Option<i64>, _>(idx) {
                val.map_or(Value::Null, |x| json!(x))
            } else if let Ok(val) = row.try_get::<Option<f64>, _>(idx) {
                val.map_or(Value::Null, |x| json!(x))
            } else if let Ok(val) = row.try_get::<Option<bool>, _>(idx) {
                val.map_or(Value::Null, Value::Bool)
            } else if let Ok(val) = row.try_get::<Option<String>, _>(idx) {
                val.map_or(Value::Null, |x| json!(x))
            } else {
                json!("<unsupported_type>")
            };
            out.insert(name, v);
        }
        Value::Object(out)
    }
}

#[async_trait]
impl EnterpriseConnector for PostgresEnterpriseConnector {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Database
    }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        [
            ConnectorCapability::Transactions,
            ConnectorCapability::BatchRead,
            ConnectorCapability::BatchWrite,
            ConnectorCapability::StreamingRead,
            ConnectorCapability::PushdownPredicates,
        ]
        .into_iter()
        .collect()
    }

    async fn validate_config(&self) -> Result<()> {
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("Postgres health check failed")?;
        Ok(HealthStatus::healthy("postgres ok"))
    }

    async fn execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let sql = req.sql.ok_or_else(|| anyhow!("Missing SQL in request"))?;
        let rows = with_retry(ctx, || {
            let sql = sql.clone();
            async move { Ok(sqlx::query(&sql).fetch_all(&self.pool).await?) }
        })
        .await?;

        let mapped_rows = rows.iter().map(Self::map_row).collect::<Vec<_>>();
        Ok(QueryResult {
            schema: Vec::new(),
            rows: mapped_rows,
            stats: [
                ("row_count".to_string(), json!(rows.len())),
                ("connector".to_string(), json!("postgres")),
            ]
            .into_iter()
            .collect(),
        })
    }

    async fn stream_execute(
        &self,
        ctx: &ConnectorContext,
        req: QueryRequest,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct SnowflakeRowType {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SnowflakeResultMeta {
    #[serde(default, rename = "rowType")]
    row_type: Vec<SnowflakeRowType>,
}

#[derive(Debug, Deserialize)]
struct SnowflakeResponse {
    #[serde(default, rename = "statementHandle")]
    statement_handle: Option<String>,
    #[serde(default)]
    data: Vec<Vec<Value>>,
    #[serde(default, rename = "resultSetMetaData")]
    result_set_meta_data: Option<SnowflakeResultMeta>,
    #[serde(default, rename = "statementStatusUrl")]
    statement_status_url: Option<String>,
    #[serde(default, rename = "nextUri")]
    next_uri: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

pub struct SnowflakeConnector {
    pub account: String,
    pub warehouse: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
}

impl SnowflakeConnector {
    fn base_url(&self) -> String {
        format!("https://{}.snowflakecomputing.com", self.account)
    }

    fn build_headers(&self, ctx: &ConnectorContext) -> Result<HeaderMap> {
        let token = auth_token(ctx).ok_or_else(|| anyhow!("Snowflake token is required"))?;
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .context("invalid snowflake auth token")?,
        );
        Ok(headers)
    }

    fn map_rows(schema: &[String], rows: &[Vec<Value>]) -> Vec<Value> {
        rows.iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                for (i, val) in r.iter().enumerate() {
                    let key = schema
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("col_{}", i));
                    m.insert(key, val.clone());
                }
                Value::Object(m)
            })
            .collect()
    }
}

#[async_trait]
impl EnterpriseConnector for SnowflakeConnector {
    fn name(&self) -> &'static str {
        "snowflake"
    }

    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Warehouse
    }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        [
            ConnectorCapability::BatchRead,
            ConnectorCapability::ArrowZeroCopy,
            ConnectorCapability::AsyncJobs,
            ConnectorCapability::PushdownPredicates,
        ]
        .into_iter()
        .collect()
    }

    async fn validate_config(&self) -> Result<()> {
        if self.account.trim().is_empty() {
            return Err(anyhow!("Snowflake account cannot be empty"));
        }
        Ok(())
    }

    async fn connect(&self, ctx: &ConnectorContext) -> Result<()> {
        self.validate_config().await?;
        let _ = self.build_headers(ctx)?;
        Ok(())
    }

    async fn health_check(&self, ctx: &ConnectorContext) -> Result<HealthStatus> {
        self.connect(ctx).await?;
        Ok(HealthStatus::healthy("snowflake connector ready"))
    }

    async fn execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let sql = req.sql.ok_or_else(|| anyhow!("Missing SQL in request"))?;
        let headers = self.build_headers(ctx)?;
        let client = reqwest::Client::new();
        let base = self.base_url();

        let mut body = serde_json::Map::new();
        body.insert("statement".to_string(), json!(sql));
        if let Some(w) = &self.warehouse {
            body.insert("warehouse".to_string(), json!(w));
        }
        if let Some(d) = &self.database {
            body.insert("database".to_string(), json!(d));
        }
        if let Some(s) = &self.schema {
            body.insert("schema".to_string(), json!(s));
        }

        let first: SnowflakeResponse = with_retry(ctx, || {
            let client = client.clone();
            let headers = headers.clone();
            let body = body.clone();
            let url = format!("{}/api/v2/statements", base);
            async move {
                let res = client.post(url).headers(headers).json(&body).send().await?;
                let status = res.status();
                let payload: SnowflakeResponse = res.json().await?;
                if !status.is_success() {
                    return Err(anyhow!(
                        "Snowflake query submit failed: code={:?} message={:?}",
                        payload.code,
                        payload.message
                    ));
                }
                Ok(payload)
            }
        })
        .await?;

        let mut schema = first
            .result_set_meta_data
            .as_ref()
            .map(|m| m.row_type.iter().map(|x| x.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default();

        let mut raw_rows = first.data;
        let mut next_uri = first.next_uri;
        let mut pages = 1usize;

        // If query was async, poll status endpoint once.
        if raw_rows.is_empty()
            && first.statement_handle.is_some()
            && first.statement_status_url.is_some()
        {
            let status_url = format!("{}{}", base, first.statement_status_url.unwrap_or_default());
            let polled: SnowflakeResponse = with_retry(ctx, || {
                let client = client.clone();
                let headers = headers.clone();
                let status_url = status_url.clone();
                async move {
                    let res = client.get(status_url).headers(headers).send().await?;
                    Ok(res.json().await?)
                }
            })
            .await?;

            if schema.is_empty() {
                schema = polled
                    .result_set_meta_data
                    .as_ref()
                    .map(|m| m.row_type.iter().map(|x| x.name.clone()).collect::<Vec<_>>())
                    .unwrap_or_default();
            }
            raw_rows.extend(polled.data);
            next_uri = polled.next_uri;
        }

        while let Some(uri) = next_uri {
            let page: SnowflakeResponse = with_retry(ctx, || {
                let client = client.clone();
                let headers = headers.clone();
                let url = format!("{}{}", base, uri);
                async move {
                    let res = client.get(url).headers(headers).send().await?;
                    Ok(res.json().await?)
                }
            })
            .await?;
            raw_rows.extend(page.data);
            next_uri = page.next_uri;
            pages += 1;
        }

        let rows = SnowflakeConnector::map_rows(&schema, &raw_rows);
        Ok(QueryResult {
            schema,
            rows,
            stats: [
                ("connector".to_string(), json!("snowflake")),
                ("row_count".to_string(), json!(raw_rows.len())),
                ("pages".to_string(), json!(pages)),
            ]
            .into_iter()
            .collect(),
        })
    }

    async fn stream_execute(
        &self,
        ctx: &ConnectorContext,
        req: QueryRequest,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct DatabricksColumn {
    name: String,
}

#[derive(Debug, Deserialize)]
struct DatabricksResultData {
    #[serde(default)]
    data_array: Vec<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct DatabricksResultManifest {
    #[serde(default)]
    schema: Option<DatabricksSchema>,
}

#[derive(Debug, Deserialize)]
struct DatabricksSchema {
    #[serde(default)]
    columns: Vec<DatabricksColumn>,
}

#[derive(Debug, Deserialize)]
struct DatabricksSqlResponse {
    #[serde(default)]
    statement_id: Option<String>,
    #[serde(default)]
    status: Option<DatabricksStatus>,
    #[serde(default)]
    result: Option<DatabricksResultData>,
    #[serde(default)]
    manifest: Option<DatabricksResultManifest>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DatabricksStatus {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<DatabricksError>,
}

#[derive(Debug, Deserialize)]
struct DatabricksError {
    #[serde(default)]
    message: Option<String>,
}

pub struct DatabricksConnector {
    pub workspace_url: String,
    pub warehouse_id: Option<String>,
}

impl DatabricksConnector {
    fn map_rows(columns: &[String], rows: &[Vec<Value>]) -> Vec<Value> {
        rows.iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                for (i, val) in r.iter().enumerate() {
                    let name = columns
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("col_{}", i));
                    m.insert(name, val.clone());
                }
                Value::Object(m)
            })
            .collect()
    }
}

#[async_trait]
impl EnterpriseConnector for DatabricksConnector {
    fn name(&self) -> &'static str {
        "databricks"
    }

    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Warehouse
    }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        [
            ConnectorCapability::AsyncJobs,
            ConnectorCapability::BatchRead,
            ConnectorCapability::PushdownPredicates,
        ]
        .into_iter()
        .collect()
    }

    async fn validate_config(&self) -> Result<()> {
        if !self.workspace_url.starts_with("http") {
            return Err(anyhow!("Databricks workspace URL must start with http"));
        }
        Ok(())
    }

    async fn connect(&self, ctx: &ConnectorContext) -> Result<()> {
        self.validate_config().await?;
        if auth_token(ctx).is_none() {
            return Err(anyhow!("Databricks auth token required"));
        }
        Ok(())
    }

    async fn health_check(&self, ctx: &ConnectorContext) -> Result<HealthStatus> {
        self.connect(ctx).await?;
        Ok(HealthStatus::healthy("databricks connector ready"))
    }

    async fn execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let token = auth_token(ctx).ok_or_else(|| anyhow!("Databricks auth token required"))?;
        let client = reqwest::Client::new();

        if let Some(sql) = req.sql {
            let wh = req
                .params
                .get("warehouse_id")
                .and_then(|v| v.as_str())
                .map(|x| x.to_string())
                .or_else(|| self.warehouse_id.clone())
                .ok_or_else(|| anyhow!("warehouse_id is required for Databricks SQL"))?;

            let submit_url = format!("{}/api/2.0/sql/statements", self.workspace_url.trim_end_matches('/'));
            let submit_body = json!({
                "statement": sql,
                "warehouse_id": wh,
            });

            let mut response: DatabricksSqlResponse = with_retry(ctx, || {
                let client = client.clone();
                let submit_url = submit_url.clone();
                let submit_body = submit_body.clone();
                let token = token.clone();
                async move {
                    let res = client
                        .post(submit_url)
                        .bearer_auth(token)
                        .json(&submit_body)
                        .send()
                        .await?;
                    Ok(res.json().await?)
                }
            })
            .await?;

            let statement_id = response
                .statement_id
                .clone()
                .ok_or_else(|| anyhow!("Databricks response missing statement_id"))?;

            let mut poll_count = 0;
            while response
                .status
                .as_ref()
                .and_then(|s| s.state.clone())
                .map(|s| s == "PENDING" || s == "RUNNING")
                .unwrap_or(false)
                && poll_count < 60
            {
                poll_count += 1;
                sleep(Duration::from_millis(500)).await;
                let poll_url = format!(
                    "{}/api/2.0/sql/statements/{}",
                    self.workspace_url.trim_end_matches('/'),
                    statement_id
                );
                response = with_retry(ctx, || {
                    let client = client.clone();
                    let poll_url = poll_url.clone();
                    let token = token.clone();
                    async move {
                        let res = client.get(poll_url).bearer_auth(token).send().await?;
                        Ok(res.json().await?)
                    }
                })
                .await?;
            }

            if let Some(status) = &response.status {
                if let Some(state) = &status.state {
                    if state == "FAILED" || state == "CANCELED" {
                        return Err(anyhow!(
                            "Databricks SQL failed: {}",
                            status
                                .error
                                .as_ref()
                                .and_then(|e| e.message.clone())
                                .unwrap_or_else(|| "unknown error".to_string())
                        ));
                    }
                }
            }

            let columns = response
                .manifest
                .as_ref()
                .and_then(|m| m.schema.as_ref())
                .map(|s| s.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
                .unwrap_or_default();

            let mut rows = response
                .result
                .as_ref()
                .map(|r| r.data_array.clone())
                .unwrap_or_default();
            let mut next_page = response.next_page_token;
            let mut pages = 1usize;

            while let Some(page_token) = next_page {
                let page_url = format!(
                    "{}/api/2.0/sql/statements/{}/result/chunks/{}",
                    self.workspace_url.trim_end_matches('/'),
                    statement_id,
                    page_token
                );
                let page: DatabricksSqlResponse = with_retry(ctx, || {
                    let client = client.clone();
                    let page_url = page_url.clone();
                    let token = token.clone();
                    async move {
                        let res = client.get(page_url).bearer_auth(token).send().await?;
                        Ok(res.json().await?)
                    }
                })
                .await?;
                rows.extend(page.result.map(|r| r.data_array).unwrap_or_default());
                next_page = page.next_page_token;
                pages += 1;
            }

            let mapped_rows = DatabricksConnector::map_rows(&columns, &rows);
            return Ok(QueryResult {
                schema: columns,
                rows: mapped_rows,
                stats: [
                    ("connector".to_string(), json!("databricks")),
                    ("pages".to_string(), json!(pages)),
                    ("row_count".to_string(), json!(rows.len())),
                ]
                .into_iter()
                .collect(),
            });
        }

        // Jobs API path
        let job_id = req
            .params
            .get("job_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow!("Databricks job path requires params.job_id"))?;
        let run_now_url = format!("{}/api/2.1/jobs/run-now", self.workspace_url.trim_end_matches('/'));
        let run_body = json!({ "job_id": job_id });

        let run_payload: Value = with_retry(ctx, || {
            let client = client.clone();
            let run_now_url = run_now_url.clone();
            let run_body = run_body.clone();
            let token = token.clone();
            async move {
                let res = client
                    .post(run_now_url)
                    .bearer_auth(token)
                    .json(&run_body)
                    .send()
                    .await?;
                Ok(res.json().await?)
            }
        })
        .await?;

        Ok(QueryResult {
            schema: vec!["run_id".to_string()],
            rows: vec![run_payload],
            stats: [
                ("connector".to_string(), json!("databricks")),
                ("mode".to_string(), json!("jobs-run-now")),
            ]
            .into_iter()
            .collect(),
        })
    }

    async fn stream_execute(
        &self,
        ctx: &ConnectorContext,
        req: QueryRequest,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

pub struct DbtConnector;

#[async_trait]
impl EnterpriseConnector for DbtConnector {
    fn name(&self) -> &'static str {
        "dbt"
    }

    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Transformation
    }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        [ConnectorCapability::AsyncJobs].into_iter().collect()
    }

    async fn validate_config(&self) -> Result<()> {
        let out = Command::new("dbt")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        if out.is_err() {
            return Err(anyhow!("dbt binary not found in PATH"));
        }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> {
        self.validate_config().await
    }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        Ok(HealthStatus::healthy("dbt shell connector ready"))
    }

    async fn execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let action = req
            .params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("run");
        let project_dir = req
            .params
            .get("project_dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("project_dir is required"))?;
        let profiles_dir = req
            .params
            .get("profiles_dir")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("profiles_dir is required"))?;

        let output = with_retry(ctx, || {
            let action = action.to_string();
            let project_dir = project_dir.to_string();
            let profiles_dir = profiles_dir.to_string();
            async move {
                Ok(Command::new("dbt")
                    .arg(action)
                    .arg("--project-dir")
                    .arg(project_dir)
                    .arg("--profiles-dir")
                    .arg(profiles_dir)
                    .arg("--log-format")
                    .arg("json")
                    .output()
                    .await?)
            }
        })
        .await
        .context("Failed to execute dbt")?;

        if !output.status.success() {
            return Err(anyhow!(
                "dbt {} failed: {}",
                action,
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(QueryResult {
            schema: vec!["status".to_string(), "stdout".to_string()],
            rows: vec![json!({
                "status": "ok",
                "stdout": String::from_utf8_lossy(&output.stdout)
            })],
            stats: [
                ("action".to_string(), json!(action)),
                ("connector".to_string(), json!("dbt")),
            ]
            .into_iter()
            .collect(),
        })
    }

    async fn stream_execute(
        &self,
        ctx: &ConnectorContext,
        req: QueryRequest,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

pub struct MySqlConnector {
    pub host: String,
    pub port: u16,
    pub database: String,
}

#[async_trait]
impl EnterpriseConnector for MySqlConnector {
    fn name(&self) -> &'static str {
        "mysql"
    }

    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Database
    }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        [
            ConnectorCapability::BatchRead,
            ConnectorCapability::BatchWrite,
            ConnectorCapability::StreamingRead,
            ConnectorCapability::PushdownPredicates,
        ]
        .into_iter()
        .collect()
    }

    async fn validate_config(&self) -> Result<()> {
        if self.host.is_empty() || self.database.is_empty() {
            return Err(anyhow!("MySQL host and database are required"));
        }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> {
        self.validate_config().await
    }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        Ok(HealthStatus::healthy("mysql command protocol ready"))
    }

    async fn execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let mut sql = req.sql.ok_or_else(|| anyhow!("Missing SQL for MySQL connector"))?;
        let offset = req
            .params
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if let Some(limit) = req.limit {
            sql = format!("{} LIMIT {} OFFSET {}", sql, limit, offset);
        }

        let user = ctx
            .auth
            .username
            .clone()
            .unwrap_or_else(|| "root".to_string());

        let output = with_retry(ctx, || {
            let sql = sql.clone();
            let user = user.clone();
            let host = self.host.clone();
            let port = self.port;
            let database = self.database.clone();
            let password = ctx.auth.password.clone();
            async move {
                let mut cmd = Command::new("mysql");
                cmd.arg("-h")
                    .arg(host)
                    .arg("-P")
                    .arg(port.to_string())
                    .arg("-u")
                    .arg(user)
                    .arg("-B")
                    .arg(database)
                    .arg("-e")
                    .arg(sql)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if let Some(p) = password {
                    cmd.env("MYSQL_PWD", p);
                }
                Ok(cmd.output().await?)
            }
        })
        .await?;

        if !output.status.success() {
            return Err(anyhow!(
                "mysql command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines();
        let headers = lines
            .next()
            .unwrap_or_default()
            .split('\t')
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        for line in lines {
            let mut obj = serde_json::Map::new();
            for (i, cell) in line.split('\t').enumerate() {
                let key = headers
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("col_{}", i));
                obj.insert(key, typed_value(cell));
            }
            rows.push(Value::Object(obj));
        }

        let row_count = rows.len();
        Ok(QueryResult {
            schema: headers,
            rows,
            stats: [
                ("connector".to_string(), json!("mysql")),
                ("row_count".to_string(), json!(row_count)),
            ]
            .into_iter()
            .collect(),
        })
    }

    async fn stream_execute(
        &self,
        ctx: &ConnectorContext,
        req: QueryRequest,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

pub struct MsSqlConnector {
    pub server: String,
    pub database: String,
}

#[async_trait]
impl EnterpriseConnector for MsSqlConnector {
    fn name(&self) -> &'static str {
        "mssql"
    }

    fn kind(&self) -> ConnectorKind {
        ConnectorKind::Database
    }

    fn capabilities(&self) -> HashSet<ConnectorCapability> {
        [
            ConnectorCapability::BatchRead,
            ConnectorCapability::BatchWrite,
            ConnectorCapability::StreamingRead,
            ConnectorCapability::PushdownPredicates,
        ]
        .into_iter()
        .collect()
    }

    async fn validate_config(&self) -> Result<()> {
        if self.server.is_empty() || self.database.is_empty() {
            return Err(anyhow!("MSSQL server and database are required"));
        }
        Ok(())
    }

    async fn connect(&self, _ctx: &ConnectorContext) -> Result<()> {
        self.validate_config().await
    }

    async fn health_check(&self, _ctx: &ConnectorContext) -> Result<HealthStatus> {
        Ok(HealthStatus::healthy("mssql command protocol ready"))
    }

    async fn execute(&self, ctx: &ConnectorContext, req: QueryRequest) -> Result<QueryResult> {
        let mut sql = req.sql.ok_or_else(|| anyhow!("Missing SQL for MSSQL connector"))?;
        let offset = req
            .params
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if let Some(limit) = req.limit {
            // SQL Server OFFSET/FETCH syntax
            sql = format!(
                "{} OFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
                sql, offset, limit
            );
        }

        let user = ctx
            .auth
            .username
            .clone()
            .ok_or_else(|| anyhow!("MSSQL username required"))?;
        let pass = ctx
            .auth
            .password
            .clone()
            .ok_or_else(|| anyhow!("MSSQL password required"))?;

        let output = with_retry(ctx, || {
            let sql = sql.clone();
            let user = user.clone();
            let pass = pass.clone();
            let server = self.server.clone();
            let database = self.database.clone();
            async move {
                Ok(Command::new("sqlcmd")
                    .arg("-S")
                    .arg(server)
                    .arg("-d")
                    .arg(database)
                    .arg("-U")
                    .arg(user)
                    .arg("-P")
                    .arg(pass)
                    .arg("-s")
                    .arg("\t")
                    .arg("-W")
                    .arg("-Q")
                    .arg(sql)
                    .output()
                    .await?)
            }
        })
        .await?;

        if !output.status.success() {
            return Err(anyhow!(
                "sqlcmd failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
        let headers = lines
            .next()
            .unwrap_or_default()
            .split('\t')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        for line in lines {
            if line.starts_with("-") {
                continue;
            }
            let mut obj = serde_json::Map::new();
            for (i, cell) in line.split('\t').enumerate() {
                let key = headers
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("col_{}", i));
                obj.insert(key, typed_value(cell));
            }
            if !obj.is_empty() {
                rows.push(Value::Object(obj));
            }
        }

        let row_count = rows.len();
        Ok(QueryResult {
            schema: headers,
            rows,
            stats: [
                ("connector".to_string(), json!("mssql")),
                ("row_count".to_string(), json!(row_count)),
            ]
            .into_iter()
            .collect(),
        })
    }

    async fn stream_execute(
        &self,
        ctx: &ConnectorContext,
        req: QueryRequest,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(self.execute(ctx, req).await?.rows)
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}
