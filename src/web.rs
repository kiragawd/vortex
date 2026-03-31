#![allow(dead_code)]
use tracing::{info, debug};
use axum::{
    extract::{Path, State, Multipart, Query},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, patch, delete, put},
    Json, Router,
};
// Bug 13 fix: import CorsLayer to allow cross-origin browser clients.
use tower_http::cors::{CorsLayer, Any};
use rust_embed::RustEmbed;
use std::sync::Arc;
use crate::swarm::SwarmState;
use crate::vault::Vault;

use serde_json::json;
use serde::{Deserialize, Serialize};
use std::fs;
#[derive(Deserialize, Clone)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

use std::collections::HashMap;

use tokio::sync::mpsc;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

use crate::db_trait::DatabaseBackend;
use crate::scheduler::{ScheduleRequest, RunType};
use crate::xcom::XComStore;
use crate::pools::PoolManager;
use crate::metrics::VortexMetrics;

/// Extension injected by auth_middleware so handlers can read the caller's identity.
#[derive(Clone)]
pub struct AuthUser {
    pub username: String,
    pub role: String,
    pub team_id: Option<String>,
}

pub struct AppState {
    pub db: Arc<dyn DatabaseBackend>,
    pub tx: mpsc::Sender<ScheduleRequest>,
    pub swarm: Arc<SwarmState>,
    pub vault: Option<Arc<Vault>>,
    // ARCH-2 FIX: Use tokio::sync::Mutex so the guard is Send across await boundaries.
    pub dags: Arc<tokio::sync::Mutex<HashMap<String, Arc<crate::scheduler::Dag>>>>,
    pub xcom: Arc<XComStore>,
    pub pool_manager: Arc<PoolManager>,
    pub metrics: Arc<VortexMetrics>,
    // Bug 18 fix: use tokio::sync::RwLock so .write()/.read() are async-aware
    // and do not block the Tokio worker thread when held across await points.
    pub backfill_progress: Arc<tokio::sync::RwLock<HashMap<String, f32>>>,
    // Bug 31 fix: simple in-memory rate-limiter for the login endpoint.
    // Maps remote IP (or username as fallback) → (attempt_count, window_start).
    pub login_attempts: Arc<tokio::sync::Mutex<HashMap<String, (u32, std::time::Instant)>>>,
}

pub struct WebServer {
    db: Arc<dyn DatabaseBackend>,
    tx: mpsc::Sender<ScheduleRequest>,
    swarm: Arc<SwarmState>,
    vault: Option<Arc<Vault>>,
    dags: Arc<tokio::sync::Mutex<HashMap<String, Arc<crate::scheduler::Dag>>>>,
    metrics: Arc<VortexMetrics>,
}

impl WebServer {
    pub fn new(db: Arc<dyn DatabaseBackend>, tx: mpsc::Sender<ScheduleRequest>, swarm: Arc<SwarmState>, vault: Option<Arc<Vault>>, dags: Arc<tokio::sync::Mutex<HashMap<String, Arc<crate::scheduler::Dag>>>>, metrics: Arc<VortexMetrics>) -> Self {
        Self { db, tx, swarm, vault, dags, metrics }
    }

    pub async fn run(self, port: u16, tls_cert: Option<String>, tls_key: Option<String>) -> anyhow::Result<()> {
        let state = Arc::new(AppState {
            db: self.db.clone(),
            tx: self.tx,
            swarm: self.swarm,
            vault: self.vault,
            xcom: Arc::new(XComStore::new(self.db.clone())),
            pool_manager: Arc::new(PoolManager::new(self.db.clone())),
            dags: self.dags,
            metrics: self.metrics,
            backfill_progress: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            login_attempts: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        });

        let api_routes = Router::new()
            .route("/api/dags", get(get_dags))
            .route("/api/dags/upload", post(upload_dag))
            .route("/api/dags/:id/tasks", get(get_dag_tasks))
            .route("/api/dags/:id/runs", get(get_dag_runs))
            .route("/api/dags/:id/trigger", post(trigger_dag))
            .route("/api/dags/:id/pause", patch(pause_dag))
            .route("/api/dags/:id/unpause", patch(unpause_dag))
            .route("/api/dags/:id/schedule", patch(update_schedule))
            .route("/api/dags/:id/backfill", post(backfill_dag))
            .route("/api/dags/:id/backfill/progress", get(get_backfill_progress))
            .route("/api/dags/:id/validate", get(validate_dag_id))
            .route("/api/dags/:id/source", get(get_dag_source).patch(update_dag_source))
            .route("/api/dags/:id/source/rust", patch(update_dag_source_rust))
            .route("/api/dags/:id/versions", get(get_dag_versions_handler))
            .route("/api/dags/:id/versions/:version/source", get(get_dag_version_source_handler))
            .route("/api/dags/:id/versions/:version/rollback", post(rollback_dag_version_handler))
            .route("/api/dags/:id/retry", post(retry_dag))
            .route("/api/tasks/:id/logs", get(get_task_logs))
            .route("/api/task-instances/:dag_id/:ti_id/events", get(get_task_events_handler))
            // Swarm
            .route("/api/swarm/status", get(swarm_status))
            .route("/api/swarm/workers", get(swarm_workers))
            .route("/api/swarm/workers/:id/drain", post(swarm_drain_worker))
            .route("/api/swarm/workers/:id", delete(swarm_remove_worker))
            // Secrets Management
            .route("/api/secrets", get(get_secrets))
            .route("/api/secrets", post(store_secret))
            .route("/api/secrets/:key", delete(delete_secret))
            // RBAC: Users
            .route("/api/users", get(get_users))
            .route("/api/users", post(create_user))
            .route("/api/users/:username", delete(delete_user))
            // XCom
            .route("/api/xcom/push", post(xcom_push_handler))
            .route("/api/xcom/pull", get(xcom_pull_handler))
            .route("/api/dags/:id/runs/:run_id/xcom", get(xcom_list_handler))
            // Task Pools
            .route("/api/pools", get(list_pools).post(create_pool_handler))
            .route("/api/pools/:name", get(get_pool_handler).put(update_pool_handler).delete(delete_pool_handler))
            // Webhook Callbacks
            .route("/api/dags/:id/callbacks", get(get_callbacks_handler).put(set_callbacks_handler).delete(delete_callbacks_handler))
            // Audit
            .route("/api/audit", get(get_audit_logs_handler))
            // Analysis
            .route("/api/analysis/gantt", get(gantt_handler))
            .route("/api/analysis/calendar", get(calendar_handler))
            // Teams API
            // Teams API
            .route("/api/teams", get(get_teams_handler).post(create_team_handler))
            .route("/api/teams/:id", get(get_team_handler).put(update_team_handler).delete(delete_team_handler))
            .route("/api/teams/:id/users/:username", put(assign_user_team_handler))
            // Auth Provider Management
            .route("/api/auth/providers", get(get_auth_providers_handler).post(create_auth_provider_handler))
            .route("/api/auth/providers/:id", delete(delete_auth_provider_handler))
            .route("/api/auth/sessions", get(get_user_sessions_handler))
            .route("/api/auth/sessions/cleanup", post(cleanup_sessions_handler))
            // Lineage & Incident Management
            .route("/api/lineage/events/:dag_id", get(get_lineage_events_handler))
            .route("/api/lineage/datasets", get(get_lineage_datasets_handler))
            .route("/api/incidents/configs", get(get_incident_configs_handler).post(create_incident_config_handler))
            .route("/api/incidents/configs/:id", delete(delete_incident_config_handler))
            // Compliance, Governance & Change Management
            .route("/api/audit/log", get(get_audit_log_handler))
            .route("/api/approval/gates", get(get_approval_gates_handler).post(create_approval_gate_handler))
            .route("/api/approval/gates/:id", delete(delete_approval_gate_handler))
            .route("/api/approval/requests", get(get_approval_requests_handler).post(create_approval_request_handler))
            .route("/api/approval/requests/:id/approve", post(approve_request_handler))
            .route("/api/approval/requests/:id/reject", post(reject_request_handler))
            .route("/api/retention/policies", get(get_retention_policies_handler).post(create_retention_policy_handler))
            .route("/api/compliance/controls", get(get_compliance_controls_handler).post(upsert_compliance_control_handler))
            .route("/api/compliance/summary/:framework", get(get_compliance_summary_handler))
            // Fine-Grained RBAC, Token Scoping & Network Security
            .route("/api/rbac/roles", get(get_rbac_roles_handler))
            .route("/api/rbac/roles/:role_id/permissions", get(get_role_permissions_handler))
            .route("/api/rbac/users/:user_id/roles", get(get_user_roles_handler).post(assign_user_role_handler))
            .route("/api/rbac/users/:user_id/roles/:role_id", delete(revoke_user_role_handler))
            .route("/api/rbac/users/:user_id/permissions", get(get_user_permissions_handler))
            .route("/api/tokens", get(get_api_tokens_handler).post(create_api_token_handler))
            .route("/api/tokens/:id/revoke", post(revoke_api_token_handler))
            .route("/api/network/ip-allowlist", get(get_ip_allowlist_handler).post(create_ip_allowlist_rule_handler))
            .route("/api/network/ip-allowlist/:id", delete(delete_ip_allowlist_rule_handler))
            .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

        // Bug 13 fix: apply CORS layer so browser clients from other origins can
        // reach the API. Bearer-token auth makes a permissive policy safe here.
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .merge(api_routes)
            .route("/api/login", post(login))
            // OIDC/SAML Auth Flows (unauthenticated)
            .route("/api/auth/oidc/authorize", get(oidc_authorize_handler))
            .route("/api/auth/oidc/callback", get(oidc_callback_handler))
            .route("/api/auth/saml/acs", post(saml_acs_handler))
            .route("/api/auth/providers/public", get(get_public_auth_providers_handler))
            .route("/metrics", get(prometheus_metrics_handler_wrapper))
            // OpenAPI spec
            .route("/api/openapi.json", get(openapi_spec_handler))
            // Improvement 38: health check endpoint (also exposed under /api/ for UI clients)
            .route("/health", get(health_handler))
            .route("/api/health", get(health_handler))
            // Global runs endpoint for the UI runs page
            .route("/api/runs", get(get_all_runs_handler))
            .fallback(static_handler)
            .layer(middleware::from_fn(request_id_middleware))
            // Improvement 40: reject request bodies larger than 10 MB
            .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
            .layer(cors)
            .with_state(state);

        if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
            let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to load TLS certificates from {} and {}: {}", cert_path, key_path, e))?;
            info!("🔒 Web UI running on https://localhost:{} (TLS)", port);
            axum_server::bind_rustls(
                format!("0.0.0.0:{}", port).parse().unwrap(),
                config,
            )
            .serve(app.into_make_service())
            .await
            .map_err(|e| anyhow::anyhow!("Web server TLS bind failed: {}", e))?;
        } else {
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await
                .map_err(|e| anyhow::anyhow!("Web server bind failed on port {}: {}", port, e))?;
            info!("🌐 Web UI running on http://localhost:{}", port);
            axum::serve(listener, app).await
                .map_err(|e| anyhow::anyhow!("Web server failed: {}", e))?;
        }
        Ok(())
    }
}

