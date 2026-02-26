use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::db::Db;

// ---------------------------------------------------------------------------
// SQL
// ---------------------------------------------------------------------------

pub const CALLBACKS_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS dag_callbacks (
    dag_id     TEXT PRIMARY KEY,
    config     TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (dag_id) REFERENCES dags(id)
)";

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
    pub fn new() -> Self {
        Self
    }

    // -----------------------------------------------------------------------
    // DB helpers
    // -----------------------------------------------------------------------

    /// Persist (upsert) a `CallbackConfig` for the given DAG.
    pub fn save_callbacks(
        db: &Arc<Db>,
        dag_id: &str,
        config: &CallbackConfig,
    ) -> Result<()> {
        let config_json = serde_json::to_string(config)?;
        let updated_at = Utc::now().to_rfc3339();
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO dag_callbacks (dag_id, config, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(dag_id) DO UPDATE SET config = excluded.config,
                                               updated_at = excluded.updated_at",
            rusqlite::params![dag_id, config_json, updated_at],
        )?;
        info!(dag_id, "saved callback config");
        Ok(())
    }

    /// Retrieve the `CallbackConfig` for the given DAG, if any.
    pub fn get_callbacks(
        db: &Arc<Db>,
        dag_id: &str,
    ) -> Result<Option<CallbackConfig>> {
        let conn = db.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT config FROM dag_callbacks WHERE dag_id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![dag_id])?;
        if let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let config: CallbackConfig = serde_json::from_str(&json)?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    /// Remove all callbacks for a DAG.
    pub fn delete_callbacks(db: &Arc<Db>, dag_id: &str) -> Result<()> {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM dag_callbacks WHERE dag_id = ?1",
            rusqlite::params![dag_id],
        )?;
        info!(dag_id, "deleted callback config");
        Ok(())
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Notification dispatch
// ---------------------------------------------------------------------------

/// Escape a string for safe inclusion inside single-quoted shell arguments.
fn shell_escape_single(s: &str) -> String {
    s.replace('\'', r"'\''")
}

/// Dispatch a single notification to one target.
pub async fn send_notification(
    target: &NotificationTarget,
    payload: &NotificationPayload,
) -> Result<()> {
    match target {
        NotificationTarget::Webhook { url, headers } => {
            let body = serde_json::to_string(payload)?;
            let escaped_body = shell_escape_single(&body);
            let escaped_url = shell_escape_single(url);

            // Build the curl command programmatically.
            let mut cmd = tokio::process::Command::new("curl");
            cmd.args([
                "-s",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "-X",
                "POST",
                "-H",
                "Content-Type: application/json",
            ]);

            // Inject extra headers if supplied.
            if let Some(hdrs) = headers {
                for (k, v) in hdrs {
                    cmd.arg("-H");
                    cmd.arg(format!("{}: {}", k, v));
                }
            }

            // Use --data-raw so we don't need to worry about @ interpretation.
            cmd.args(["--data-raw", &escaped_body, &escaped_url]);

            let output = cmd.output().await?;
            let status_str = String::from_utf8_lossy(&output.stdout);
            let http_status: u16 = status_str.trim().parse().unwrap_or(0);

            if !output.status.success() || http_status >= 400 {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!(
                    url = %url,
                    http_status,
                    stderr = %stderr,
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
            // Format a minimal Slack-compatible payload.
            let text = format!(
                "*[VORTEX]* `{}` — DAG `{}` run `{}` → *{}*\n{}",
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

            let body = serde_json::to_string(&slack_payload)?;
            let escaped_body = shell_escape_single(&body);
            let escaped_url = shell_escape_single(webhook_url);

            let output = tokio::process::Command::new("curl")
                .args([
                    "-s",
                    "-o",
                    "/dev/null",
                    "-w",
                    "%{http_code}",
                    "-X",
                    "POST",
                    "-H",
                    "Content-Type: application/json",
                    "--data-raw",
                    &escaped_body,
                    &escaped_url,
                ])
                .output()
                .await?;

            let status_str = String::from_utf8_lossy(&output.stdout);
            let http_status: u16 = status_str.trim().parse().unwrap_or(0);

            if !output.status.success() || http_status >= 400 {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!(
                    webhook_url = %webhook_url,
                    http_status,
                    stderr = %stderr,
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
            // Best-effort: use curl's SMTP support.
            // curl smtp://host:port --mail-from <from> --mail-rcpt <to> ...
            let subject = format!(
                "VORTEX: {} — DAG {} ({})",
                payload.event_type, payload.dag_id, payload.state
            );
            let body_text = format!(
                "Subject: {}\r\nFrom: {}\r\nTo: {}\r\n\r\n{}\r\n\r\nDAG: {}\nRun: {}\nState: {}\nTime: {}",
                subject,
                from,
                to.join(", "),
                payload.message,
                payload.dag_id,
                payload.run_id,
                payload.state,
                payload.timestamp,
            );

            let smtp_url = format!("smtp://{}:{}", smtp_host, smtp_port);

            let mut cmd = tokio::process::Command::new("curl");
            cmd.args(["-s", "--url", &smtp_url, "--mail-from", from]);

            for recipient in to {
                cmd.args(["--mail-rcpt", recipient]);
            }

            if let (Some(user), Some(pass)) = (username, password) {
                cmd.args(["--user", &format!("{}:{}", user, pass)]);
            }

            // Pipe the email body via stdin using --upload-file -
            cmd.args(["--upload-file", "-"]);
            cmd.stdin(std::process::Stdio::piped());

            let mut child = cmd.spawn()?;
            if let Some(stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let mut stdin = tokio::io::BufWriter::new(stdin);
                stdin.write_all(body_text.as_bytes()).await?;
                stdin.flush().await?;
            }

            let output = child.wait_with_output().await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    smtp_host = %smtp_host,
                    stderr = %stderr,
                    "email notification may have failed (best effort)"
                );
                // Don't hard-error on email — it's best effort.
            } else {
                info!(smtp_host = %smtp_host, to = ?to, "email notification sent");
            }
            Ok(())
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

    // Spawn one task per target and collect.
    let futures: Vec<_> = targets
        .iter()
        .map(|t| {
            let target = t.clone();
            let p = payload.clone();
            tokio::spawn(async move { send_notification(&target, &p).await })
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
