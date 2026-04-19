#![allow(dead_code)]
// Event-Driven Architecture & Sensor Framework
//
// Provides an enterprise event bus, webhook ingestion, sensor registry,
// event-triggered DAG runs, and async event routing.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock, Mutex};
use tracing::{info, warn, debug};

// ─── Event Types ──────────────────────────────────────────────

/// Core event envelope — every event flowing through the bus has this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub event_type: EventType,
    pub source: String,
    pub timestamp: DateTime<Utc>,
    pub payload: Value,
    pub metadata: HashMap<String, String>,
}

impl Event {
    pub fn new(event_type: EventType, source: &str, payload: Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type,
            source: source.to_string(),
            timestamp: Utc::now(),
            payload,
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// File created/modified/deleted on watched path
    FileChange,
    /// Webhook HTTP request received
    WebhookReceived,
    /// Dataset updated (S3, GCS, BigQuery table, etc.)
    DatasetUpdated,
    /// DAG run completed (success, failure, or any terminal state)
    DagCompleted,
    /// Task state changed
    TaskStateChanged,
    /// Scheduled timer fired
    TimerFired,
    /// External system event (Kafka message, SQS, Pub/Sub)
    ExternalMessage,
    /// Custom user-defined event type
    Custom(String),
}

/// Routing rule: when an event matches a filter, trigger a DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTrigger {
    pub id: String,
    pub name: String,
    pub event_type: EventType,
    pub filter: EventFilter,
    pub action: TriggerAction,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

/// Filter conditions that an event must match to fire a trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    /// Source must match (glob-like: "webhook/*", "s3://bucket/*")
    pub source_pattern: Option<String>,
    /// JSON path expressions that must evaluate truthy on the payload
    pub payload_conditions: Vec<PayloadCondition>,
    /// Only fire if all metadata key-value pairs are present
    pub required_metadata: HashMap<String, String>,
}

impl EventFilter {
    pub fn matches(&self, event: &Event) -> bool {
        // Source pattern match
        if let Some(ref pattern) = self.source_pattern {
            if !glob_match(pattern, &event.source) {
                return false;
            }
        }

        // Metadata match
        for (key, value) in &self.required_metadata {
            match event.metadata.get(key) {
                Some(v) if v == value => {}
                _ => return false,
            }
        }

        // Payload conditions
        for condition in &self.payload_conditions {
            if !condition.evaluate(&event.payload) {
                return false;
            }
        }

        true
    }
}

/// Simple payload condition: check a JSON field against an expected value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadCondition {
    pub field: String,
    pub operator: ConditionOperator,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperator {
    Equals,
    NotEquals,
    Contains,
    GreaterThan,
    LessThan,
    Exists,
}

impl PayloadCondition {
    pub fn evaluate(&self, payload: &Value) -> bool {
        let field_value = json_path_get(payload, &self.field);
        match self.operator {
            ConditionOperator::Exists => field_value.is_some(),
            ConditionOperator::Equals => field_value.map_or(false, |v| v == &self.value),
            ConditionOperator::NotEquals => field_value.map_or(true, |v| v != &self.value),
            ConditionOperator::Contains => {
                field_value.and_then(|v| v.as_str()).map_or(false, |s| {
                    self.value.as_str().map_or(false, |needle| s.contains(needle))
                })
            }
            ConditionOperator::GreaterThan => {
                field_value.and_then(|v| v.as_f64()).map_or(false, |v| {
                    self.value.as_f64().map_or(false, |t| v > t)
                })
            }
            ConditionOperator::LessThan => {
                field_value.and_then(|v| v.as_f64()).map_or(false, |v| {
                    self.value.as_f64().map_or(false, |t| v < t)
                })
            }
        }
    }
}

/// Action to take when a trigger fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerAction {
    /// DAG ID to trigger
    pub dag_id: String,
    /// Optional: override specific task configs
    pub config_overrides: HashMap<String, Value>,
    /// Optional: pass event payload as a DAG run config
    pub pass_event_payload: bool,
}