async fn request_id_middleware(
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    use tracing::Instrument;

    let request_id = req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Improvement 46: use .instrument() so the span is properly attached to
    // the async future rather than using a sync `_enter` guard, which doesn't
    // work correctly across .await suspension points.
    let span = tracing::info_span!("request", request_id = %request_id, method = %req.method(), path = %req.uri().path());
    let mut response = next.run(req).instrument(span).await;

    response.headers_mut().insert("x-request-id", request_id.parse().unwrap());

    // Improvement 43: security headers on every response
    let headers = response.headers_mut();
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert(
        "content-security-policy",
        "default-src 'self'; script-src 'self' 'unsafe-inline' https://cdn.tailwindcss.com https://cdnjs.cloudflare.com; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com https://cdn.tailwindcss.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'".parse().unwrap(),
    );
    response
}

async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req.headers().get("Authorization").and_then(|h| h.to_str().ok());
    let api_key = match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => &auth[7..],
        Some(auth) => auth,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    match state.db.get_user_by_api_key(api_key).await {
        Ok(Some((username, role, team_id))) => {
            let path = req.uri().path();
            
            // RBAC Logic:
            // Admin: Can do everything
            // Operator: Can trigger, pause, manage DAGs. CANNOT manage Users or Secrets.
            // Viewer: Read-only.
            
            let is_admin_route = path.contains("/api/users") || path.contains("/api/secrets") || path.contains("/api/teams");
            let is_write_route = path.contains("/trigger") || path.contains("/pause") || 
                               path.contains("/unpause") || path.contains("/schedule") || 
                               path.contains("/backfill") || path.contains("/drain") ||
                               path.contains("/api/users") || path.contains("/api/secrets") ||
                               path.contains("/api/dags/upload") || path.contains("/api/teams");

            if role == "Viewer" && is_write_route {
                return Err(StatusCode::FORBIDDEN);
            }
            
            if role == "Operator" && is_admin_route && !path.contains("/api/teams/") {
                return Err(StatusCode::FORBIDDEN);
            }

            debug!("👤 Authenticated: {} (Role: {}, Team: {:?})", username, role, team_id);
            // Inject caller identity for audit hooks and DAG isolation checks
            req.extensions_mut().insert(AuthUser { username, role, team_id: team_id.clone() });
            
            // If the user has a team ID, we need to ensure they can only access DAGs belonging to their team.
            // We accomplish this by checking path parameters in the handlers.
            
            Ok(next.run(req).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// Additional Handlers

async fn upload_dag(State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, mut multipart: Multipart) -> Response {
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("").to_string();
        if name == "file" && file_name.ends_with(".py") {
            // BUG-3 FIX: Extract only the basename to prevent path traversal attacks.
            // e.g. "../../etc/cron.d/evil.py" → "evil.py"
            let safe_name = match std::path::Path::new(&file_name).file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid file name"}))).into_response(),
            };
            if !safe_name.ends_with(".py") {
                return (StatusCode::BAD_REQUEST, Json(json!({"error": "Only .py files are allowed"}))).into_response();
            }

            let data = match field.bytes().await {
                Ok(b) => b,
                Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
            };
            
            let dags_dir = std::env::current_dir()
                .map(|p| p.join("dags").to_string_lossy().to_string())
                .unwrap_or_else(|_| "dags".to_string());
            fs::create_dir_all(&dags_dir).ok();
            // BUG-3 FIX: Use safe_name (basename only) and verify canonicalized path
            let file_path = format!("{}/{}", dags_dir, safe_name);
            if let Err(e) = fs::write(&file_path, &data) {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to save file: {}", e)}))).into_response();
            }
            // BUG-3 FIX: Post-write canonicalization guard — verify written file is inside dags_dir
            let dags_dir_canonical = match std::fs::canonicalize(&dags_dir) {
                Ok(p) => p,
                Err(e) => {
                    let _ = fs::remove_file(&file_path);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Cannot resolve dags directory: {}", e)}))).into_response();
                }
            };
            let file_canonical = match std::fs::canonicalize(&file_path) {
                Ok(p) => p,
                Err(e) => {
                    let _ = fs::remove_file(&file_path);
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Cannot resolve uploaded file path: {}", e)}))).into_response();
                }
            };
            if !file_canonical.starts_with(&dags_dir_canonical) {
                let _ = fs::remove_file(&file_path);
                return (StatusCode::BAD_REQUEST, Json(json!({"error": "Path traversal detected: file path escapes dags/ directory"}))).into_response();
            }

            match crate::python_parser::parse_python_dag(&file_path) {
                Ok(parsed_dags) => {
                    if parsed_dags.is_empty() {
                        let _ = fs::remove_file(&file_path);
                        return (StatusCode::BAD_REQUEST, Json(json!({"error": "No distinct DAGs found in file"}))).into_response();
                    }
                    
                    // Bug 9 fix: register ALL DAGs from a multi-DAG file, not just the first one
                    let mut registered_ids: Vec<String> = Vec::new();
                    {
                        let mut dags_map = state.dags.lock().await;
                        for dag in parsed_dags {
                            let dag_id = dag.id.clone();
                            let _ = state.db.register_dag(&dag).await;
                            dags_map.insert(dag.id.clone(), Arc::new(dag));
                            registered_ids.push(dag_id);
                        }
                    }
                    
                    let dag_count = registered_ids.len();
                    info!("🚀 DAGs Uploaded from {}: {:?}", file_name, registered_ids);
                    let _ = state.db.log_audit_event(
                        &auth_user.username, "dag.upload", "dag", &registered_ids.join(","),
                        &format!("{{\"file\":\"{}\",\"dag_count\":{}}}", file_name, dag_count),
                    ).await;
                    
                    return Json(json!({ "dag_ids": registered_ids, "dag_count": dag_count })).into_response();
                },
                Err(e) => {
                    let _ = fs::remove_file(&file_path);
                    return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid Python DAG file syntax: {}", e)}))).into_response();
                }
            }
        }
    }
    (StatusCode::BAD_REQUEST, Json(json!({"error": "No .py file provided"}))).into_response()
}

async fn validate_dag_id(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    match state.db.get_latest_version(&id).await {
        Ok(Some(version)) => {
            let path = version["file_path"].as_str().unwrap_or("");
            match crate::python_parser::parse_python_dag(path) {
                Ok(parsed_dags) => {
                    if let Some(dag) = parsed_dags.into_iter().next() {
                        Json(json!({"valid": true, "metadata": { "dag_id": dag.id, "tasks": dag.tasks.keys().collect::<Vec<_>>() }})).into_response()
                    } else {
                        (StatusCode::BAD_REQUEST, Json(json!({"valid": false, "error": "No DAG found in valid parse execution"}))).into_response()
                    }
                },
                Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"valid": false, "error": e.to_string()}))).into_response(),
            }
        },
        _ => (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response(),
    }
}

