use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::db_trait::DatabaseBackend;

use tokio::sync::Semaphore;
use once_cell::sync::Lazy;

/// Limit concurrent notification dispatches to prevent resource exhaustion.
static NOTIFICATION_SEMAPHORE: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(10));



// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single notification destination.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum NotificationTarget {
    Webhook {
        url: String,
        headers: Option<HashMap<String, String>>,
    },
    Slack {
        webhook_url: String,
        channel: Option<String>,
    },
    Email {
        smtp_host: String,
        smtp_port: u16,
        from: String,
        to: Vec<String>,
        // TODO (BUG-083): Wrap credentials in `secrecy::Secret<String>` for zeroize-on-drop.
        username: Option<String>,
        password: Option<String>,
    },
}

/// Per-DAG callback configuration stored in `dag_callbacks`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CallbackConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_success: Option<Vec<NotificationTarget>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_failure: Option<Vec<NotificationTarget>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_retry: Option<Vec<NotificationTarget>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_sla_miss: Option<Vec<NotificationTarget>>,
}

/// Payload sent to every notification target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPayload {
    pub event_type: String,
    pub dag_id: String,
    pub task_id: Option<String>,
    pub run_id: String,
    pub state: String,
    pub timestamp: String,
    pub message: String,
}

impl NotificationPayload {
    pub fn new(
        event_type: impl Into<String>,
        dag_id: impl Into<String>,
        task_id: Option<String>,
        run_id: impl Into<String>,
        state: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            dag_id: dag_id.into(),
            task_id,
            run_id: run_id.into(),
            state: state.into(),
            timestamp: Utc::now().to_rfc3339(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// NotificationManager
// ---------------------------------------------------------------------------

/// Stateless manager — all state lives in the DB or is passed in.
pub struct NotificationManager;

impl NotificationManager {
    /// Persist (upsert) a `CallbackConfig` for the given DAG.
    pub async fn save_callbacks(
        db: &Arc<dyn DatabaseBackend>,
        dag_id: &str,
        config: &CallbackConfig,
    ) -> Result<()> {
        let config_json = serde_json::to_string(config)?;
        db.save_callbacks(dag_id, &config_json).await?;
        info!(dag_id, "saved callback config");
        Ok(())
    }

    /// Retrieve the `CallbackConfig` for the given DAG, if any.
    pub async fn get_callbacks(
        db: &Arc<dyn DatabaseBackend>,
        dag_id: &str,
    ) -> Result<Option<CallbackConfig>> {
        match db.get_callbacks(dag_id).await? {
            Some(val) => {
                let config: CallbackConfig = serde_json::from_value(val)?;
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    /// Remove all callbacks for a DAG.
    pub async fn delete_callbacks(db: &Arc<dyn DatabaseBackend>, dag_id: &str) -> Result<()> {
        db.delete_callbacks(dag_id).await?;
        info!(dag_id, "deleted callback config");
        Ok(())
    }
}


// ---------------------------------------------------------------------------
// SSRF Protection
// ---------------------------------------------------------------------------

/// Validate a webhook URL to prevent SSRF attacks.
/// Rejects non-HTTP(S) schemes, private/link-local IPs, and localhost.
fn validate_webhook_url(url_str: &str) -> Result<(), String> {
    // Basic scheme check
    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
        return Err("Only http and https URL schemes are allowed".into());
    }

    // Extract host portion: skip scheme, take up to first '/' or ':' after host
    let after_scheme = url_str
        .strip_prefix("https://")
        .or_else(|| url_str.strip_prefix("http://"))
        .unwrap_or("");

    let host = after_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    if host.is_empty() {
        return Err("URL has no host".into());
    }

    let host_lower = host.to_lowercase();
    if host_lower == "localhost" {
        return Err("Webhook URLs targeting localhost are not allowed".into());
    }

    // Check for private/link-local IP addresses
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let is_private = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()                                // 127.x
                || v4.octets()[0] == 10                         // 10.x
                || (v4.octets()[0] == 172 && (16..=31).contains(&v4.octets()[1])) // 172.16-31.x
                || (v4.octets()[0] == 192 && v4.octets()[1] == 168) // 192.168.x
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254) // 169.254.x (link-local)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()                                // ::1
                || v6.octets()[0] == 0xfc || v6.octets()[0] == 0xfd // fc00::/7 (ULA)
                || (v6.octets()[0] == 0xfe && (v6.octets()[1] & 0xc0) == 0x80) // fe80::/10 (link-local)
            }
        };
        if is_private {
            return Err(format!("Webhook URLs targeting private/link-local IP {} are not allowed", ip));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Notification dispatch
// ---------------------------------------------------------------------------

/// Dispatch a single notification to one target.
pub async fn send_notification(
    target: &NotificationTarget,
    payload: &NotificationPayload,
) -> Result<()> {
    match target {
        NotificationTarget::Webhook { url, headers } => {
            validate_webhook_url(url).map_err(|e| anyhow!("SSRF protection: {}", e))?;
            let body = serde_json::to_string(payload)?;

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?;

            let mut request = client
                .post(url)
                .header("Content-Type", "application/json")
                .body(body);

            // Inject extra headers if supplied.
            if let Some(hdrs) = headers {
                for (k, v) in hdrs {
                    request = request.header(k.as_str(), v.as_str());
                }
            }

            let response = request.send().await.map_err(|e| {
                anyhow!("webhook POST to {} failed: {}", url, e)
            })?;

            let http_status = response.status().as_u16();
            if http_status >= 400 {
                error!(
                    url = %url,
                    http_status,
                    "webhook notification failed"
                );
                return Err(anyhow!(
                    "webhook POST to {} returned HTTP {}",
                    url,
                    http_status
                ));
            }
            info!(url = %url, http_status, "webhook notification sent");
            Ok(())
        }

        NotificationTarget::Slack {
            webhook_url,
            channel,
        } => {
            validate_webhook_url(webhook_url).map_err(|e| anyhow!("SSRF protection: {}", e))?;
            // Format a minimal Slack-compatible payload.
            let text = format!(
                "*[RYUO]* `{}` — DAG `{}` run `{}` → *{}*\n{}",
                payload.event_type,
                payload.dag_id,
                payload.run_id,
                payload.state,
                payload.message,
            );

            let slack_payload = match channel {
                Some(ch) => serde_json::json!({ "text": text, "channel": ch }),
                None => serde_json::json!({ "text": text }),
            };

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?;

            let response = client
                .post(webhook_url)
                .header("Content-Type", "application/json")
                .json(&slack_payload)
                .send()
                .await
                .map_err(|e| anyhow!("slack webhook POST to {} failed: {}", webhook_url, e))?;

            let http_status = response.status().as_u16();
            if http_status >= 400 {
                error!(
                    webhook_url = %webhook_url,
                    http_status,
                    "slack notification failed"
                );
                return Err(anyhow!(
                    "slack webhook POST to {} returned HTTP {}",
                    webhook_url,
                    http_status
                ));
            }
            info!(webhook_url = %webhook_url, http_status, "slack notification sent");
            Ok(())
        }

        NotificationTarget::Email {
            smtp_host,
            smtp_port,
            from,
            to,
            username,
            password,
        } => {
            // ARCH-4 FIX: Use `lettre` async SMTP instead of shelling out to `curl`.
            use lettre::{
                AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
                message::header::ContentType,
                transport::smtp::authentication::{Credentials, Mechanism},
            };

            let subject = format!(
                "RYUO: {} — DAG {} ({})",
                payload.event_type, payload.dag_id, payload.state
            );

            // Build the email message
            let mut msg_builder = Message::builder()
                .from(from.parse().map_err(|e| anyhow!("invalid from address '{}': {}", from, e))?)
                .subject(&subject);

            for recipient in to {
                msg_builder = msg_builder.to(
                    recipient.parse().map_err(|e| anyhow!("invalid to address '{}': {}", recipient, e))?
                );
            }

            let body = format!(
                "{}\r\n\r\nDAG: {}\nRun: {}\nState: {}\nTime: {}",
                payload.message, payload.dag_id, payload.run_id, payload.state, payload.timestamp,
            );

            let email = msg_builder
                .header(ContentType::TEXT_PLAIN)
                .body(body)
                .map_err(|e| anyhow!("failed to build email: {}", e))?;

            // Build SMTP transport with the configured port
            let mut transport_builder = if *smtp_port == 465 {
                AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host)
                    .map_err(|e| anyhow!("invalid smtp_host '{}': {}", smtp_host, e))?
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host)
                    .map_err(|e| anyhow!("invalid smtp_host '{}': {}", smtp_host, e))?
            };
            
            transport_builder = transport_builder.port(*smtp_port);

            if let (Some(user), Some(pass)) = (username, password) {
                transport_builder = transport_builder
                    .credentials(Credentials::new(user.clone(), pass.clone()))
                    .authentication(vec![Mechanism::Login, Mechanism::Plain]);
            }

            let mailer = transport_builder.build();

            match mailer.send(email).await {
                Ok(_) => {
                    info!(smtp_host = %smtp_host, to = ?to, "email notification sent");
                    Ok(())
                }
                Err(e) => {
                    error!(
                        smtp_host = %smtp_host,
                        error = %e,
                        "email notification failed"
                    );
                    Err(anyhow!("email send via {} failed: {}", smtp_host, e))
                }
            }
        }
    }
}

/// Fire all callbacks for the given event concurrently.
///
/// Returns one `Result<()>` per dispatched notification (in declaration order).
/// Never panics — individual failures are captured in the returned Vec.
pub async fn fire_callbacks(
    config: &CallbackConfig,
    event: &str,
    payload: &NotificationPayload,
) -> Vec<Result<()>> {
    let targets: &[NotificationTarget] = match event {
        "success" => config
            .on_success
            .as_deref()
            .unwrap_or(&[]),
        "failure" => config
            .on_failure
            .as_deref()
            .unwrap_or(&[]),
        "retry" => config
            .on_retry
            .as_deref()
            .unwrap_or(&[]),
        "sla_miss" => config
            .on_sla_miss
            .as_deref()
            .unwrap_or(&[]),
        other => {
            warn!(event = other, "unknown notification event — skipping");
            &[]
        }
    };

    if targets.is_empty() {
        info!(event, "no notification targets configured");
        return vec![];
    }

    info!(
        event,
        count = targets.len(),
        dag_id = %payload.dag_id,
        "firing notifications"
    );

    // Spawn one task per target and collect, with rate limiting.
    let futures: Vec<_> = targets
        .iter()
        .map(|t| {
            let target = t.clone();
            let p = payload.clone();
            tokio::spawn(async move {
                let _permit = NOTIFICATION_SEMAPHORE.acquire().await
                    .map_err(|e| anyhow!("semaphore error: {}", e))?;
                send_notification(&target, &p).await
            })
        })
        .collect();

    let mut results = Vec::with_capacity(futures.len());
    for handle in futures {
        match handle.await {
            Ok(inner) => results.push(inner),
            Err(join_err) => results.push(Err(anyhow!("task join error: {}", join_err))),
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_config_round_trips() {
        let cfg = CallbackConfig {
            on_success: Some(vec![NotificationTarget::Webhook {
                url: "https://example.com/hook".into(),
                headers: None,
            }]),
            on_failure: Some(vec![NotificationTarget::Slack {
                webhook_url: "https://hooks.slack.com/abc".into(),
                channel: Some("#alerts".into()),
            }]),
            on_retry: None,
            on_sla_miss: None,
        };

        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: CallbackConfig = serde_json::from_str(&json).unwrap();
        // on_retry and on_sla_miss are skipped in serialization.
        assert!(decoded.on_retry.is_none());
        assert!(decoded.on_success.is_some());
    }

    #[test]
    fn notification_payload_has_timestamp() {
        let p = NotificationPayload::new(
            "success",
            "my_dag",
            None,
            "run_001",
            "Succeeded",
            "All good",
        );
        assert!(!p.timestamp.is_empty());
        assert_eq!(p.event_type, "success");
    }

    #[tokio::test]
    async fn fire_callbacks_unknown_event_returns_empty() {
        let cfg = CallbackConfig::default();
        let p = NotificationPayload::new(
            "unknown",
            "dag1",
            None,
            "run1",
            "Unknown",
            "",
        );
        let results = fire_callbacks(&cfg, "unknown", &p).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn fire_callbacks_no_targets_returns_empty() {
        let cfg = CallbackConfig {
            on_success: None,
            ..Default::default()
        };
        let p = NotificationPayload::new("success", "dag1", None, "run1", "Succeeded", "");
        let results = fire_callbacks(&cfg, "success", &p).await;
        assert!(results.is_empty());
    }
}
