#![allow(dead_code)]
// OpenTelemetry, Distributed Tracing & APM Integration
//
// Provides:
// - Configurable OpenTelemetry (OTLP) trace exporter
// - W3C Trace Context propagation through gRPC and HTTP
// - Trace context extraction/injection helpers
// - Span builders for scheduler, executor, and DAG operations
// - APM metric hooks (duration histograms, error counters)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Trace Context (W3C traceparent) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub trace_flags: u8,
}

impl TraceContext {
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string().replace('-', ""),
            span_id: Uuid::new_v4().to_string().replace('-', "")[..16].to_string(),
            parent_span_id: None,
            trace_flags: 1, // sampled
        }
    }

    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: Uuid::new_v4().to_string().replace('-', "")[..16].to_string(),
            parent_span_id: Some(self.span_id.clone()),
            trace_flags: self.trace_flags,
        }
    }

    /// Parse W3C traceparent header: "00-<trace_id>-<span_id>-<flags>"
    pub fn from_traceparent(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }
        let trace_id = parts[1].to_string();
        let parent_span_id = parts[2].to_string();
        let flags = u8::from_str_radix(parts[3], 16).ok()?;
        // Validate lengths
        if trace_id.len() != 32 || parent_span_id.len() != 16 {
            return None;
        }
        Some(Self {
            trace_id,
            span_id: Uuid::new_v4().to_string().replace('-', "")[..16].to_string(),
            parent_span_id: Some(parent_span_id),
            trace_flags: flags,
        })
    }

    /// Serialize to W3C traceparent header
    pub fn to_traceparent(&self) -> String {
        format!("00-{}-{}-{:02x}", self.trace_id, self.span_id, self.trace_flags)
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Span ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: SpanKind,
    pub status: SpanStatus,
    pub start_time_unix_nano: u128,
    pub end_time_unix_nano: Option<u128>,
    pub attributes: HashMap<String, String>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SpanKind {
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SpanStatus {
    Unset,
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp_unix_nano: u128,
    pub attributes: HashMap<String, String>,
}

impl Span {
    pub fn new(ctx: &TraceContext, name: &str, kind: SpanKind) -> Self {
        Self {
            trace_id: ctx.trace_id.clone(),
            span_id: ctx.span_id.clone(),
            parent_span_id: ctx.parent_span_id.clone(),
            name: name.to_string(),
            kind,
            status: SpanStatus::Unset,
            start_time_unix_nano: now_unix_nano(),
            end_time_unix_nano: None,
            attributes: HashMap::new(),
            events: Vec::new(),
        }
    }

    pub fn set_attribute(&mut self, key: &str, value: &str) {
        self.attributes.insert(key.to_string(), value.to_string());
    }

    pub fn add_event(&mut self, name: &str) {
        self.events.push(SpanEvent {
            name: name.to_string(),
            timestamp_unix_nano: now_unix_nano(),
            attributes: HashMap::new(),
        });
    }

    pub fn set_ok(&mut self) {
        self.status = SpanStatus::Ok;
    }

    pub fn set_error(&mut self, message: &str) {
        self.status = SpanStatus::Error;
        self.set_attribute("error.message", message);
    }

    pub fn end(&mut self) {
        self.end_time_unix_nano = Some(now_unix_nano());
    }

    pub fn duration_ns(&self) -> Option<u128> {
        self.end_time_unix_nano
            .map(|end| end.saturating_sub(self.start_time_unix_nano))
    }
}

fn now_unix_nano() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

// ─── OTLP Exporter ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpExporterConfig {
    pub endpoint: String,
    pub protocol: OtlpProtocol,
    pub headers: HashMap<String, String>,
    pub timeout_ms: u64,
    pub batch_size: usize,
    pub export_interval_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum OtlpProtocol {
    Grpc,
    HttpJson,
}

impl Default for OtlpExporterConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            protocol: OtlpProtocol::Grpc,
            headers: HashMap::new(),
            timeout_ms: 10_000,
            batch_size: 512,
            export_interval_ms: 5_000,
        }
    }
}

/// Batching span exporter that sends spans to an OTLP collector.
/// In production, uses `reqwest` to POST to the OTLP endpoint.
pub struct OtlpExporter {
    config: OtlpExporterConfig,
    buffer: tokio::sync::Mutex<Vec<Span>>,
}