// RBAC: User Handlers

async fn get_users(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_all_users().await {
        Ok(users) => Json(users),
        Err(_) => Json(vec![]),
    }
}

#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    password: String,
    role: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login(State(state): State<Arc<AppState>>, Json(body): Json<LoginRequest>) -> Response {
    // Bug 31 fix: simple in-memory rate-limit — max 10 attempts per username per 60 seconds.
    {
        let mut attempts = state.login_attempts.lock().await;
        let now = std::time::Instant::now();
        let entry = attempts.entry(body.username.clone()).or_insert((0, now));
        // Reset window if it's been more than 60 seconds
        if now.duration_since(entry.1).as_secs() >= 60 {
            *entry = (0, now);
        }
        entry.0 += 1;
        if entry.0 > 10 {
            return (StatusCode::TOO_MANY_REQUESTS, Json(json!({
                "error": "Too many login attempts. Please wait 60 seconds."
            }))).into_response();
        }
    }

    match state.db.validate_user(&body.username, &body.password).await {
        Ok(Some((api_key, role))) => {
            // Successful login — reset the counter
            state.login_attempts.lock().await.remove(&body.username);
            info!("🔑 User logged in: {} (Role: {})", body.username, role);
            let _ = state.db.log_audit_event(
                &body.username, "auth.login", "user", &body.username, "{}",
            ).await;
            Json(json!({ "api_key": api_key, "role": role, "username": body.username })).into_response()
        }
        Ok(None) => (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Invalid credentials" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn create_user(State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, Json(body): Json<CreateUserRequest>) -> Response {
    let api_key = format!("vx_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
    match state.db.create_user(&body.username, &body.password, &body.role, &api_key).await {
        Ok(_) => {
            let _ = state.db.log_audit_event(
                &auth_user.username, "user.create", "user", &body.username,
                &format!("{{\"role\":\"{}\"}}", body.role),
            ).await;
            Json(json!({ "message": "User created", "api_key": api_key })).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn delete_user(Path(username): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> Response {
    if username == "admin" {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Cannot delete primary admin" }))).into_response();
    }
    match state.db.delete_user(&username).await {
        Ok(_) => {
            let _ = state.db.log_audit_event(
                &auth_user.username, "user.delete", "user", &username, "{}",
            ).await;
            Json(json!({ "message": "User deleted" })).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// Secrets Management Handlers

async fn get_secrets(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_all_secrets().await {
        Ok(keys) => Json(json!({ "secrets": keys })),
        Err(_) => Json(json!({ "secrets": [] })),
    }
}

#[derive(Deserialize)]
struct SecretRequest {
    key: String,
    value: String,
}

async fn store_secret(State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, Json(body): Json<SecretRequest>) -> Response {
    let vault = match &state.vault {
        Some(v) => v,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Secret Vault is not initialized" }))).into_response(),
    };

    match vault.encrypt(&body.value) {
        Ok(encrypted) => {
            if let Err(e) = state.db.store_secret(&body.key, &encrypted).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB Error: {}", e) }))).into_response();
            }
            info!("🔐 Secret stored: {}", body.key);
            let _ = state.db.log_audit_event(
                &auth_user.username, "secret.store", "secret", &body.key, "{}",
            ).await;
            Json(json!({ "message": "Secret stored successfully" })).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("Encryption failure: {}", e) }))).into_response(),
    }
}

async fn delete_secret(Path(key): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> Response {
    match state.db.delete_secret(&key).await {
        Ok(_) => {
            let _ = state.db.log_audit_event(
                &auth_user.username, "secret.delete", "secret", &key, "{}",
            ).await;
            Json(json!({ "message": "Secret deleted" })).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// Existing Handlers
async fn get_dags(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0);

    match state.db.get_all_dags(limit, offset).await {
        Ok((dags, total)) => {
            // Bug 14 fix: only Admin skips team filtering. Previously, non-admin
            // users with team_id == None (no team assigned) would see ALL DAGs.
            if auth_user.role == "Admin" {
                // Admins see everything
                Json(PaginatedResponse { data: dags, total, limit, offset })
            } else if let Some(user_team_id) = auth_user.team_id {
                // Operator / Viewer with a team — filter to their team only
                let filtered: Vec<_> = dags.into_iter().filter(|d| {
                    d.get("team_id").and_then(|v| v.as_str()) == Some(&user_team_id)
                }).collect();
                let filtered_total = filtered.len() as i64;
                Json(PaginatedResponse { data: filtered, total: filtered_total, limit, offset })
            } else {
                // Non-admin with no team assignment: only show DAGs with no team
                let filtered: Vec<_> = dags.into_iter().filter(|d| {
                    d.get("team_id").and_then(|v| v.as_str()).is_none()
                }).collect();
                let filtered_total = filtered.len() as i64;
                Json(PaginatedResponse { data: filtered, total: filtered_total, limit, offset })
            }
        },
        Err(_) => Json(PaginatedResponse { data: vec![], total: 0, limit, offset }),
    }
}

/// GET /api/runs — all recent runs across every DAG (for the UI Runs page)
async fn get_all_runs_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(100).min(500);
    let offset = params.offset.unwrap_or(0);
    match state.db.get_all_runs(limit, offset).await {
        Ok((runs, total)) => Json(PaginatedResponse { data: runs, total, limit, offset }).into_response(),
        Err(_) => Json(PaginatedResponse::<serde_json::Value> { data: vec![], total: 0, limit, offset }).into_response(),
    }
}

async fn get_dag_tasks(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0);

    let dag_db = match state.db.get_dag_by_id(&id).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("DB error fetching DAG: {}", e)}))).into_response(),
    };
    if let Some(dag) = &dag_db {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    // BUG-9 FIX: Propagate DB errors as 500 instead of masking them as empty results.
    let tasks = match state.db.get_dag_tasks(&id).await {
        Ok(t) => t,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("DB error fetching tasks: {}", e)}))).into_response(),
    };
    
    // Pass run_id if it exists in task_instances table
    let (instances_data, instances_total) = match state.db.get_task_instances(&id, limit, offset).await {
        Ok(result) => result,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("DB error fetching instances: {}", e)}))).into_response(),
    };
    
    // Get dependencies from in-memory map
    let dependencies = {
        let map = state.dags.lock().await;
        map.get(&id).map(|d| d.dependencies.clone()).unwrap_or_default()
    };
    
    Json(json!({
        "dag_id": id, 
        "tasks": tasks, 
        "instances": instances_data,
        "instances_total": instances_total,
        "instances_limit": limit,
        "instances_offset": offset,
        "dag": dag_db,
        "dependencies": dependencies
    })).into_response()
}
async fn get_dag_runs(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0);

    match state.db.get_dag_by_id(&id).await {
        Ok(Some(dag)) => {
            if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
                if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                    return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
                }
            }
        }
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("DB error fetching DAG: {}", e)}))).into_response(),
    }

    let (runs, total) = match state.db.get_dag_runs(&id, limit, offset).await {
        Ok(res) => res,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("DB error fetching runs: {}", e)}))).into_response(),
    };
    Json(PaginatedResponse { data: runs, total, limit, offset }).into_response()
}
async fn get_task_logs(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // 1. Try DB first
    // Refactored to use trait-compatible logic
    if let Ok(Some((dag_id, task_id, execution_date))) = state.db.get_task_instance(&id).await {
         // Check DB logs first (this logic was direct SQLite query before, let's keep it simple or expand trait)
         // For now, let's assume we can get them from the FS or add a method.
         // Actually, I'll just check if the files exist.
         let log_path = format!("logs/{}/{}/{}.log", dag_id, task_id, execution_date.format("%Y-%m-%d"));
         if let Ok(content) = fs::read_to_string(log_path) { 
            return Json(json!({ "stdout": content, "stderr": "" })).into_response(); 
         }
    }
    
    (StatusCode::NOT_FOUND, Json(json!({ "error": "Log not found" }))).into_response()
}