// ─── Event Bus ────────────────────────────────────────────────

/// Central event bus — broadcast events to all subscribers and evaluate triggers.
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    triggers: Arc<RwLock<Vec<EventTrigger>>>,
    event_log: Arc<Mutex<Vec<Event>>>,
    max_log_size: usize,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            triggers: Arc::new(RwLock::new(Vec::new())),
            event_log: Arc::new(Mutex::new(Vec::new())),
            max_log_size: 10_000,
        }
    }

    /// Subscribe to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Publish an event to the bus. Returns triggered DAG IDs.
    pub async fn publish(&self, event: Event) -> Result<Vec<TriggeredDag>> {
        debug!(event_type = ?event.event_type, source = %event.source, "Event published");

        // Log the event
        {
            let mut log = self.event_log.lock().await;
            if log.len() >= self.max_log_size {
                let half = log.len() / 2;
                log.drain(0..half);
            }
            log.push(event.clone());
        }

        // Broadcast to subscribers (best effort — if no receivers, that's fine)
        let _ = self.sender.send(event.clone());

        // Evaluate triggers
        let triggers = self.triggers.read().await;
        let mut triggered = Vec::new();
        for trigger in triggers.iter() {
            if !trigger.enabled { continue; }
            if trigger.event_type != event.event_type { continue; }
            if trigger.filter.matches(&event) {
                info!(
                    trigger = %trigger.name, dag = %trigger.action.dag_id,
                    event_id = %event.id, "Trigger fired"
                );
                triggered.push(TriggeredDag {
                    trigger_id: trigger.id.clone(),
                    trigger_name: trigger.name.clone(),
                    dag_id: trigger.action.dag_id.clone(),
                    event_id: event.id.clone(),
                    config: if trigger.action.pass_event_payload {
                        event.payload.clone()
                    } else {
                        Value::Object(
                            trigger.action.config_overrides.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        )
                    },
                    triggered_at: Utc::now(),
                });
            }
        }
        Ok(triggered)
    }

    /// Register a new trigger.
    pub async fn register_trigger(&self, trigger: EventTrigger) -> Result<()> {
        let mut triggers = self.triggers.write().await;
        if triggers.iter().any(|t| t.id == trigger.id) {
            return Err(anyhow!("Trigger with id '{}' already exists", trigger.id));
        }
        info!(trigger_id = %trigger.id, name = %trigger.name, "Trigger registered");
        triggers.push(trigger);
        Ok(())
    }

    /// Remove a trigger by ID.
    pub async fn remove_trigger(&self, trigger_id: &str) -> Result<()> {
        let mut triggers = self.triggers.write().await;
        let len_before = triggers.len();
        triggers.retain(|t| t.id != trigger_id);
        if triggers.len() == len_before {
            return Err(anyhow!("Trigger '{}' not found", trigger_id));
        }
        Ok(())
    }

    /// Enable or disable a trigger.
    pub async fn set_trigger_enabled(&self, trigger_id: &str, enabled: bool) -> Result<()> {
        let mut triggers = self.triggers.write().await;
        let trigger = triggers.iter_mut().find(|t| t.id == trigger_id)
            .ok_or_else(|| anyhow!("Trigger '{}' not found", trigger_id))?;
        trigger.enabled = enabled;
        Ok(())
    }

    /// List all triggers.
    pub async fn list_triggers(&self) -> Vec<EventTrigger> {
        self.triggers.read().await.clone()
    }

    /// Get recent events from the log.
    pub async fn recent_events(&self, limit: usize) -> Vec<Event> {
        let log = self.event_log.lock().await;
        log.iter().rev().take(limit).cloned().collect()
    }

    /// Get event count.
    pub async fn event_count(&self) -> usize {
        self.event_log.lock().await.len()
    }
}

/// Result of a trigger evaluation — represents a DAG that should be triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggeredDag {
    pub trigger_id: String,
    pub trigger_name: String,
    pub dag_id: String,
    pub event_id: String,
    pub config: Value,
    pub triggered_at: DateTime<Utc>,
}