impl OtlpExporter {
    pub fn new(config: OtlpExporterConfig) -> Self {
        Self {
            config,
            buffer: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    pub async fn record(&self, span: Span) {
        let should_flush;
        {
            let mut buf = self.buffer.lock().await;
            buf.push(span);
            should_flush = buf.len() >= self.config.batch_size;
        }
        if should_flush {
            self.flush().await;
        }
    }

    pub async fn flush(&self) {
        let spans: Vec<Span> = {
            let mut buf = self.buffer.lock().await;
            std::mem::take(&mut *buf)
        };

        if spans.is_empty() {
            return;
        }

        let payload = serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [
                        {"key": "service.name", "value": {"stringValue": "vortex"}},
                        {"key": "service.version", "value": {"stringValue": env!("CARGO_PKG_VERSION")}}
                    ]
                },
                "scopeSpans": [{
                    "scope": {"name": "vortex.telemetry", "version": env!("CARGO_PKG_VERSION")},
                    "spans": spans.iter().map(|s| {
                        serde_json::json!({
                            "traceId": s.trace_id,
                            "spanId": s.span_id,
                            "parentSpanId": s.parent_span_id,
                            "name": s.name,
                            "kind": match s.kind {
                                SpanKind::Internal => 1,
                                SpanKind::Server => 2,
                                SpanKind::Client => 3,
                                SpanKind::Producer => 4,
                                SpanKind::Consumer => 5,
                            },
                            "startTimeUnixNano": s.start_time_unix_nano.to_string(),
                            "endTimeUnixNano": s.end_time_unix_nano.map(|n| n.to_string()),
                            "status": {
                                "code": match s.status {
                                    SpanStatus::Unset => 0,
                                    SpanStatus::Ok => 1,
                                    SpanStatus::Error => 2,
                                }
                            },
                            "attributes": s.attributes.iter().map(|(k, v)| {
                                serde_json::json!({"key": k, "value": {"stringValue": v}})
                            }).collect::<Vec<_>>()
                        })
                    }).collect::<Vec<_>>()
                }]
            }]
        });

        let url = match self.config.protocol {
            OtlpProtocol::HttpJson => format!("{}/v1/traces", self.config.endpoint),
            OtlpProtocol::Grpc => self.config.endpoint.clone(),
        };

        // Best-effort export — log errors but don't block
        let client = reqwest::Client::new();
        let mut req = client
            .post(&url)
            .timeout(std::time::Duration::from_millis(self.config.timeout_ms))
            .json(&payload);

        for (k, v) in &self.config.headers {
            req = req.header(k, v);
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Exported {} spans to OTLP", spans.len());
            }
            Ok(resp) => {
                tracing::warn!("OTLP export returned status {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("OTLP export failed: {}", e);
            }
        }
    }

    pub fn config(&self) -> &OtlpExporterConfig {
        &self.config
    }
}

// ─── Convenience Span Builders ───────────────────────────────

/// Create a span for a DAG execution
pub fn dag_execution_span(ctx: &TraceContext, dag_id: &str, run_id: &str) -> Span {
    let mut span = Span::new(ctx, &format!("dag.execute:{}", dag_id), SpanKind::Internal);
    span.set_attribute("dag.id", dag_id);
    span.set_attribute("dag.run_id", run_id);
    span
}

/// Create a span for a task execution
pub fn task_execution_span(
    ctx: &TraceContext,
    dag_id: &str,
    task_id: &str,
    run_id: &str,
) -> Span {
    let mut span = Span::new(ctx, &format!("task.execute:{}", task_id), SpanKind::Internal);
    span.set_attribute("dag.id", dag_id);
    span.set_attribute("task.id", task_id);
    span.set_attribute("dag.run_id", run_id);
    span
}

/// Create a span for an HTTP request
pub fn http_request_span(ctx: &TraceContext, method: &str, path: &str) -> Span {
    let mut span = Span::new(ctx, &format!("{} {}", method, path), SpanKind::Server);
    span.set_attribute("http.method", method);
    span.set_attribute("http.target", path);
    span
}

/// Create a span for a gRPC call
pub fn grpc_span(ctx: &TraceContext, service: &str, method: &str) -> Span {
    let mut span = Span::new(ctx, &format!("{}/{}", service, method), SpanKind::Client);
    span.set_attribute("rpc.system", "grpc");
    span.set_attribute("rpc.service", service);
    span.set_attribute("rpc.method", method);
    span
}

/// Create a span for a DB query
pub fn db_query_span(ctx: &TraceContext, operation: &str, table: &str) -> Span {
    let mut span = Span::new(ctx, &format!("db.{}", operation), SpanKind::Client);
    span.set_attribute("db.system", "postgresql");
    span.set_attribute("db.operation", operation);
    span.set_attribute("db.sql.table", table);
    span
}

// ─── APM Metrics Collector ───────────────────────────────────