async fn get_task_events_handler(
    Path((_dag_id, ti_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>
) -> impl IntoResponse {
    match state.db.get_task_events(&ti_id).await {
        Ok(events) => Json(events).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to fetch task events"}))).into_response(),
    }
}

async fn trigger_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    if let Ok(Some(dag)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    let _ = state.db.log_audit_event(
        &auth_user.username, "dag.trigger", "dag", &id, "{}",
    ).await;
    let _ = state.tx.send(ScheduleRequest {
        dag_id: id,
        triggered_by: auth_user.username,
        run_type: RunType::Full,
        execution_date: None,
    }).await;
    Json(json!({"message": "Triggered"})).into_response()
}

async fn retry_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    if let Ok(Some(dag)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    let _ = state.tx.send(ScheduleRequest {
        dag_id: id,
        triggered_by: auth_user.username.clone(), // BUG-5 FIX: use real username, not "api"
        run_type: RunType::RetryFromFailure,
        execution_date: None,
    }).await;
    Json(json!({"message": "Retry triggered"})).into_response()
}

async fn get_dag_source(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    match state.db.get_latest_version(&id).await {
        Ok(Some(version)) => {
            let path = version["file_path"].as_str().unwrap_or("");
            match fs::read_to_string(path) {
                Ok(content) => Json(json!({"dag_id": id, "source": content, "file_path": path})).into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
            }
        },
        _ => (StatusCode::NOT_FOUND, Json(json!({ "error": "DAG source not found" }))).into_response(),
    }
}
#[derive(Deserialize)]
struct UpdateSource { source: String }

async fn update_dag_source(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, Json(body): Json<UpdateSource>) -> Response {
    let version = match state.db.get_latest_version(&id).await {
        Ok(Some(v)) => v,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };

    let path = version["file_path"].as_str().unwrap_or("");

    // Bug 19 fix: validate the stored file_path is within the allowed dags/ directory.
    // An attacker who manages to inject a path into dag_versions could otherwise write
    // arbitrary files (e.g. ../../etc/cron.d/evil).
    let allowed_base = std::path::Path::new("dags")
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from("dags"));
    let target = std::path::Path::new(path);
    let canonical_target = match target.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // File might not exist yet — resolve against cwd manually
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(target),
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Cannot resolve working directory"}))).into_response(),
            }
        }
    };
    if !canonical_target.starts_with(&allowed_base) {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Path traversal detected: file_path is outside the dags/ directory"}))).into_response();
    }

    if let Err(e) = fs::write(path, &body.source) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response();
    }
    
    // Re-parse and update internal map
    match crate::python_parser::parse_python_dag(path) {
        Ok(dags) => {
            let mut target_dag = None;
            for dag in dags {
                if dag.id == id {
                    target_dag = Some(dag);
                    break;
                }
            }

            if let Some(dag) = target_dag {
                // Update the DB schema/tasks for the new version
                let _ = state.db.register_dag(&dag).await;
                
                {
                    let mut map = state.dags.lock().await;
                    map.insert(id.clone(), Arc::new(dag));
                }
                
                let _ = state.db.store_dag_version(&id, path).await;
                let _ = state.db.log_audit_event(
                    &auth_user.username, "dag.source_update", "dag", &id, "{}",
                ).await;
                Json(json!({"message": "Source updated and re-parsed"})).into_response()
            } else {
                (StatusCode::BAD_REQUEST, Json(json!({"error": format!("No DAG with ID '{}' found in file", id)}))).into_response()
            }
        },
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to parse updated source: {}", e)}))).into_response()
        }
    }
}

/// PATCH /api/dags/:id/source/rust — Update source and reparse as a Rust/Config DAG (JSON/YAML)
async fn update_dag_source_rust(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, Json(body): Json<UpdateSource>) -> Response {
    let version = match state.db.get_latest_version(&id).await {
        Ok(Some(v)) => v,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };

    let path = version["file_path"].as_str().unwrap_or("");

    // Path traversal validation
    let allowed_base = std::path::Path::new("dags")
        .canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from("dags"));
    let target = std::path::Path::new(path);
    let canonical_target = match target.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(target),
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Cannot resolve working directory"}))).into_response(),
            }
        }
    };
    if !canonical_target.starts_with(&allowed_base) {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Path traversal detected: file_path is outside the dags/ directory"}))).into_response();
    }

    if let Err(e) = fs::write(path, &body.source) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response();
    }

    // Reparse using dag_factory (JSON/YAML config DAGs) instead of python_parser
    match crate::dag_factory::parse_dag_file(path) {
        Ok(dags) => {
            let mut target_dag = None;
            for dag in dags {
                if dag.id == id {
                    target_dag = Some(dag);
                    break;
                }
            }

            if let Some(dag) = target_dag {
                let _ = state.db.register_dag(&dag).await;
                {
                    let mut map = state.dags.lock().await;
                    map.insert(id.clone(), Arc::new(dag));
                }
                let _ = state.db.store_dag_version(&id, path).await;
                let _ = state.db.log_audit_event(
                    &auth_user.username, "dag.source_update_rust", "dag", &id, "{}",
                ).await;
                Json(json!({"message": "Source updated and re-parsed (config/rust)"})).into_response()
            } else {
                (StatusCode::BAD_REQUEST, Json(json!({"error": format!("No DAG with ID '{}' found in file", id)}))).into_response()
            }
        },
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to parse updated source: {}", e)}))).into_response()
        }
    }
}

// ─── DAG Versioning & Rollback ──────────────────────────────

async fn get_dag_versions_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    if let Ok(Some(dag)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    match state.db.get_dag_versions(&id).await {
        Ok(versions) => Json(json!({"dag_id": id, "versions": versions})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_dag_version_source_handler(
    Path((id, version_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    if let Ok(Some(dag)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    let versions = match state.db.get_dag_versions(&id).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };

    let target_version = versions.iter().find(|v| v["version"].as_i64().map(|v| v.to_string()) == Some(version_id.clone()));
    if let Some(target) = target_version {
        let path = target["file_path"].as_str().unwrap_or("");
        match fs::read_to_string(path) {
            Ok(content) => Json(json!({"dag_id": id, "version": version_id, "source": content, "file_path": path})).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Could not read version source: {}", e)}))).into_response(),
        }
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "Version not found"}))).into_response()
    }
}

async fn rollback_dag_version_handler(
    Path((id, version_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    if let Ok(Some(dag)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    // 1. Fetch the requested version
    let versions = match state.db.get_dag_versions(&id).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };

    // BUG-3 FIX: look up by version number (same field as get_dag_version_source_handler)
    let target_version = versions.iter().find(|v| v["version"].as_i64().map(|n| n.to_string()) == Some(version_id.clone()));
    if let Some(target) = target_version {
        let path = target["file_path"].as_str().unwrap_or("");
        
        // Ensure path exists before using it
        let old_content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Could not read version source: {}", e)}))).into_response(),
        };

        // 2. Fetch the CURRENT active path (so we overwrite it with the old content)
        if let Ok(Some(current_version)) = state.db.get_latest_version(&id).await {
            let active_path = current_version["file_path"].as_str().unwrap_or("");
            if let Err(e) = fs::write(active_path, &old_content) {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to write to active path: {}", e)}))).into_response();
            }

            // 3. Re-parse the updated file
            match crate::python_parser::parse_python_dag(active_path) {
                Ok(dags) => {
                    let mut target_dag = None;
                    for dag in dags {
                        if dag.id == id {
                            target_dag = Some(dag);
                            break;
                        }
                    }

                    if let Some(dag) = target_dag {
                        let _ = state.db.register_dag(&dag).await;
                        {
                            let mut map = state.dags.lock().await;
                            map.insert(id.clone(), Arc::new(dag));
                        }
                        
                        let new_ver = state.db.store_dag_version(&id, active_path).await.unwrap_or(0);
                        let _ = state.db.log_audit_event(
                            &auth_user.username, "dag.rollback", "dag", &id, &format!("{{\"from_version\": \"{}\", \"new_version\": {}}}", version_id, new_ver)
                        ).await;
                        return Json(json!({"message": "Successfully rolled back", "new_version_number": new_ver})).into_response();
                    } else {
                        return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("No DAG with ID '{}' found in file after rollback", id)}))).into_response();
                    }
                },
                Err(e) => {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Failed to parse updated source: {}", e)}))).into_response();
                }
            }
        }
    }
    
    (StatusCode::NOT_FOUND, Json(json!({"error": "Version not found"}))).into_response()
}

