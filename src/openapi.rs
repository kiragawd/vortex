#![allow(dead_code)]
// OpenAPI Spec Generation, API Versioning & Rate Limiting
//
// Provides:
// - Programmatic OpenAPI 3.1 spec generation for all Ryuo API routes
// - API version extraction middleware (header or path-based)
// - Per-endpoint token-bucket rate limiter
//
// NOTE (BUG-077): This spec is hand-maintained and does not cover all endpoints.
// Missing: /api/v1/admin/secrets/rotate, /api/runs, /api/dags/:id/backfill/progress,
// /api/dags/:id/validate, /api/auth/logout, approval v1 aliases.
// TODO: Auto-generate from route definitions.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── OpenAPI Spec ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: OpenApiInfo,
    pub servers: Vec<OpenApiServer>,
    pub paths: HashMap<String, HashMap<String, OpenApiOperation>>,
    pub components: OpenApiComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiInfo {
    pub title: String,
    pub description: String,
    pub version: String,
    pub contact: OpenApiContact,
    pub license: OpenApiLicense,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiContact {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiLicense {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiServer {
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiOperation {
    pub summary: String,
    pub tags: Vec<String>,
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub responses: HashMap<String, OpenApiResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<OpenApiParameter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<Vec<HashMap<String, Vec<String>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiResponse {
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiParameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: String,
    pub required: bool,
    pub schema: OpenApiSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiComponents {
    #[serde(rename = "securitySchemes")]
    pub security_schemes: HashMap<String, OpenApiSecurityScheme>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenApiSecurityScheme {
    #[serde(rename = "type")]
    pub scheme_type: String,
    pub scheme: String,
    #[serde(rename = "bearerFormat")]
    pub bearer_format: String,
}

struct RouteEntry {
    method: &'static str,
    path: &'static str,
    summary: &'static str,
    tag: &'static str,
    operation_id: &'static str,
}

pub fn generate_openapi_spec() -> OpenApiSpec {
    let routes: Vec<RouteEntry> = vec![
        // Health & Metrics
        RouteEntry { method: "get", path: "/health", summary: "Health check", tag: "System", operation_id: "getHealth" },
        RouteEntry { method: "get", path: "/metrics", summary: "Prometheus metrics", tag: "System", operation_id: "getMetrics" },
        // Auth
        RouteEntry { method: "post", path: "/api/login", summary: "Authenticate user", tag: "Auth", operation_id: "login" },
        RouteEntry { method: "get", path: "/api/auth/providers", summary: "List auth providers", tag: "Auth", operation_id: "getAuthProviders" },
        RouteEntry { method: "post", path: "/api/auth/providers", summary: "Create auth provider", tag: "Auth", operation_id: "createAuthProvider" },
        RouteEntry { method: "get", path: "/api/auth/oidc/authorize", summary: "OIDC authorize redirect", tag: "Auth", operation_id: "oidcAuthorize" },
        RouteEntry { method: "get", path: "/api/auth/oidc/callback", summary: "OIDC callback", tag: "Auth", operation_id: "oidcCallback" },
        RouteEntry { method: "post", path: "/api/auth/saml/acs", summary: "SAML ACS endpoint", tag: "Auth", operation_id: "samlAcs" },
        RouteEntry { method: "get", path: "/api/auth/sessions", summary: "List user sessions", tag: "Auth", operation_id: "getUserSessions" },
        RouteEntry { method: "post", path: "/api/auth/sessions/cleanup", summary: "Cleanup expired sessions", tag: "Auth", operation_id: "cleanupSessions" },
        // DAGs
        RouteEntry { method: "get", path: "/api/dags", summary: "List all DAGs", tag: "DAGs", operation_id: "getDags" },
        RouteEntry { method: "post", path: "/api/dags/upload", summary: "Upload a DAG", tag: "DAGs", operation_id: "uploadDag" },
        RouteEntry { method: "get", path: "/api/dags/{id}/tasks", summary: "Get DAG tasks", tag: "DAGs", operation_id: "getDagTasks" },
        RouteEntry { method: "get", path: "/api/dags/{id}/runs", summary: "Get DAG runs", tag: "DAGs", operation_id: "getDagRuns" },
        RouteEntry { method: "post", path: "/api/dags/{id}/trigger", summary: "Trigger DAG execution", tag: "DAGs", operation_id: "triggerDag" },
        RouteEntry { method: "patch", path: "/api/dags/{id}/pause", summary: "Pause DAG", tag: "DAGs", operation_id: "pauseDag" },
        RouteEntry { method: "patch", path: "/api/dags/{id}/unpause", summary: "Unpause DAG", tag: "DAGs", operation_id: "unpauseDag" },
        RouteEntry { method: "patch", path: "/api/dags/{id}/schedule", summary: "Update DAG schedule", tag: "DAGs", operation_id: "updateSchedule" },
        RouteEntry { method: "post", path: "/api/dags/{id}/backfill", summary: "Backfill DAG runs", tag: "DAGs", operation_id: "backfillDag" },
        RouteEntry { method: "get", path: "/api/dags/{id}/source", summary: "Get DAG source code", tag: "DAGs", operation_id: "getDagSource" },
        RouteEntry { method: "get", path: "/api/dags/{id}/versions", summary: "List DAG versions", tag: "DAGs", operation_id: "getDagVersions" },
        RouteEntry { method: "post", path: "/api/dags/{id}/retry", summary: "Retry failed DAG run", tag: "DAGs", operation_id: "retryDag" },
        // Tasks
        RouteEntry { method: "get", path: "/api/tasks/{id}/logs", summary: "Get task logs", tag: "Tasks", operation_id: "getTaskLogs" },
        // Swarm
        RouteEntry { method: "get", path: "/api/swarm/status", summary: "Swarm cluster status", tag: "Swarm", operation_id: "swarmStatus" },
        RouteEntry { method: "get", path: "/api/swarm/workers", summary: "List swarm workers", tag: "Swarm", operation_id: "swarmWorkers" },
        RouteEntry { method: "post", path: "/api/swarm/workers/{id}/drain", summary: "Drain a worker", tag: "Swarm", operation_id: "swarmDrainWorker" },
        RouteEntry { method: "delete", path: "/api/swarm/workers/{id}", summary: "Remove a worker", tag: "Swarm", operation_id: "swarmRemoveWorker" },
        // Secrets
        RouteEntry { method: "get", path: "/api/secrets", summary: "List secrets", tag: "Secrets", operation_id: "getSecrets" },
        RouteEntry { method: "post", path: "/api/secrets", summary: "Store a secret", tag: "Secrets", operation_id: "storeSecret" },
        RouteEntry { method: "delete", path: "/api/secrets/{key}", summary: "Delete a secret", tag: "Secrets", operation_id: "deleteSecret" },
        // Users
        RouteEntry { method: "get", path: "/api/users", summary: "List users", tag: "Users", operation_id: "getUsers" },
        RouteEntry { method: "post", path: "/api/users", summary: "Create user", tag: "Users", operation_id: "createUser" },
        RouteEntry { method: "delete", path: "/api/users/{username}", summary: "Delete user", tag: "Users", operation_id: "deleteUser" },
        // XCom
        RouteEntry { method: "post", path: "/api/xcom/push", summary: "Push XCom value", tag: "XCom", operation_id: "xcomPush" },
        RouteEntry { method: "get", path: "/api/xcom/pull", summary: "Pull XCom value", tag: "XCom", operation_id: "xcomPull" },
        // Pools
        RouteEntry { method: "get", path: "/api/pools", summary: "List pools", tag: "Pools", operation_id: "listPools" },
        RouteEntry { method: "post", path: "/api/pools", summary: "Create pool", tag: "Pools", operation_id: "createPool" },
        RouteEntry { method: "get", path: "/api/pools/{name}", summary: "Get pool by name", tag: "Pools", operation_id: "getPool" },
        // Teams
        RouteEntry { method: "get", path: "/api/teams", summary: "List teams", tag: "Teams", operation_id: "getTeams" },
        RouteEntry { method: "post", path: "/api/teams", summary: "Create team", tag: "Teams", operation_id: "createTeam" },
        // Lineage
        RouteEntry { method: "get", path: "/api/lineage/events/{dag_id}", summary: "Get lineage events", tag: "Lineage", operation_id: "getLineageEvents" },
        RouteEntry { method: "get", path: "/api/lineage/datasets", summary: "List lineage datasets", tag: "Lineage", operation_id: "getLineageDatasets" },
        // Incidents
        RouteEntry { method: "get", path: "/api/incidents/configs", summary: "List incident configs", tag: "Incidents", operation_id: "getIncidentConfigs" },
        RouteEntry { method: "post", path: "/api/incidents/configs", summary: "Create incident config", tag: "Incidents", operation_id: "createIncidentConfig" },
        RouteEntry { method: "delete", path: "/api/incidents/configs/{id}", summary: "Delete incident config", tag: "Incidents", operation_id: "deleteIncidentConfig" },
        // Compliance
        RouteEntry { method: "get", path: "/api/audit/log", summary: "Query audit log", tag: "Compliance", operation_id: "getAuditLog" },
        RouteEntry { method: "get", path: "/api/approval/gates", summary: "List approval gates", tag: "Compliance", operation_id: "getApprovalGates" },
        RouteEntry { method: "post", path: "/api/approval/gates", summary: "Create approval gate", tag: "Compliance", operation_id: "createApprovalGate" },
        RouteEntry { method: "get", path: "/api/approval/requests", summary: "List approval requests", tag: "Compliance", operation_id: "getApprovalRequests" },
        RouteEntry { method: "post", path: "/api/approval/requests", summary: "Submit approval request", tag: "Compliance", operation_id: "createApprovalRequest" },
        RouteEntry { method: "post", path: "/api/approval/requests/{id}/approve", summary: "Approve request", tag: "Compliance", operation_id: "approveRequest" },
        RouteEntry { method: "post", path: "/api/approval/requests/{id}/reject", summary: "Reject request", tag: "Compliance", operation_id: "rejectRequest" },
        RouteEntry { method: "get", path: "/api/retention/policies", summary: "List retention policies", tag: "Compliance", operation_id: "getRetentionPolicies" },
        RouteEntry { method: "get", path: "/api/compliance/controls", summary: "List compliance controls", tag: "Compliance", operation_id: "getComplianceControls" },
        RouteEntry { method: "get", path: "/api/compliance/summary/{framework}", summary: "Get compliance summary", tag: "Compliance", operation_id: "getComplianceSummary" },
        // RBAC
        RouteEntry { method: "get", path: "/api/rbac/roles", summary: "List RBAC roles", tag: "RBAC", operation_id: "getRbacRoles" },
        RouteEntry { method: "get", path: "/api/rbac/roles/{role_id}/permissions", summary: "Get role permissions", tag: "RBAC", operation_id: "getRolePermissions" },
        RouteEntry { method: "get", path: "/api/rbac/users/{user_id}/roles", summary: "Get user roles", tag: "RBAC", operation_id: "getUserRoles" },
        RouteEntry { method: "post", path: "/api/rbac/users/{user_id}/roles", summary: "Assign role to user", tag: "RBAC", operation_id: "assignUserRole" },
        RouteEntry { method: "get", path: "/api/rbac/users/{user_id}/permissions", summary: "Get user permissions", tag: "RBAC", operation_id: "getUserPermissions" },
        // Tokens
        RouteEntry { method: "get", path: "/api/tokens", summary: "List API tokens", tag: "Tokens", operation_id: "getApiTokens" },
        RouteEntry { method: "post", path: "/api/tokens", summary: "Create API token", tag: "Tokens", operation_id: "createApiToken" },
        RouteEntry { method: "post", path: "/api/tokens/{id}/revoke", summary: "Revoke API token", tag: "Tokens", operation_id: "revokeApiToken" },
        // Network
        RouteEntry { method: "get", path: "/api/network/ip-allowlist", summary: "List IP allowlist", tag: "Network", operation_id: "getIpAllowlist" },
        RouteEntry { method: "post", path: "/api/network/ip-allowlist", summary: "Add IP allowlist rule", tag: "Network", operation_id: "createIpAllowlistRule" },
        RouteEntry { method: "delete", path: "/api/network/ip-allowlist/{id}", summary: "Delete IP allowlist rule", tag: "Network", operation_id: "deleteIpAllowlistRule" },
    ];

    let mut paths: HashMap<String, HashMap<String, OpenApiOperation>> = HashMap::new();

    for route in &routes {
        let secured = !matches!(
            route.path,
            "/health" | "/metrics" | "/api/login" | "/api/auth/oidc/authorize"
                | "/api/auth/oidc/callback" | "/api/auth/saml/acs" | "/api/auth/providers/public"
        );

        // Extract path parameters
        let mut params = Vec::new();
        for segment in route.path.split('/') {
            if segment.starts_with('{') && segment.ends_with('}') {
                params.push(OpenApiParameter {
                    name: segment[1..segment.len() - 1].to_string(),
                    location: "path".to_string(),
                    required: true,
                    schema: OpenApiSchema {
                        schema_type: "string".to_string(),
                    },
                });
            }
        }

        let op = OpenApiOperation {
            summary: route.summary.to_string(),
            tags: vec![route.tag.to_string()],
            operation_id: route.operation_id.to_string(),
            responses: {
                let mut r = HashMap::new();
                r.insert(
                    "200".to_string(),
                    OpenApiResponse {
                        description: "Successful response".to_string(),
                    },
                );
                if secured {
                    r.insert(
                        "401".to_string(),
                        OpenApiResponse {
                            description: "Unauthorized".to_string(),
                        },
                    );
                }
                r
            },
            parameters: params,
            security: if secured {
                Some(vec![{
                    let mut m = HashMap::new();
                    m.insert("bearerAuth".to_string(), Vec::new());
                    m
                }])
            } else {
                None
            },
        };

        paths
            .entry(route.path.to_string())
            .or_default()
            .insert(route.method.to_string(), op);
    }

    let mut security_schemes = HashMap::new();
    security_schemes.insert(
        "bearerAuth".to_string(),
        OpenApiSecurityScheme {
            scheme_type: "http".to_string(),
            scheme: "bearer".to_string(),
            bearer_format: "JWT".to_string(),
        },
    );

    OpenApiSpec {
        openapi: "3.1.0".to_string(),
        info: OpenApiInfo {
            title: "Ryuo Orchestration Platform API".to_string(),
            description: "Enterprise-grade workflow orchestration API".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            contact: OpenApiContact {
                name: "Ryuo Team".to_string(),
                url: "https://github.com/ryuo-orchestration/ryuo".to_string(),
            },
            license: OpenApiLicense {
                name: "Apache-2.0".to_string(),
            },
        },
        servers: vec![
            OpenApiServer {
                url: "/".to_string(),
                description: "Current server".to_string(),
            },
        ],
        paths,
        components: OpenApiComponents {
            security_schemes,
        },
    }
}

// ─── API Version Middleware ──────────────────────────────────

/// Supported API versions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiVersion {
    V1,
}

impl ApiVersion {
    pub fn from_header(headers: &HeaderMap) -> Self {
        if let Some(val) = headers.get("X-API-Version") {
            if let Ok(s) = val.to_str() {
                return match s {
                    "1" | "v1" => ApiVersion::V1,
                    _ => ApiVersion::V1, // default
                };
            }
        }
        ApiVersion::V1
    }
}

/// Middleware that extracts API version from X-API-Version header
/// and injects it as a request extension.
pub async fn api_version_middleware(request: Request, next: Next) -> Response {
    let version = ApiVersion::from_header(request.headers());
    let mut request = request;
    request.extensions_mut().insert(version);
    next.run(request).await
}

// ─── Rate Limiter ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per second
    pub rps: f64,
    /// Maximum burst size
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            rps: 100.0,
            burst: 200,
        }
    }
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: std::time::Instant,
}

impl TokenBucket {
    fn new(config: &RateLimitConfig) -> Self {
        Self {
            tokens: config.burst as f64,
            max_tokens: config.burst as f64,
            refill_rate: config.rps,
            last_refill: std::time::Instant::now(),
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub async fn check(&self, key: &str) -> bool {
        let mut buckets = self.buckets.lock().await;
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(&self.config));
        bucket.try_consume()
    }
}

/// Rate limiting middleware — keyed by client IP or "anonymous"
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    let key = request
        .headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("anonymous").trim().to_string())
        .unwrap_or_else(|| "anonymous".to_string());

    if !limiter.check(&key).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", "1")],
            "Rate limit exceeded",
        )
            .into_response();
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_spec_generation() {
        let spec = generate_openapi_spec();
        assert_eq!(spec.openapi, "3.1.0");
        assert!(!spec.paths.is_empty());
        assert!(spec.paths.contains_key("/api/dags"));
        assert!(spec.paths.contains_key("/health"));
        assert!(spec.paths.contains_key("/api/rbac/roles"));

        // Verify DAG has get method
        let dags = spec.paths.get("/api/dags").unwrap();
        assert!(dags.contains_key("get"));
        assert_eq!(dags["get"].tags, vec!["DAGs"]);
    }

    #[test]
    fn test_openapi_path_parameters() {
        let spec = generate_openapi_spec();
        let trigger = spec
            .paths
            .get("/api/dags/{id}/trigger")
            .unwrap()
            .get("post")
            .unwrap();
        assert_eq!(trigger.parameters.len(), 1);
        assert_eq!(trigger.parameters[0].name, "id");
        assert_eq!(trigger.parameters[0].location, "path");
    }

    #[test]
    fn test_openapi_security() {
        let spec = generate_openapi_spec();
        // Health should have no security
        let health = spec.paths.get("/health").unwrap().get("get").unwrap();
        assert!(health.security.is_none());

        // DAGs should require auth
        let dags = spec.paths.get("/api/dags").unwrap().get("get").unwrap();
        assert!(dags.security.is_some());
    }

    #[test]
    fn test_api_version_from_header() {
        let mut headers = HeaderMap::new();
        assert_eq!(ApiVersion::from_header(&headers), ApiVersion::V1);

        headers.insert("X-API-Version", "v1".parse().unwrap());
        assert_eq!(ApiVersion::from_header(&headers), ApiVersion::V1);
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(RateLimitConfig {
            rps: 2.0,
            burst: 3,
        });

        // Should allow burst
        assert!(limiter.check("test").await);
        assert!(limiter.check("test").await);
        assert!(limiter.check("test").await);

        // Should deny after burst
        assert!(!limiter.check("test").await);

        // Different key should still work
        assert!(limiter.check("other").await);
    }
}