/// In-process APM metrics collector for duration histograms and error rates
pub struct ApmMetrics {
    pub request_durations: tokio::sync::Mutex<Vec<ApmSample>>,
    pub error_count: std::sync::atomic::AtomicU64,
    pub request_count: std::sync::atomic::AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApmSample {
    pub endpoint: String,
    pub duration_ms: f64,
    pub status: u16,
    pub timestamp: u128,
}

impl ApmMetrics {
    pub fn new() -> Self {
        Self {
            request_durations: tokio::sync::Mutex::new(Vec::new()),
            error_count: std::sync::atomic::AtomicU64::new(0),
            request_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub async fn record(&self, endpoint: &str, duration_ms: f64, status: u16) {
        self.request_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if status >= 500 {
            self.error_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let mut durations = self.request_durations.lock().await;
        // Keep a rolling window of 10k samples
        if durations.len() >= 10_000 {
            durations.drain(..5_000);
        }
        durations.push(ApmSample {
            endpoint: endpoint.to_string(),
            duration_ms,
            status,
            timestamp: now_unix_nano(),
        });
    }

    pub fn total_requests(&self) -> u64 {
        self.request_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn total_errors(&self) -> u64 {
        self.error_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn error_rate(&self) -> f64 {
        let total = self.total_requests() as f64;
        if total == 0.0 {
            return 0.0;
        }
        self.total_errors() as f64 / total
    }
}

impl Default for ApmMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_new() {
        let ctx = TraceContext::new();
        assert_eq!(ctx.trace_id.len(), 32);
        assert_eq!(ctx.span_id.len(), 16);
        assert!(ctx.parent_span_id.is_none());
        assert_eq!(ctx.trace_flags, 1);
    }

    #[test]
    fn test_trace_context_child() {
        let parent = TraceContext::new();
        let child = parent.child();
        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.span_id, parent.span_id);
        assert_eq!(child.parent_span_id.as_deref(), Some(parent.span_id.as_str()));
    }

    #[test]
    fn test_traceparent_roundtrip() {
        let ctx = TraceContext::new();
        let header = ctx.to_traceparent();
        assert!(header.starts_with("00-"));

        let parsed = TraceContext::from_traceparent(&header).unwrap();
        assert_eq!(parsed.trace_id, ctx.trace_id);
        assert_eq!(parsed.parent_span_id.as_deref(), Some(ctx.span_id.as_str()));
    }

    #[test]
    fn test_traceparent_invalid() {
        assert!(TraceContext::from_traceparent("invalid").is_none());
        assert!(TraceContext::from_traceparent("01-abc-def-00").is_none());
    }

    #[test]
    fn test_span_lifecycle() {
        let ctx = TraceContext::new();
        let mut span = Span::new(&ctx, "test.op", SpanKind::Internal);
        span.set_attribute("key", "value");
        span.add_event("processing");
        span.set_ok();
        span.end();

        assert_eq!(span.name, "test.op");
        assert_eq!(span.status, SpanStatus::Ok);
        assert!(span.end_time_unix_nano.is_some());
        assert!(span.duration_ns().unwrap() > 0);
        assert_eq!(span.attributes["key"], "value");
        assert_eq!(span.events.len(), 1);
    }

    #[test]
    fn test_span_error() {
        let ctx = TraceContext::new();
        let mut span = Span::new(&ctx, "test.fail", SpanKind::Server);
        span.set_error("something broke");
        span.end();

        assert_eq!(span.status, SpanStatus::Error);
        assert_eq!(span.attributes["error.message"], "something broke");
    }

    #[test]
    fn test_convenience_spans() {
        let ctx = TraceContext::new();
        let dag_span = dag_execution_span(&ctx, "my_dag", "run_123");
        assert_eq!(dag_span.attributes["dag.id"], "my_dag");
        assert_eq!(dag_span.kind, SpanKind::Internal);

        let task_span = task_execution_span(&ctx, "my_dag", "task1", "run_123");
        assert_eq!(task_span.attributes["task.id"], "task1");

        let http_span = http_request_span(&ctx, "GET", "/api/dags");
        assert_eq!(http_span.kind, SpanKind::Server);
        assert_eq!(http_span.attributes["http.method"], "GET");

        let grpc_span_v = grpc_span(&ctx, "SwarmService", "SubmitTask");
        assert_eq!(grpc_span_v.kind, SpanKind::Client);
        assert_eq!(grpc_span_v.attributes["rpc.system"], "grpc");

        let db_span = db_query_span(&ctx, "SELECT", "dag_runs");
        assert_eq!(db_span.attributes["db.system"], "postgresql");
    }

    #[tokio::test]
    async fn test_apm_metrics() {
        let apm = ApmMetrics::new();
        apm.record("/api/dags", 12.5, 200).await;
        apm.record("/api/dags", 8.0, 200).await;
        apm.record("/api/dags", 150.0, 500).await;

        assert_eq!(apm.total_requests(), 3);
        assert_eq!(apm.total_errors(), 1);
        assert!((apm.error_rate() - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_otlp_config_default() {
        let config = OtlpExporterConfig::default();
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert_eq!(config.protocol, OtlpProtocol::Grpc);
        assert_eq!(config.batch_size, 512);
    }
}