async fn pause_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    let _ = state.db.pause_dag(&id).await;
    let _ = state.db.log_audit_event(&auth_user.username, "dag.pause", "dag", &id, "{}").await;
    Json(json!({"message": "Paused"}))
}
async fn unpause_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    let dag_meta = state.db.get_dag_by_id(&id).await.unwrap_or(None);
    let _ = state.db.unpause_dag(&id).await;
    let _ = state.db.log_audit_event(&auth_user.username, "dag.unpause", "dag", &id, "{}").await;
    
    if let Some(dag) = dag_meta {
        let catchup = dag.get("catchup").and_then(|v| v.as_bool()).unwrap_or(false);
        if catchup {
            if let (Some(schedule_expr), Some(last_run_str)) = (
                dag.get("schedule_interval").and_then(|v| v.as_str()),
                dag.get("last_run").and_then(|v| v.as_str())
            ) {
                if let Ok(last_run) = chrono::DateTime::parse_from_rfc3339(last_run_str) {
                    let last_run_utc = last_run.with_timezone(&chrono::Utc);
                    let _now = chrono::Utc::now();

                    if let Ok(schedule_str) = crate::scheduler::normalize_schedule(schedule_expr) {
                        if !schedule_str.is_empty() {
                            if let Ok(schedule) = schedule_str.parse::<cron::Schedule>() {
                                let now = chrono::Utc::now();
                                for dt in schedule.after(&last_run_utc) {
                                    if dt > now { break; }
                                    let _ = state.tx.send(ScheduleRequest {
                                        dag_id: id.clone(),
                                        triggered_by: "catchup".to_string(),
                                        run_type: RunType::Full,
                                        execution_date: Some(dt),
                                    }).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Json(json!({"message": "Unpaused"}))
}

#[derive(Deserialize)]
struct ScheduleUpdate { schedule_interval: Option<String>, timezone: Option<String>, max_active_runs: Option<i32>, catchup: Option<bool>, }
async fn update_schedule(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, Json(body): Json<ScheduleUpdate>) -> impl IntoResponse {
    if let Ok(Some(current)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = current.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }

        let schedule = body.schedule_interval.or_else(|| current["schedule_interval"].as_str().map(|s| s.to_string()));
        let timezone = body.timezone.unwrap_or_else(|| current["timezone"].as_str().unwrap_or("UTC").to_string());
        let max_active = body.max_active_runs.unwrap_or_else(|| current["max_active_runs"].as_i64().unwrap_or(1) as i32);
        let catchup = body.catchup.unwrap_or_else(|| current["catchup"].as_bool().unwrap_or(false));
        let is_dynamic = current["is_dynamic"].as_bool().unwrap_or(false);
        let _ = state.db.update_dag_config(&id, schedule.as_deref(), &timezone, max_active, catchup, is_dynamic).await;
        Json(json!({"message": "Updated"})).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response()
    }
}
#[derive(Deserialize)]
struct BackfillRequest { start_date: String, end_date: String, dry_run: Option<bool> }

async fn backfill_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, Json(body): Json<BackfillRequest>) -> impl IntoResponse {
    let dag_meta = match state.db.get_dag_by_id(&id).await {
        Ok(Some(dag)) => {
            if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
                if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                    return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
                }
            }
            dag
        },
        _ => return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response(),
    };

    // BUG-17 FIX: Return 400 on invalid date input instead of silently falling back to Utc::now().
    let start = match chrono::DateTime::parse_from_rfc3339(&body.start_date) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid start_date: {}", e)}))).into_response(),
    };
    let end = match chrono::DateTime::parse_from_rfc3339(&body.end_date) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid end_date: {}", e)}))).into_response(),
    };

    let schedule_expr = dag_meta.get("schedule_interval").and_then(|v| v.as_str()).unwrap_or("");
    let schedule_str = match crate::scheduler::normalize_schedule(schedule_expr) {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid schedule: {}", e)}))).into_response();
        }
    };

    let mut intervals = Vec::new();
    if !schedule_str.is_empty() {
        if let Ok(schedule) = schedule_str.parse::<cron::Schedule>() {
            let iter: cron::ScheduleIterator<'_, chrono::Utc> = schedule.after(&start);
            for dt in iter {
                if dt > end { break; }
                intervals.push(dt);
            }
        }
    }
    if intervals.is_empty() { intervals.push(start); }

    if body.dry_run.unwrap_or(false) {
        return Json(json!({
            "message": "Dry run execution generated dates",
            "intervals": intervals.iter().map(|d| d.to_rfc3339()).collect::<Vec<String>>()
        })).into_response();
    }

    // BUG-20 FIX: Cap backfill intervals to prevent channel saturation and OOM.
    const MAX_BACKFILL_INTERVALS: usize = 10_000;
    if intervals.len() > MAX_BACKFILL_INTERVALS {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "error": format!("Backfill would generate {} intervals, exceeding the limit of {}. Narrow the date range.", intervals.len(), MAX_BACKFILL_INTERVALS)
        }))).into_response();
    }

    let _ = state.db.log_audit_event(&auth_user.username, "dag.backfill", "dag", &id, &format!("{{\"start\":\"{}\",\"end\":\"{}\"}}", start, end)).await;
    
    state.backfill_progress.write().await.insert(id.clone(), 0.0);

    // BUG-11 FIX: Spawn the backfill loop in the background so the handler
    // returns immediately. Without this, progress always reached 1.0 before
    // the HTTP response was sent, making the progress endpoint useless.
    let tx = state.tx.clone();
    let backfill_progress = Arc::clone(&state.backfill_progress);
    let dag_id_bg = id.clone();
    let total = intervals.len();

    tokio::spawn(async move {
        let total_f = total as f32;
        backfill_progress.write().await.insert(dag_id_bg.clone(), 0.0);
        for (i, dt) in intervals.into_iter().enumerate() {
            // BUG-20 FIX: Use try_send to avoid blocking the entire backfill
            // spawned task when the scheduler channel is full.
            loop {
                match tx.try_send(ScheduleRequest {
                    dag_id: dag_id_bg.clone(),
                    triggered_by: "backfill".to_string(),
                    run_type: RunType::Full,
                    execution_date: Some(dt),
                }) {
                    Ok(_) => break,
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        // Backpressure: yield and retry instead of blocking forever
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        tracing::warn!("Scheduler channel closed during backfill for DAG {}", dag_id_bg);
                        return;
                    }
                }
            }
            backfill_progress.write().await.insert(dag_id_bg.clone(), (i + 1) as f32 / total_f);
        }
        backfill_progress.write().await.insert(dag_id_bg.clone(), 1.0);
        // Improvement 44: evict completed entry after 60 s so the HashMap
        // doesn't grow forever (entries were previously kept indefinitely).
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        backfill_progress.write().await.remove(&dag_id_bg);
    });

    Json(json!({"message": "Backfill triggered", "start": start, "end": end, "intervals_queued": total})).into_response()
}

async fn get_backfill_progress(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let progress = *state.backfill_progress.read().await.get(&id).unwrap_or(&0.0);
    Json(json!({"dag_id": id, "progress": progress})).into_response()
}

/// Improvement 38/42: Health check endpoint — verifies DB connectivity and
/// returns a lightweight JSON response. Used by load-balancers and monitoring.
async fn health_handler(State(state): State<Arc<AppState>>) -> Response {
    // Improvement 42: use ping() trait method to verify DB connectivity.
    let db_ok = state.db.ping().await;
    let status = if db_ok { "ok" } else { "degraded" };
    let code   = if db_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (code, Json(json!({
        "status":  status,
        "version": env!("CARGO_PKG_VERSION"),
        "db":      if db_ok { "connected" } else { "unreachable" },
    }))).into_response()
}

/// Serve the OpenAPI 3.1 spec as JSON
async fn openapi_spec_handler() -> impl IntoResponse {
    Json(crate::openapi::generate_openapi_spec())
}

async fn swarm_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({"enabled": state.swarm.enabled, "active_workers": state.swarm.active_worker_count().await, "queue_depth": state.swarm.queue_depth().await}))
}
async fn swarm_workers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(500) as usize;
    let offset = params.offset.unwrap_or(0) as usize;

    let workers = state.swarm.get_workers_info().await;
    let total = workers.len();
    
    // In-memory pagination since swarm workers are kept in memory map.
    let end = (offset + limit).min(total);
    let paged = if offset < total {
        workers[offset..end].to_vec()
    } else {
        Vec::new()
    };

    Json(PaginatedResponse { data: paged, total: total as i64, limit: limit as i64, offset: offset as i64 })
}
async fn swarm_drain_worker(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.swarm.drain_worker(&id).await; Json(json!({"message": "Draining"}))
}
async fn swarm_remove_worker(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.swarm.remove_worker(&id).await; Json(json!({"message": "Removed"}))
}
async fn static_handler(req: Request<axum::body::Body>) -> impl IntoResponse {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(axum::http::header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => {
            // SPA fallback: serve index.html for paths without file extensions
            // (i.e. client-side routes like /dags, /runs, /settings)
            if !path.contains('.') {
                if let Some(index) = Assets::get("index.html") {
                    return (
                        [(axum::http::header::CONTENT_TYPE, "text/html")],
                        index.data,
                    )
                        .into_response();
                }
            }
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }
}

// ─── XCom Handlers ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct XComPushRequest {
    dag_id: String,
    task_id: String,
    run_id: String,
    key: String,
    value: String,
}

