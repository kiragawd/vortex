#![allow(dead_code)]
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;
use tracing::warn;

use crate::db_trait::DatabaseBackend;

// ──────────────────────────── Permission Check ────────────────────────────────

/// Check if a user has a specific permission, optionally scoped to a team.
pub async fn user_has_permission(
    db: &dyn DatabaseBackend,
    user_id: &str,
    permission: &str,
    team_id: Option<&str>,
) -> Result<bool> {
    db.check_user_permission(user_id, permission, team_id).await
}

/// Get all effective permissions for a user (union of all roles).
pub async fn get_user_permissions(
    db: &dyn DatabaseBackend,
    user_id: &str,
    team_id: Option<&str>,
) -> Result<HashSet<String>> {
    let perms = db.get_user_effective_permissions(user_id, team_id).await?;
    Ok(perms.into_iter().collect())
}

// ──────────────────────────── API Token Engine ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: String,
    pub name: String,
    pub user_id: String,
    pub scopes: Vec<String>,
    pub team_id: Option<String>,
    pub expires_at: Option<String>,
    pub revoked: bool,
}

/// Generate a new API token (returns the raw token string — only shown once).
pub fn generate_token() -> String {
    format!("vtx_{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
}

/// Hash a token for storage.
pub fn hash_token(token: &str) -> Result<String> {
    Ok(bcrypt::hash(token, 10)?)
}

/// Verify a raw token against a stored hash.
pub fn verify_token(token: &str, hash: &str) -> bool {
    bcrypt::verify(token, hash).unwrap_or(false)
}

/// Validate that a token's scopes include the required permission.
///
/// # Scope matching semantics
/// 1. `"*"` — global wildcard, grants all permissions unconditionally.
/// 2. **Direct match** — exact string equality (e.g. scope `"dag.read"` matches required `"dag.read"`).
/// 3. **Category wildcard** — `"<category>.*"` matches any `"<category>.<action>"`.
///    Only applies when the required permission contains a `.` separator, so
///    `"dag.*"` matches `"dag.read"` but would NOT match a bare `"dag"` permission.
///    (BUG-068: added the `.` check to prevent over-permissive matching on
///    single-segment permissions.)
pub fn token_has_scope(token_scopes: &[String], required: &str) -> bool {
    // Wildcard scope grants everything
    if token_scopes.iter().any(|s| s == "*") {
        return true;
    }
    // Direct match
    if token_scopes.iter().any(|s| s == required) {
        return true;
    }
    // Category wildcard: "dag.*" matches "dag.read" but NOT "dag" (no sub-permission).
    // BUG-068: Only check category wildcard when required has a dotted sub-permission.
    if required.contains('.') {
        if let Some(category) = required.split('.').next() {
            let wildcard = format!("{}.*", category);
            if token_scopes.iter().any(|s| s == &wildcard) {
                return true;
            }
        }
    }
    false
}

// ──────────────────────────── IP Allowlist ─────────────────────────────────

/// Check if a client IP is allowed.
pub async fn check_ip_allowed(
    db: &dyn DatabaseBackend,
    client_ip: &str,
) -> Result<bool> {
    let rules = db.get_ip_allowlist().await?;
    if rules.is_empty() {
        // NOTE: No IP allowlist rules configured — allowing all IPs by default.
        // To restrict access, add IP rules via the API or database.
        tracing::debug!("ip_allowlist: no rules configured, allowing all IPs");
        return Ok(true);
    }

    let client: IpAddr = match client_ip.parse() {
        Ok(ip) => ip,
        Err(_) => return Ok(false),
    };

    for rule in &rules {
        let enabled = rule.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
        if !enabled {
            continue;
        }
        if let Some(cidr_str) = rule.get("cidr").and_then(|v| v.as_str()) {
            if cidr_contains(cidr_str, &client) {
                return Ok(true);
            }
        }
    }
    warn!(client_ip = %client_ip, "IP not in allowlist");
    Ok(false)
}

/// Simple CIDR check (supports /32 single IPs and /N subnets for IPv4).
fn cidr_contains(cidr: &str, addr: &IpAddr) -> bool {
    let parts: Vec<&str> = cidr.split('/').collect();
    let base: IpAddr = match parts[0].parse() {
        Ok(ip) => ip,
        Err(_) => return false,
    };

    if parts.len() == 1 {
        // Exact IP match
        return &base == addr;
    }

    let prefix_len: u32 = match parts[1].parse() {
        Ok(p) => p,
        Err(_) => return false,
    };

    match (base, addr) {
        (IpAddr::V4(base_v4), IpAddr::V4(addr_v4)) => {
            if prefix_len == 0 {
                return true;
            }
            if prefix_len > 32 {
                return false;
            }
            let mask = !0u32 << (32 - prefix_len);
            let base_bits = u32::from(base_v4);
            let addr_bits = u32::from(*addr_v4);
            (base_bits & mask) == (addr_bits & mask)
        }
        (IpAddr::V6(base_v6), IpAddr::V6(addr_v6)) => {
            if prefix_len == 0 {
                return true;
            }
            if prefix_len > 128 {
                return false;
            }
            let base_bits = u128::from(base_v6);
            let addr_bits = u128::from(*addr_v6);
            let mask = !0u128 << (128 - prefix_len);
            (base_bits & mask) == (addr_bits & mask)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_has_scope_direct() {
        let scopes = vec!["dag.read".into(), "dag.write".into()];
        assert!(token_has_scope(&scopes, "dag.read"));
        assert!(!token_has_scope(&scopes, "admin.users"));
    }

    #[test]
    fn test_token_has_scope_wildcard() {
        let scopes = vec!["*".into()];
        assert!(token_has_scope(&scopes, "anything"));
    }

    #[test]
    fn test_token_has_scope_category_wildcard() {
        let scopes = vec!["dag.*".into()];
        assert!(token_has_scope(&scopes, "dag.read"));
        assert!(token_has_scope(&scopes, "dag.execute"));
        assert!(!token_has_scope(&scopes, "admin.users"));
    }

    #[test]
    fn test_cidr_contains_exact() {
        let ip: IpAddr = "10.0.1.5".parse().unwrap();
        assert!(cidr_contains("10.0.1.5/32", &ip));
        assert!(!cidr_contains("10.0.1.6/32", &ip));
    }

    #[test]
    fn test_cidr_contains_subnet() {
        let ip: IpAddr = "10.0.1.5".parse().unwrap();
        assert!(cidr_contains("10.0.0.0/8", &ip));
        assert!(cidr_contains("10.0.1.0/24", &ip));
        assert!(!cidr_contains("192.168.0.0/16", &ip));
    }

    #[test]
    fn test_generate_and_verify_token() {
        let token = generate_token();
        assert!(token.starts_with("vtx_"));
        let hash = hash_token(&token).unwrap();
        assert!(verify_token(&token, &hash));
        assert!(!verify_token("wrong_token", &hash));
    }
}