// ─── Webhook Receiver ─────────────────────────────────────────

/// Configuration for a webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub id: String,
    pub name: String,
    pub path: String,
    pub secret: Option<String>,
    pub event_type: EventType,
    pub enabled: bool,
}

/// Webhook receiver — validates and converts HTTP requests into events.
pub struct WebhookReceiver {
    configs: Arc<RwLock<Vec<WebhookConfig>>>,
}

impl WebhookReceiver {
    pub fn new() -> Self {
        Self {
            configs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn register(&self, config: WebhookConfig) -> Result<()> {
        let mut configs = self.configs.write().await;
        if configs.iter().any(|c| c.path == config.path) {
            return Err(anyhow!("Webhook path '{}' already registered", config.path));
        }
        configs.push(config);
        Ok(())
    }

    pub async fn remove(&self, webhook_id: &str) -> Result<()> {
        let mut configs = self.configs.write().await;
        let before = configs.len();
        configs.retain(|c| c.id != webhook_id);
        if configs.len() == before {
            return Err(anyhow!("Webhook '{}' not found", webhook_id));
        }
        Ok(())
    }

    pub async fn list(&self) -> Vec<WebhookConfig> {
        self.configs.read().await.clone()
    }

    /// Process an incoming webhook request. Returns an Event if the webhook is valid.
    pub async fn process_request(
        &self,
        path: &str,
        headers: &HashMap<String, String>,
        body: Value,
    ) -> Result<Event> {
        let configs = self.configs.read().await;
        let config = configs.iter().find(|c| c.path == path)
            .ok_or_else(|| anyhow!("No webhook registered for path '{}'", path))?;

        if !config.enabled {
            return Err(anyhow!("Webhook '{}' is disabled", config.name));
        }

        // Validate HMAC signature if secret is configured
        if let Some(ref secret) = config.secret {
            let signature = headers.get("x-webhook-signature")
                .or_else(|| headers.get("x-hub-signature-256"))
                .ok_or_else(|| anyhow!("Missing webhook signature header"))?;
            validate_webhook_signature(secret, &serde_json::to_string(&body)?, signature)?;
        }

        let event = Event::new(config.event_type.clone(), &format!("webhook/{}", config.name), body)
            .with_metadata("webhook_id", &config.id)
            .with_metadata("webhook_path", path);

        info!(webhook = %config.name, event_id = %event.id, "Webhook event created");
        Ok(event)
    }
}

// ─── Sensor Registry ──────────────────────────────────────────

/// Pluggable sensor trait for enterprise sensor framework.
#[async_trait]
pub trait EventSensor: Send + Sync {
    fn name(&self) -> &str;
    fn sensor_type(&self) -> &str;
    async fn poll(&self) -> Result<Option<Event>>;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}

/// Registry of enterprise sensors with lifecycle management.
pub struct SensorRegistry {
    sensors: Arc<RwLock<HashMap<String, Arc<dyn EventSensor>>>>,
    running: Arc<RwLock<HashMap<String, bool>>>,
}

impl SensorRegistry {
    pub fn new() -> Self {
        Self {
            sensors: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, sensor: Arc<dyn EventSensor>) -> Result<()> {
        let name = sensor.name().to_string();
        let mut sensors = self.sensors.write().await;
        if sensors.contains_key(&name) {
            return Err(anyhow!("Sensor '{}' already registered", name));
        }
        sensors.insert(name.clone(), sensor);
        self.running.write().await.insert(name, false);
        Ok(())
    }

    pub async fn unregister(&self, name: &str) -> Result<()> {
        let mut sensors = self.sensors.write().await;
        sensors.remove(name).ok_or_else(|| anyhow!("Sensor '{}' not found", name))?;
        self.running.write().await.remove(name);
        Ok(())
    }

    pub async fn start_sensor(&self, name: &str) -> Result<()> {
        let sensors = self.sensors.read().await;
        let sensor = sensors.get(name).ok_or_else(|| anyhow!("Sensor '{}' not found", name))?;
        sensor.start().await?;
        self.running.write().await.insert(name.to_string(), true);
        info!(sensor = %name, "Sensor started");
        Ok(())
    }

    pub async fn stop_sensor(&self, name: &str) -> Result<()> {
        let sensors = self.sensors.read().await;
        let sensor = sensors.get(name).ok_or_else(|| anyhow!("Sensor '{}' not found", name))?;
        sensor.stop().await?;
        self.running.write().await.insert(name.to_string(), false);
        info!(sensor = %name, "Sensor stopped");
        Ok(())
    }

    pub async fn poll_sensor(&self, name: &str) -> Result<Option<Event>> {
        let sensors = self.sensors.read().await;
        let sensor = sensors.get(name).ok_or_else(|| anyhow!("Sensor '{}' not found", name))?;
        sensor.poll().await
    }

    pub async fn list_sensors(&self) -> Vec<SensorInfo> {
        let sensors = self.sensors.read().await;
        let running = self.running.read().await;
        sensors.iter().map(|(name, sensor)| {
            SensorInfo {
                name: name.clone(),
                sensor_type: sensor.sensor_type().to_string(),
                running: *running.get(name).unwrap_or(&false),
            }
        }).collect()
    }

    /// Poll all running sensors and collect events.
    pub async fn poll_all(&self) -> Vec<Event> {
        let sensors = self.sensors.read().await;
        let running = self.running.read().await;
        let mut events = Vec::new();
        for (name, sensor) in sensors.iter() {
            if *running.get(name).unwrap_or(&false) {
                match sensor.poll().await {
                    Ok(Some(event)) => events.push(event),
                    Ok(None) => {}
                    Err(e) => warn!(sensor = %name, error = %e, "Sensor poll failed"),
                }
            }
        }
        events
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorInfo {
    pub name: String,
    pub sensor_type: String,
    pub running: bool,
}

// ─── Built-in Sensors ─────────────────────────────────────────

/// File watcher sensor — emits events when files change in watched directories.
pub struct FileWatchSensor {
    name: String,
    watch_paths: Vec<String>,
    last_check: Mutex<HashMap<String, std::time::SystemTime>>,
    first_poll: Mutex<bool>,
}

impl FileWatchSensor {
    pub fn new(name: &str, watch_paths: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            watch_paths,
            last_check: Mutex::new(HashMap::new()),
            first_poll: Mutex::new(true),
        }
    }
}

#[async_trait]
impl EventSensor for FileWatchSensor {
    fn name(&self) -> &str { &self.name }
    fn sensor_type(&self) -> &str { "file_watch" }

    async fn poll(&self) -> Result<Option<Event>> {
        // BUG-064: On the first poll, only baseline file timestamps — don't report changes.
        let mut is_first = self.first_poll.lock().await;
        let first = *is_first;
        if first {
            *is_first = false;
        }
        drop(is_first);

        let mut last_check = self.last_check.lock().await;
        for path in &self.watch_paths {
            if let Ok(metadata) = tokio::fs::metadata(path).await {
                if let Ok(modified) = metadata.modified() {
                    let is_new = last_check.get(path).map_or(true, |prev| modified > *prev);
                    last_check.insert(path.clone(), modified);
                    if is_new && !first {
                        return Ok(Some(Event::new(
                            EventType::FileChange,
                            &format!("file_watch/{}", self.name),
                            serde_json::json!({"path": path, "size": metadata.len()}),
                        )));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn start(&self) -> Result<()> { Ok(()) }
    async fn stop(&self) -> Result<()> { Ok(()) }
}

/// Timer sensor — fires events on a cron-like schedule.
pub struct TimerSensor {
    name: String,
    interval_secs: u64,
    last_fired: Mutex<Option<DateTime<Utc>>>,
}

impl TimerSensor {
    pub fn new(name: &str, interval_secs: u64) -> Self {
        Self {
            name: name.to_string(),
            interval_secs,
            last_fired: Mutex::new(None),
        }
    }
}

#[async_trait]
impl EventSensor for TimerSensor {
    fn name(&self) -> &str { &self.name }
    fn sensor_type(&self) -> &str { "timer" }

    async fn poll(&self) -> Result<Option<Event>> {
        let mut last_fired = self.last_fired.lock().await;
        let now = Utc::now();
        let should_fire = last_fired.map_or(true, |prev| {
            (now - prev).num_seconds() >= self.interval_secs as i64
        });
        if should_fire {
            *last_fired = Some(now);
            Ok(Some(Event::new(
                EventType::TimerFired,
                &format!("timer/{}", self.name),
                serde_json::json!({"interval_secs": self.interval_secs, "fired_at": now.to_rfc3339()}),
            )))
        } else {
            Ok(None)
        }
    }

    async fn start(&self) -> Result<()> { Ok(()) }
    async fn stop(&self) -> Result<()> { Ok(()) }
}

/// HTTP health sensor — polls an HTTP endpoint and emits events on state changes.
pub struct HttpPollSensor {
    name: String,
    url: String,
    expected_status: u16,
    last_state: Mutex<Option<bool>>,
}

impl HttpPollSensor {
    pub fn new(name: &str, url: &str, expected_status: u16) -> Self {
        Self {
            name: name.to_string(),
            url: url.to_string(),
            expected_status,
            last_state: Mutex::new(None),
        }
    }
}

#[async_trait]
impl EventSensor for HttpPollSensor {
    fn name(&self) -> &str { &self.name }
    fn sensor_type(&self) -> &str { "http_poll" }

    async fn poll(&self) -> Result<Option<Event>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = client.get(&self.url).send().await;
        let is_healthy = resp.map(|r| r.status().as_u16() == self.expected_status).unwrap_or(false);

        let mut last = self.last_state.lock().await;
        let changed = last.map_or(true, |prev| prev != is_healthy);
        *last = Some(is_healthy);

        if changed {
            Ok(Some(Event::new(
                EventType::Custom("http_state_change".into()),
                &format!("http_poll/{}", self.name),
                serde_json::json!({
                    "url": self.url, "healthy": is_healthy,
                    "expected_status": self.expected_status
                }),
            )))
        } else {
            Ok(None)
        }
    }

    async fn start(&self) -> Result<()> { Ok(()) }
    async fn stop(&self) -> Result<()> { Ok(()) }
}

// ─── Helpers ──────────────────────────────────────────────────

/// Simple glob matching with * wildcards.
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 { return pattern == text; }

    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        match text[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 { return false; }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    // If pattern ends with *, remaining text is OK
    if !pattern.ends_with('*') {
        text.len() == pos
    } else {
        true
    }
}

/// Simple JSON path accessor (dot-separated, e.g. "data.status").
fn json_path_get<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for key in path.split('.') {
        match current.get(key) {
            Some(v) => current = v,
            None => return None,
        }
    }
    Some(current)
}

/// Validate HMAC-SHA256 webhook signature.
fn validate_webhook_signature(secret: &str, payload: &str, signature: &str) -> Result<()> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use subtle::ConstantTimeEq;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| anyhow!("Invalid HMAC key"))?;
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    // Strip optional "sha256=" prefix
    let sig = signature.strip_prefix("sha256=").unwrap_or(signature);
    // BUG-009: Use constant-time comparison to prevent timing attacks
    let is_valid: bool = expected.as_bytes().ct_eq(sig.as_bytes()).into();
    if !is_valid {
        return Err(anyhow!("Webhook signature mismatch"));
    }
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = Event::new(EventType::FileChange, "test", serde_json::json!({"path": "/tmp/x"}))
            .with_metadata("env", "prod");
        assert_eq!(event.event_type, EventType::FileChange);
        assert_eq!(event.source, "test");
        assert_eq!(event.metadata.get("env").unwrap(), "prod");
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("webhook/*", "webhook/github"));
        assert!(glob_match("s3://bucket/*", "s3://bucket/path/to/file"));
        assert!(!glob_match("webhook/*", "other/path"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "not-exact"));
    }

    #[test]
    fn test_json_path_get() {
        let v = serde_json::json!({"data": {"status": "ready", "count": 42}});
        assert_eq!(json_path_get(&v, "data.status"), Some(&serde_json::json!("ready")));
        assert_eq!(json_path_get(&v, "data.count"), Some(&serde_json::json!(42)));
        assert!(json_path_get(&v, "data.missing").is_none());
    }

    #[test]
    fn test_payload_condition_equals() {
        let cond = PayloadCondition {
            field: "status".to_string(),
            operator: ConditionOperator::Equals,
            value: serde_json::json!("ready"),
        };
        assert!(cond.evaluate(&serde_json::json!({"status": "ready"})));
        assert!(!cond.evaluate(&serde_json::json!({"status": "pending"})));
    }

    #[test]
    fn test_payload_condition_exists() {
        let cond = PayloadCondition {
            field: "data.key".to_string(),
            operator: ConditionOperator::Exists,
            value: Value::Null,
        };
        assert!(cond.evaluate(&serde_json::json!({"data": {"key": "val"}})));
        assert!(!cond.evaluate(&serde_json::json!({"data": {}})));
    }

    #[test]
    fn test_payload_condition_greater_than() {
        let cond = PayloadCondition {
            field: "count".to_string(),
            operator: ConditionOperator::GreaterThan,
            value: serde_json::json!(10),
        };
        assert!(cond.evaluate(&serde_json::json!({"count": 42})));
        assert!(!cond.evaluate(&serde_json::json!({"count": 5})));
    }

    #[test]
    fn test_event_filter_matches() {
        let filter = EventFilter {
            source_pattern: Some("webhook/*".to_string()),
            payload_conditions: vec![PayloadCondition {
                field: "action".to_string(),
                operator: ConditionOperator::Equals,
                value: serde_json::json!("push"),
            }],
            required_metadata: HashMap::new(),
        };
        let event = Event::new(EventType::WebhookReceived, "webhook/github", serde_json::json!({"action": "push"}));
        assert!(filter.matches(&event));

        let non_matching = Event::new(EventType::WebhookReceived, "webhook/github", serde_json::json!({"action": "pull"}));
        assert!(!filter.matches(&non_matching));
    }

    #[tokio::test]
    async fn test_event_bus_publish_and_trigger() {
        let bus = EventBus::new(100);
        let trigger = EventTrigger {
            id: "t1".to_string(),
            name: "on-file-change".to_string(),
            event_type: EventType::FileChange,
            filter: EventFilter {
                source_pattern: Some("file_watch/*".to_string()),
                payload_conditions: vec![],
                required_metadata: HashMap::new(),
            },
            action: TriggerAction {
                dag_id: "etl_pipeline".to_string(),
                config_overrides: HashMap::new(),
                pass_event_payload: true,
            },
            enabled: true,
            created_at: Utc::now(),
        };
        bus.register_trigger(trigger).await.unwrap();

        let event = Event::new(EventType::FileChange, "file_watch/watcher1", serde_json::json!({"path": "/data/new.csv"}));
        let triggered = bus.publish(event).await.unwrap();
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].dag_id, "etl_pipeline");
    }

    #[tokio::test]
    async fn test_event_bus_disabled_trigger() {
        let bus = EventBus::new(100);
        let trigger = EventTrigger {
            id: "t2".to_string(),
            name: "disabled-trigger".to_string(),
            event_type: EventType::FileChange,
            filter: EventFilter {
                source_pattern: None,
                payload_conditions: vec![],
                required_metadata: HashMap::new(),
            },
            action: TriggerAction {
                dag_id: "some_dag".to_string(),
                config_overrides: HashMap::new(),
                pass_event_payload: false,
            },
            enabled: false,
            created_at: Utc::now(),
        };
        bus.register_trigger(trigger).await.unwrap();
        let event = Event::new(EventType::FileChange, "test", Value::Null);
        let triggered = bus.publish(event).await.unwrap();
        assert_eq!(triggered.len(), 0);
    }

    #[tokio::test]
    async fn test_webhook_receiver() {
        let receiver = WebhookReceiver::new();
        receiver.register(WebhookConfig {
            id: "wh1".to_string(),
            name: "github".to_string(),
            path: "/webhooks/github".to_string(),
            secret: None,
            event_type: EventType::WebhookReceived,
            enabled: true,
        }).await.unwrap();

        let headers = HashMap::new();
        let body = serde_json::json!({"action": "push"});
        let event = receiver.process_request("/webhooks/github", &headers, body).await.unwrap();
        assert_eq!(event.event_type, EventType::WebhookReceived);
        assert_eq!(event.metadata.get("webhook_id").unwrap(), "wh1");
    }

    #[tokio::test]
    async fn test_sensor_registry() {
        let registry = SensorRegistry::new();
        let sensor = Arc::new(TimerSensor::new("test-timer", 60));
        registry.register(sensor).await.unwrap();

        let sensors = registry.list_sensors().await;
        assert_eq!(sensors.len(), 1);
        assert_eq!(sensors[0].name, "test-timer");
        assert_eq!(sensors[0].sensor_type, "timer");
        assert!(!sensors[0].running);

        registry.start_sensor("test-timer").await.unwrap();
        let sensors = registry.list_sensors().await;
        assert!(sensors[0].running);
    }

    #[tokio::test]
    async fn test_timer_sensor_poll() {
        let sensor = TimerSensor::new("test", 0);
        let event = sensor.poll().await.unwrap();
        assert!(event.is_some());
        assert_eq!(event.unwrap().event_type, EventType::TimerFired);
    }

    #[tokio::test]
    async fn test_file_watch_sensor() {
        let path = "/tmp/ryuo_event_test_file";
        tokio::fs::write(path, b"test").await.unwrap();
        let sensor = FileWatchSensor::new("test", vec![path.to_string()]);
        // First poll is baseline only — should not report changes.
        let event = sensor.poll().await.unwrap();
        assert!(event.is_none(), "First poll should return None (baseline)");
        // Simulate a modification and re-poll.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        tokio::fs::write(path, b"updated").await.unwrap();
        let event = sensor.poll().await.unwrap();
        assert!(event.is_some());
        assert_eq!(event.unwrap().event_type, EventType::FileChange);
        tokio::fs::remove_file(path).await.ok();
    }

    #[tokio::test]
    async fn test_event_bus_recent_events() {
        let bus = EventBus::new(100);
        for i in 0..5 {
            let event = Event::new(EventType::TimerFired, &format!("timer/{}", i), Value::Null);
            bus.publish(event).await.unwrap();
        }
        let recent = bus.recent_events(3).await;
        assert_eq!(recent.len(), 3);
    }

    #[tokio::test]
    async fn test_trigger_lifecycle() {
        let bus = EventBus::new(100);
        let trigger = EventTrigger {
            id: "t3".to_string(),
            name: "test".to_string(),
            event_type: EventType::DatasetUpdated,
            filter: EventFilter {
                source_pattern: None,
                payload_conditions: vec![],
                required_metadata: HashMap::new(),
            },
            action: TriggerAction {
                dag_id: "dag1".to_string(),
                config_overrides: HashMap::new(),
                pass_event_payload: false,
            },
            enabled: true,
            created_at: Utc::now(),
        };
        bus.register_trigger(trigger).await.unwrap();
        assert_eq!(bus.list_triggers().await.len(), 1);

        bus.set_trigger_enabled("t3", false).await.unwrap();
        assert!(!bus.list_triggers().await[0].enabled);

        bus.remove_trigger("t3").await.unwrap();
        assert_eq!(bus.list_triggers().await.len(), 0);
    }
}