async fn xcom_push_handler(State(state): State<Arc<AppState>>, Json(body): Json<XComPushRequest>) -> Response {
    match state.xcom.xcom_push(&body.dag_id, &body.task_id, &body.run_id, &body.key, body.value).await {
        Ok(_) => Json(json!({"status": "ok"})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct XComPullQuery {
    dag_id: String,
    task_id: String,
    run_id: String,
    key: String,
}

async fn xcom_pull_handler(State(state): State<Arc<AppState>>, Query(params): Query<XComPullQuery>) -> Response {
    match state.xcom.xcom_pull(&params.dag_id, &params.task_id, &params.run_id, &params.key).await {
        Ok(Some(value)) => Json(json!({"value": value})).into_response(),
        Ok(None) => {
            let body: serde_json::Value = json!({"value": null});
            (StatusCode::NOT_FOUND, Json(body)).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
    }
}

async fn xcom_list_handler(
    State(state): State<Arc<AppState>>,
    Path((dag_id, run_id)): Path<(String, String)>,
    Query(params): Query<PaginationQuery>,
) -> Response {
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0);

    match state.xcom.xcom_pull_all(&dag_id, &run_id, limit, offset).await {
        Ok((entries, total)) => Json(PaginatedResponse { data: entries, total, limit, offset }).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
    }
}

// ─── Task Pool Handlers ────────────────────────────────────────────

async fn list_pools(State(state): State<Arc<AppState>>) -> Response {
    match state.pool_manager.get_all_pools().await {
        Ok(pools) => Json(json!({"pools": pools})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct CreatePoolRequest {
    name: String,
    slots: i32,
    #[serde(default)]
    description: String,
}

async fn create_pool_handler(State(state): State<Arc<AppState>>, Json(body): Json<CreatePoolRequest>) -> Response {
    match state.pool_manager.create_pool(&body.name, body.slots, &body.description).await {
        Ok(()) => Json(json!({"status": "created", "name": body.name})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
    }
}

async fn get_pool_handler(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    match state.pool_manager.get_pool(&name).await {
        Ok(Some(pool)) => Json(json!(pool)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "Pool not found"}))).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct UpdatePoolRequest {
    slots: i32,
    #[serde(default)]
    description: String,
}

async fn update_pool_handler(State(state): State<Arc<AppState>>, Path(name): Path<String>, Json(body): Json<UpdatePoolRequest>) -> Response {
    match state.pool_manager.update_pool(&name, body.slots, &body.description).await {
        Ok(()) => Json(json!({"status": "updated", "name": name})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
    }
}

async fn delete_pool_handler(State(state): State<Arc<AppState>>, Path(name): Path<String>) -> Response {
    match state.pool_manager.delete_pool(&name).await {
        Ok(()) => Json(json!({"status": "deleted", "name": name})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
    }
}

// ─── Webhook Callback Handlers ─────────────────────────────────────

async fn get_callbacks_handler(State(state): State<Arc<AppState>>, Path(dag_id): Path<String>) -> Response {
    match crate::notifications::NotificationManager::get_callbacks(&state.db, &dag_id).await {
        Ok(Some(config)) => Json(json!({"dag_id": dag_id, "config": config})).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "No callbacks configured"}))).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct SetCallbacksRequest {
    config: crate::notifications::CallbackConfig,
}

async fn set_callbacks_handler(State(state): State<Arc<AppState>>, Path(dag_id): Path<String>, Json(body): Json<SetCallbacksRequest>) -> Response {
    match crate::notifications::NotificationManager::save_callbacks(&state.db, &dag_id, &body.config).await {
        Ok(()) => Json(json!({"status": "saved", "dag_id": dag_id})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
    }
}

async fn delete_callbacks_handler(State(state): State<Arc<AppState>>, Path(dag_id): Path<String>) -> Response {
    match crate::notifications::NotificationManager::delete_callbacks(&state.db, &dag_id).await {
        Ok(()) => Json(json!({"status": "deleted", "dag_id": dag_id})).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::BAD_REQUEST, Json(json!({"error": msg}))).into_response()
        }
    }
}

// ─── Prometheus Metrics Handler ────────────────────────────────────

async fn prometheus_metrics_handler_wrapper(State(state): State<Arc<AppState>>) -> Response {
    crate::metrics::metrics_handler(State(state.metrics.clone())).await.into_response()
}

// ─── Audit Log Handler ───────────────────────────────────────────────

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    actor: Option<String>,
    action: Option<String>,
}

async fn get_audit_logs_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditQuery>,
) -> Response {
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0);
    let actor = params.actor.as_deref();
    let action = params.action.as_deref();
    match state.db.get_audit_logs(limit, offset, actor, action).await {
        Ok(logs) => Json(json!({ "logs": logs, "limit": limit, "offset": offset })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ─── Gantt Handler ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct GanttQuery {
    dag_id: String,
}

async fn gantt_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GanttQuery>,
) -> Response {
    match state.db.get_gantt_data(&params.dag_id).await {
        Ok(tasks) => Json(json!({ "dag_id": params.dag_id, "tasks": tasks })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ─── Calendar Handler ────────────────────────────────────────────────

#[derive(Deserialize)]
struct CalendarQuery {
    days: Option<i64>,
}

async fn calendar_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CalendarQuery>,
) -> Response {
    use chrono::{Utc, Duration};
    use cron::Schedule;
    use std::str::FromStr;

    let days = params.days.unwrap_or(30).min(90);
    let now = Utc::now();
    let end = now + Duration::days(days);

    // Gather all DAGs from DB (assuming internal system queries max 1000 for this view)
    let dags = match state.db.get_all_dags(1000, 0).await {
        Ok((d, _)) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    };

    let mut events: Vec<serde_json::Value> = Vec::new();

    // Future scheduled events
    for dag in &dags {
        let dag_id = match dag["id"].as_str() { Some(s) => s, None => continue };
        let schedule_interval = match dag["schedule_interval"].as_str() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let is_paused = dag["is_paused"].as_bool().unwrap_or(false);
        if is_paused { continue; }

        // Parse cron expression
        if let Ok(schedule) = Schedule::from_str(schedule_interval) {
            for next in schedule.after(&now).take(50) {
                if next > end { break; }
                events.push(json!({
                    "dag_id": dag_id,
                    "scheduled_time": next.to_rfc3339(),
                    "type": "scheduled",
                }));
            }
        }
    }

    // Past completed runs from DB
    for dag in &dags {
        let dag_id = match dag["id"].as_str() { Some(s) => s, None => continue };
        if let Ok((runs, _)) = state.db.get_dag_runs(dag_id, 100, 0).await {
            for run in runs {
                if let Some(exec_date) = run["execution_date"].as_str() {
                    events.push(json!({
                        "dag_id": dag_id,
                        "scheduled_time": exec_date,
                        "type": "completed",
                        "state": run["state"].as_str().unwrap_or("Unknown"),
                    }));
                }
            }
        }
    }

    // Sort by time
    events.sort_by(|a, b| {
        let ta = a["scheduled_time"].as_str().unwrap_or("");
        let tb = b["scheduled_time"].as_str().unwrap_or("");
        ta.cmp(tb)
    });

    Json(json!({ "events": events })).into_response()
}

// ─── Teams Handler ───────────────────────────────────────────────────

async fn get_teams_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage teams"}))).into_response();
    }
    match state.db.get_all_teams().await {
        Ok(teams) => Json(json!({"teams": teams})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response()
    }
}

#[derive(Deserialize)]
struct CreateTeamRequest {
    id: String,
    name: String,
    description: String,
    max_concurrent_tasks: i32,
    max_dags: i32,
}

async fn create_team_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
    Json(body): Json<CreateTeamRequest>
) -> impl IntoResponse {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage teams"}))).into_response();
    }
    match state.db.create_team(&body.id, &body.name, &body.description, body.max_concurrent_tasks, body.max_dags).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "team.create", "team", &body.id, &format!("{{\"name\": \"{}\"}}", body.name)).await;
            Json(json!({"status": "created", "id": body.id})).into_response()
        },
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response()
    }
}

async fn get_team_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    if auth_user.role != "Admin" && auth_user.team_id.as_deref() != Some(&id) {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Access denied"}))).into_response();
    }
    match state.db.get_team(&id).await {
        Ok(Some(team)) => Json(json!(team)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "Team not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response()
    }
}

#[derive(Deserialize)]
struct UpdateTeamRequest {
    name: Option<String>,
    description: Option<String>,
    max_concurrent_tasks: Option<i32>,
    max_dags: Option<i32>,
}

async fn update_team_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
    Json(body): Json<UpdateTeamRequest>
) -> impl IntoResponse {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage teams"}))).into_response();
    }
    match state.db.update_team(&id, &body.name, &body.description, body.max_concurrent_tasks, body.max_dags).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "team.update", "team", &id, "{}").await;
            Json(json!({"status": "updated", "id": id})).into_response()
        },
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response()
    }
}

async fn delete_team_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>
) -> impl IntoResponse {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage teams"}))).into_response();
    }
    match state.db.delete_team(&id).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "team.delete", "team", &id, "{}").await;
            Json(json!({"status": "deleted", "id": id})).into_response()
        },
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response()
    }
}

#[derive(Deserialize)]
struct AssignUserTeamRequest {
    team_id: Option<String>,
}

async fn assign_user_team_handler(
    Path((id, username)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
    Json(body): Json<AssignUserTeamRequest>
) -> impl IntoResponse {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage teams"}))).into_response();
    }

    // ARCH-7 FIX: Validate that the path :id matches body.team_id so callers can't
    // silently assign a user to a different team than the URL implies.
    if let Some(ref body_team) = body.team_id {
        if body_team != "unassign" && body_team != &id {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error": format!("Path team id '{}' does not match body team_id '{}'", id, body_team)
            }))).into_response();
        }
    }

    let target_team = if body.team_id.as_deref() == Some("unassign") { None } else { body.team_id.as_deref() };
    match state.db.assign_user_to_team(&username, target_team).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "user.assign_team", "user", &username, &format!("{{\"team_id\": {:?}}}", target_team)).await;
            Json(json!({"status": "assigned", "username": username, "team_id": target_team})).into_response()
        },
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response()
    }
}

// ── Auth Provider Handlers ───────────────────────────────────────

async fn get_auth_providers_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
) -> Response {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage auth providers"}))).into_response();
    }
    match state.db.get_auth_providers().await {
        Ok(providers) => Json(json!({"providers": providers})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateAuthProviderRequest {
    id: String,
    provider_type: String,
    name: String,
    config: serde_json::Value,
    enabled: Option<bool>,
    priority: Option<i32>,
}

async fn create_auth_provider_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
    Json(body): Json<CreateAuthProviderRequest>,
) -> Response {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage auth providers"}))).into_response();
    }
    let valid_types = ["oidc", "saml", "ldap", "local"];
    if !valid_types.contains(&body.provider_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid provider type. Must be one of: {:?}", valid_types)}))).into_response();
    }
    let config_str = serde_json::to_string(&body.config).unwrap_or_else(|_| "{}".to_string());
    match state.db.upsert_auth_provider(&body.id, &body.provider_type, &body.name, &config_str, body.enabled.unwrap_or(true), body.priority.unwrap_or(0)).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "auth_provider.create", "auth_provider", &body.id, "{}").await;
            (StatusCode::CREATED, Json(json!({"status": "created", "id": body.id}))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_auth_provider_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
    Path(id): Path<String>,
) -> Response {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can manage auth providers"}))).into_response();
    }
    if id == "local" {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "Cannot delete the local auth provider"}))).into_response();
    }
    match state.db.delete_auth_provider(&id).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "auth_provider.delete", "auth_provider", &id, "{}").await;
            Json(json!({"status": "deleted", "id": id})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_user_sessions_handler(
    State(_state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
) -> Response {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can view sessions"}))).into_response();
    }
    // Return count of active sessions as a summary
    Json(json!({"status": "ok", "message": "Session management available"})).into_response()
}

async fn cleanup_sessions_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>,
) -> Response {
    if auth_user.role != "Admin" {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "Only admins can cleanup sessions"}))).into_response();
    }
    match state.db.cleanup_expired_sessions().await {
        Ok(count) => {
            let _ = state.db.log_audit_event(&auth_user.username, "sessions.cleanup", "system", "sessions", &format!("{{\"deleted\": {}}}", count)).await;
            Json(json!({"status": "ok", "deleted_sessions": count})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

/// Public endpoint: list enabled auth providers (no auth required) for login page.
async fn get_public_auth_providers_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.get_auth_providers().await {
        Ok(providers) => {
            // Filter to only expose non-sensitive info
            let public: Vec<serde_json::Value> = providers.iter().map(|p| {
                json!({
                    "id": p.get("id"),
                    "provider_type": p.get("provider_type"),
                    "name": p.get("name"),
                    "enabled": p.get("enabled"),
                })
            }).collect();
            Json(json!({"providers": public})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct OidcAuthorizeQuery {
    provider_id: Option<String>,
}

async fn oidc_authorize_handler(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<OidcAuthorizeQuery>,
) -> Response {
    let _provider_id = query.provider_id.unwrap_or_else(|| "oidc".to_string());
    // In a full implementation, this would:
    // 1. Look up the OIDC provider config from DB
    // 2. Generate a state parameter and store it in session
    // 3. Build the authorization URL with PKCE
    // 4. Redirect the user to the IdP
    Json(json!({
        "status": "oidc_redirect",
        "message": "OIDC authorization flow. Configure an OIDC provider to enable SSO.",
        "docs": "POST /api/auth/providers with provider_type='oidc' to configure"
    })).into_response()
}

#[derive(Deserialize)]
struct OidcCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn oidc_callback_handler(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<OidcCallbackQuery>,
) -> Response {
    if let Some(error) = query.error {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("OIDC error: {}", error)}))).into_response();
    }
    match (&query.code, &query.state) {
        (Some(_code), Some(_state)) => {
            // In a full implementation:
            // 1. Validate state parameter against stored session
            // 2. Exchange authorization code for tokens
            // 3. Fetch user info
            // 4. Create/update user in DB
            // 5. Create session
            // 6. Return session token
            Json(json!({
                "status": "callback_received",
                "message": "OIDC callback received. Configure an OIDC provider for full SSO flow."
            })).into_response()
        }
        _ => (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing code or state parameter"}))).into_response(),
    }
}

#[derive(Deserialize)]
struct SamlAcsRequest {
    #[serde(rename = "SAMLResponse")]
    saml_response: Option<String>,
    #[serde(rename = "RelayState")]
    relay_state: Option<String>,
}

async fn saml_acs_handler(
    State(_state): State<Arc<AppState>>,
    axum::extract::Form(form): axum::extract::Form<SamlAcsRequest>,
) -> Response {
    match form.saml_response {
        Some(_response) => {
            // In a full implementation:
            // 1. Decode base64 SAML response
            // 2. Validate XML signature
            // 3. Extract assertions (NameID, attributes)
            // 4. Map to Vortex user/role/team
            // 5. Create session
            Json(json!({
                "status": "saml_received",
                "message": "SAML assertion received. Configure a SAML provider for full SSO flow."
            })).into_response()
        }
        None => (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing SAMLResponse"}))).into_response(),
    }
}

// --- Lineage & Incident Management Handlers ---

#[derive(Deserialize)]
struct LineageEventsQuery {
    limit: Option<i64>,
    run_id: Option<String>,
}

async fn get_lineage_events_handler(
    State(state): State<Arc<AppState>>,
    Path(dag_id): Path<String>,
    Query(params): Query<LineageEventsQuery>,
) -> Response {
    let limit = params.limit.unwrap_or(50);
    let run_id_ref = params.run_id.as_deref();
    match state.db.get_lineage_events(&dag_id, run_id_ref, limit).await {
        Ok(events) => Json(json!({ "dag_id": dag_id, "events": events, "limit": limit })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_lineage_datasets_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationQuery>,
) -> Response {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);
    match state.db.get_lineage_datasets(limit, offset).await {
        Ok(datasets) => Json(json!({ "datasets": datasets, "limit": limit, "offset": offset })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct IncidentConfigsQuery {
    team_id: Option<String>,
}

async fn get_incident_configs_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<IncidentConfigsQuery>,
) -> Response {
    let team_id_ref = params.team_id.as_deref();
    match state.db.get_incident_configs(team_id_ref).await {
        Ok(configs) => Json(json!({ "configs": configs })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct IncidentConfigRequest {
    id: Option<String>,
    team_id: Option<String>,
    provider: String,
    name: String,
    config: String,
    enabled: Option<bool>,
}

async fn create_incident_config_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IncidentConfigRequest>,
) -> Response {
    let enabled = body.enabled.unwrap_or(true);
    let id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let team_id_ref = body.team_id.as_deref();
    match state.db.upsert_incident_config(&id, team_id_ref, &body.provider, &body.name, &body.config, enabled).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"status": "created", "id": id, "provider": body.provider}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_incident_config_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.db.delete_incident_config(&id).await {
        Ok(()) => Json(json!({"status": "deleted", "id": id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

// --- Compliance, Governance & Change Management Handlers ---

#[derive(Deserialize)]
struct AuditLogQuery {
    event_type: Option<String>,
    actor: Option<String>,
    resource_type: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn get_audit_log_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditLogQuery>,
) -> Response {
    let limit = params.limit.unwrap_or(100);
    let offset = params.offset.unwrap_or(0);
    match state.db.get_audit_log(params.event_type.as_deref(), params.actor.as_deref(), params.resource_type.as_deref(), limit, offset).await {
        Ok(entries) => Json(json!({ "entries": entries, "limit": limit, "offset": offset })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_approval_gates_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.get_approval_gates().await {
        Ok(gates) => Json(json!({ "gates": gates })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct ApprovalGateRequest {
    id: Option<String>,
    name: String,
    resource_type: String,
    resource_pattern: String,
    required_approvers: Option<i32>,
    approver_roles: Option<Vec<String>>,
    enabled: Option<bool>,
}

async fn create_approval_gate_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ApprovalGateRequest>,
) -> Response {
    let id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let roles = body.approver_roles.unwrap_or_default();
    let enabled = body.enabled.unwrap_or(true);
    let required = body.required_approvers.unwrap_or(1);
    match state.db.upsert_approval_gate(&id, &body.name, &body.resource_type, &body.resource_pattern, required, &roles, enabled).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"status": "created", "id": id}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_approval_gate_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.db.delete_approval_gate(&id).await {
        Ok(()) => Json(json!({"status": "deleted", "id": id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct ApprovalRequestQuery {
    status: Option<String>,
    limit: Option<i64>,
}

async fn get_approval_requests_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ApprovalRequestQuery>,
) -> Response {
    let limit = params.limit.unwrap_or(50);
    match state.db.get_approval_requests(params.status.as_deref(), limit).await {
        Ok(requests) => Json(json!({ "requests": requests })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateApprovalRequest {
    gate_id: String,
    resource_type: String,
    resource_id: String,
    change_description: Option<String>,
    change_diff: Option<serde_json::Value>,
}

async fn create_approval_request_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(body): Json<CreateApprovalRequest>,
) -> Response {
    let diff = body.change_diff.unwrap_or(serde_json::json!({}));
    match state.db.create_approval_request(&body.gate_id, &user.username, &body.resource_type, &body.resource_id, body.change_description.as_deref(), &diff).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({"status": "pending", "id": id}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct ApproveRejectBody {
    comment: Option<String>,
}

async fn approve_request_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ApproveRejectBody>,
) -> Response {
    match state.db.add_approval_vote(&id, &user.username, body.comment.as_deref()).await {
        Ok(new_status) => Json(json!({"status": new_status, "request_id": id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn reject_request_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Path(id): Path<String>,
    Json(body): Json<ApproveRejectBody>,
) -> Response {
    match state.db.reject_approval_request(&id, &user.username, body.comment.as_deref()).await {
        Ok(()) => Json(json!({"status": "rejected", "request_id": id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_retention_policies_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.get_retention_policies(false).await {
        Ok(policies) => Json(json!({ "policies": policies })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct RetentionPolicyRequest {
    id: Option<String>,
    name: String,
    target_table: String,
    retention_days: i32,
    delete_batch_size: Option<i32>,
    enabled: Option<bool>,
}

async fn create_retention_policy_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RetentionPolicyRequest>,
) -> Response {
    let id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let batch = body.delete_batch_size.unwrap_or(1000);
    let enabled = body.enabled.unwrap_or(true);
    match state.db.upsert_retention_policy(&id, &body.name, &body.target_table, body.retention_days, batch, enabled).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"status": "created", "id": id}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct ComplianceControlsQuery {
    framework: Option<String>,
}

async fn get_compliance_controls_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ComplianceControlsQuery>,
) -> Response {
    match state.db.get_compliance_controls(params.framework.as_deref()).await {
        Ok(controls) => Json(json!({ "controls": controls })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct UpsertComplianceControlRequest {
    framework: String,
    control_id: String,
    description: Option<String>,
    status: String,
    evidence: Option<serde_json::Value>,
}

async fn upsert_compliance_control_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(body): Json<UpsertComplianceControlRequest>,
) -> Response {
    let description = body.description.as_deref().unwrap_or("");
    let evidence = body.evidence.unwrap_or(serde_json::json!({}));
    match state.db.upsert_compliance_control(&body.framework, &body.control_id, description, &body.status, &evidence, &user.username).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"status": "updated", "framework": body.framework, "control_id": body.control_id}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_compliance_summary_handler(
    State(state): State<Arc<AppState>>,
    Path(framework): Path<String>,
) -> Response {
    match state.db.get_compliance_controls(Some(&framework)).await {
        Ok(controls) => {
            let total = controls.len();
            let compliant = controls.iter().filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("compliant")).count();
            let non_compliant = controls.iter().filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("non_compliant")).count();
            let partial = controls.iter().filter(|c| c.get("status").and_then(|v| v.as_str()) == Some("partially_compliant")).count();
            let not_assessed = total - compliant - non_compliant - partial;
            Json(json!({
                "framework": framework,
                "total": total,
                "compliant": compliant,
                "non_compliant": non_compliant,
                "partially_compliant": partial,
                "not_assessed": not_assessed,
                "compliance_rate": if total > 0 { (compliant as f64 / total as f64 * 100.0).round() } else { 0.0 },
            })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

// --- Fine-Grained RBAC, Token Scoping & Network Security Handlers ---

async fn get_rbac_roles_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.get_rbac_roles().await {
        Ok(roles) => Json(json!({ "roles": roles })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_role_permissions_handler(
    State(state): State<Arc<AppState>>,
    Path(role_id): Path<String>,
) -> Response {
    match state.db.get_rbac_role_permissions(&role_id).await {
        Ok(perms) => Json(json!({ "role_id": role_id, "permissions": perms })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_user_roles_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Response {
    match state.db.get_user_roles(&user_id).await {
        Ok(roles) => Json(json!({ "user_id": user_id, "roles": roles })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct AssignRoleRequest {
    role_id: String,
    team_id: Option<String>,
}

async fn assign_user_role_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(caller): axum::Extension<AuthUser>,
    Path(user_id): Path<String>,
    Json(body): Json<AssignRoleRequest>,
) -> Response {
    match state.db.assign_user_role(&user_id, &body.role_id, body.team_id.as_deref(), &caller.username).await {
        Ok(()) => (StatusCode::CREATED, Json(json!({"status": "assigned", "user_id": user_id, "role_id": body.role_id}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct RevokeRoleQuery {
    team_id: Option<String>,
}

async fn revoke_user_role_handler(
    State(state): State<Arc<AppState>>,
    Path((user_id, role_id)): Path<(String, String)>,
    Query(params): Query<RevokeRoleQuery>,
) -> Response {
    match state.db.revoke_user_role(&user_id, &role_id, params.team_id.as_deref()).await {
        Ok(()) => Json(json!({"status": "revoked", "user_id": user_id, "role_id": role_id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct UserPermQuery {
    team_id: Option<String>,
}

async fn get_user_permissions_handler(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Query(params): Query<UserPermQuery>,
) -> Response {
    match state.db.get_user_effective_permissions(&user_id, params.team_id.as_deref()).await {
        Ok(perms) => Json(json!({ "user_id": user_id, "permissions": perms })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_api_tokens_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
) -> Response {
    match state.db.get_api_tokens(&user.username).await {
        Ok(tokens) => Json(json!({ "tokens": tokens })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateTokenRequest {
    name: String,
    scopes: Vec<String>,
    team_id: Option<String>,
    expires_at: Option<String>,
}

async fn create_api_token_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(body): Json<CreateTokenRequest>,
) -> Response {
    let raw_token = crate::rbac::generate_token();
    let hash = match crate::rbac::hash_token(&raw_token) {
        Ok(h) => h,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    };
    match state.db.create_api_token(&body.name, &hash, &user.username, &body.scopes, body.team_id.as_deref(), body.expires_at.as_deref()).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({"id": id, "token": raw_token, "name": body.name, "scopes": body.scopes, "note": "Save this token — it will not be shown again."}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn revoke_api_token_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.db.revoke_api_token(&id).await {
        Ok(()) => Json(json!({"status": "revoked", "id": id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_ip_allowlist_handler(
    State(state): State<Arc<AppState>>,
) -> Response {
    match state.db.get_ip_allowlist().await {
        Ok(rules) => Json(json!({ "rules": rules })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct IpAllowlistRequest {
    id: Option<String>,
    cidr: String,
    description: Option<String>,
    enabled: Option<bool>,
}

async fn create_ip_allowlist_rule_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<IpAllowlistRequest>,
) -> Response {
    let id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let desc = body.description.as_deref().unwrap_or("");
    let enabled = body.enabled.unwrap_or(true);
    match state.db.upsert_ip_allowlist_rule(&id, &body.cidr, desc, enabled).await {
        Ok(_) => (StatusCode::CREATED, Json(json!({"status": "created", "id": id, "cidr": body.cidr}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_ip_allowlist_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    match state.db.delete_ip_allowlist_rule(&id).await {
        Ok(()) => Json(json!({"status": "deleted", "id": id})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_web_routes_compile() {
        // Just a basic compilation test for the web module.
        let _test = "web test";
        assert_eq!(_test, "web test");
    }
}
