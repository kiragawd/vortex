use tracing::{info, debug};
use axum::{
    extract::{Path, State, Multipart, Query},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, patch, delete, put},
    Json, Router,
};
use rust_embed::RustEmbed;
use std::sync::Arc;
use crate::swarm::SwarmState;
use crate::vault::Vault;

use serde_json::json;
use serde::Deserialize;
use std::fs;
use std::collections::HashMap;
use tokio::sync::mpsc;
use std::sync::Mutex;

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
    pub dags: Arc<Mutex<HashMap<String, Arc<crate::scheduler::Dag>>>>,
    pub xcom: Arc<XComStore>,
    pub pool_manager: Arc<PoolManager>,
    pub metrics: Arc<VortexMetrics>,
    pub backfill_progress: Arc<std::sync::RwLock<HashMap<String, f32>>>,
}

pub struct WebServer {
    db: Arc<dyn DatabaseBackend>,
    tx: mpsc::Sender<ScheduleRequest>,
    swarm: Arc<SwarmState>,
    vault: Option<Arc<Vault>>,
    dags: Arc<Mutex<HashMap<String, Arc<crate::scheduler::Dag>>>>,
    metrics: Arc<VortexMetrics>,
}

impl WebServer {
    pub fn new(db: Arc<dyn DatabaseBackend>, tx: mpsc::Sender<ScheduleRequest>, swarm: Arc<SwarmState>, vault: Option<Arc<Vault>>, dags: Arc<Mutex<HashMap<String, Arc<crate::scheduler::Dag>>>>, metrics: Arc<VortexMetrics>) -> Self {
        Self { db, tx, swarm, vault, dags, metrics }
    }

    pub async fn run(self, port: u16, tls_cert: Option<String>, tls_key: Option<String>) {
        let state = Arc::new(AppState {
            db: self.db.clone(),
            tx: self.tx,
            swarm: self.swarm,
            vault: self.vault,
            xcom: Arc::new(XComStore::new(self.db.clone())),
            pool_manager: Arc::new(PoolManager::new(self.db.clone())),
            dags: self.dags,
            metrics: self.metrics,
            backfill_progress: Arc::new(std::sync::RwLock::new(HashMap::new())),
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
            .route("/api/dags/:id/versions", get(get_dag_versions_handler))
            .route("/api/dags/:id/versions/:version/source", get(get_dag_version_source_handler))
            .route("/api/dags/:id/versions/:version/rollback", post(rollback_dag_version_handler))
            .route("/api/dags/:id/retry", post(retry_dag))
            .route("/api/tasks/:id/logs", get(get_task_logs))
            // Swarm
            .route("/api/swarm/status", get(swarm_status))
            .route("/api/swarm/workers", get(swarm_workers))
            .route("/api/swarm/workers/:id/drain", post(swarm_drain_worker))
            .route("/api/swarm/workers/:id", delete(swarm_remove_worker))
            // Pillar 3: Secrets
            .route("/api/secrets", get(get_secrets))
            .route("/api/secrets", post(store_secret))
            .route("/api/secrets/:key", delete(delete_secret))
            // RBAC: Users
            .route("/api/users", get(get_users))
            .route("/api/users", post(create_user))
            .route("/api/users/:username", delete(delete_user))
            // Phase 2: XCom
            .route("/api/xcom/push", post(xcom_push_handler))
            .route("/api/xcom/pull", get(xcom_pull_handler))
            .route("/api/dags/:id/runs/:run_id/xcom", get(xcom_list_handler))
            // Phase 2: Task Pools
            .route("/api/pools", get(list_pools).post(create_pool_handler))
            .route("/api/pools/:name", get(get_pool_handler).put(update_pool_handler).delete(delete_pool_handler))
            // Phase 2: Webhook Callbacks
            .route("/api/dags/:id/callbacks", get(get_callbacks_handler).put(set_callbacks_handler).delete(delete_callbacks_handler))
            // Wave 2: Audit
            .route("/api/audit", get(get_audit_logs_handler))
            // Wave 2: Analysis
            .route("/api/analysis/gantt", get(gantt_handler))
            .route("/api/analysis/calendar", get(calendar_handler))
            // Wave 2: Teams API
            // Wave 2: Teams API
            .route("/api/teams", get(get_teams_handler).post(create_team_handler))
            .route("/api/teams/:id", get(get_team_handler).put(update_team_handler).delete(delete_team_handler))
            .route("/api/teams/:id/users/:username", put(assign_user_team_handler))
            .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

        let app = Router::new()
            .merge(api_routes)
            .route("/api/login", post(login))
            .route("/metrics", get(prometheus_metrics_handler_wrapper))
            .fallback(static_handler)
            .layer(middleware::from_fn(request_id_middleware))
            .with_state(state);

        if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
            let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
                .await
                .expect("Failed to load TLS certificates");
            info!("🔒 Web UI running on https://localhost:{} (TLS)", port);
            axum_server::bind_rustls(
                format!("0.0.0.0:{}", port).parse().unwrap(),
                config,
            )
            .serve(app.into_make_service())
            .await
            .unwrap();
        } else {
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
            info!("🌐 Web UI running on http://localhost:{}", port);
            axum::serve(listener, app).await.unwrap();
        }
    }
}

async fn request_id_middleware(
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let request_id = req.headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let span = tracing::info_span!("request", request_id = %request_id, method = %req.method(), path = %req.uri().path());
    let _enter = span.enter();

    let mut response = next.run(req).await;
    response.headers_mut().insert(
        "x-request-id",
        request_id.parse().unwrap(),
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

// Phase 2.4 Handlers

async fn upload_dag(State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>, mut multipart: Multipart) -> Response {
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("").to_string();
        if name == "file" && file_name.ends_with(".py") {
            let data = match field.bytes().await {
                Ok(b) => b,
                Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
            };
            let content = String::from_utf8_lossy(&data);
            
            let dags_dir = std::env::current_dir()
                .map(|p| p.join("dags").to_string_lossy().to_string())
                .unwrap_or_else(|_| "dags".to_string());
            fs::create_dir_all(&dags_dir).ok();
            let file_path = format!("{}/{}", dags_dir, file_name);
            if let Err(e) = fs::write(&file_path, &data) {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to save file: {}", e)}))).into_response();
            }

            match crate::python_parser::parse_python_dag(&file_path) {
                Ok(parsed_dags) => {
                    if parsed_dags.is_empty() {
                        let _ = fs::remove_file(&file_path);
                        return (StatusCode::BAD_REQUEST, Json(json!({"error": "No distinct DAGs found in file"}))).into_response();
                    }
                    
                    let dag = parsed_dags.into_iter().next().unwrap();
                    let dag_id = dag.id.clone();
                    let task_count = dag.tasks.len();
                    
                    {
                        let mut dags = state.dags.lock().unwrap();
                        dags.insert(dag.id.clone(), Arc::new(dag.clone()));
                    }
                    
                    let _ = state.db.register_dag(&dag).await;
                    
                    info!("🚀 DAG Uploaded: {} ({} tasks)", dag_id, task_count);
                    let _ = state.db.log_audit_event(
                        &auth_user.username, "dag.upload", "dag", &dag_id,
                        &format!("{{\"file\":\"{}\"}}", file_name),
                    ).await;
                    
                    return Json(json!({ "dag_id": dag_id, "tasks": task_count })).into_response();
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
    match state.db.validate_user(&body.username, &body.password).await {
        Ok(Some((api_key, role))) => {
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

// Pillar 3: Secrets Handlers

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
async fn get_dags(State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    match state.db.get_all_dags().await {
        Ok(dags) => {
            if let Some(user_team_id) = auth_user.team_id {
                // Filter the list to only include DAGs matching the user's team
                let filtered: Vec<_> = dags.into_iter().filter(|d| {
                    d.get("team_id").and_then(|v| v.as_str()) == Some(&user_team_id)
                }).collect();
                Json(filtered)
            } else {
                Json(dags)
            }
        },
        Err(_) => Json(vec![]),
    }
}
async fn get_dag_tasks(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    let dag_db = state.db.get_dag_by_id(&id).await.unwrap_or_default();
    if let Some(dag) = &dag_db {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    let tasks = state.db.get_dag_tasks(&id).await.unwrap_or_default();
    
    // Pillar 4: Pass run_id if it exists in task_instances table
    let instances = state.db.get_task_instances(&id).await.unwrap_or_default();
    
    // Get dependencies from in-memory map
    let dependencies = {
        let map = state.dags.lock().unwrap();
        map.get(&id).map(|d| d.dependencies.clone()).unwrap_or_default()
    };
    
    Json(json!({
        "dag_id": id, 
        "tasks": tasks, 
        "instances": instances, 
        "dag": dag_db,
        "dependencies": dependencies
    })).into_response()
}
async fn get_dag_runs(Path(id): Path<String>, State(state): State<Arc<AppState>>, axum::extract::Extension(auth_user): axum::extract::Extension<AuthUser>) -> impl IntoResponse {
    if let Ok(Some(dag)) = state.db.get_dag_by_id(&id).await {
        if let Some(t) = dag.get("team_id").and_then(|v| v.as_str()) {
            if auth_user.team_id.as_deref() != Some(t) && auth_user.team_id.is_some() {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "DAG belongs to another team"}))).into_response();
            }
        }
    } else {
        return (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response();
    }

    let runs = state.db.get_dag_runs(&id).await.unwrap_or_default();
    Json(json!({"dag_id": id, "runs": runs})).into_response()
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
        triggered_by: "api".to_string(),
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
                // Pillar 4: Update the DB schema/tasks for the new version
                let _ = state.db.register_dag(&dag).await;
                
                {
                    let mut map = state.dags.lock().unwrap();
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

// ─── Phase 4 Wave 2: DAG Versioning & Rollback ──────────────────────────────

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

    // Find the specific version and read its file path
    let target_version = versions.iter().find(|v| v["id"].as_str() == Some(&version_id));
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
                            let mut map = state.dags.lock().unwrap();
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
                    let now = chrono::Utc::now();
                    
                    let schedule_str = crate::scheduler::normalize_schedule(schedule_expr);
                    if !schedule_str.is_empty() {
                        if let Ok(schedule) = schedule_str.parse::<cron::Schedule>() {
                            let iter: cron::ScheduleIterator<'_, chrono::Utc> = schedule.after(&last_run_utc);
                            for dt in iter {
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

    let start = chrono::DateTime::parse_from_rfc3339(&body.start_date).map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let end = chrono::DateTime::parse_from_rfc3339(&body.end_date).map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    let schedule_expr = dag_meta.get("schedule_interval").and_then(|v| v.as_str()).unwrap_or("");
    let schedule_str = crate::scheduler::normalize_schedule(schedule_expr);

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

    let _ = state.db.log_audit_event(&auth_user.username, "dag.backfill", "dag", &id, &format!("{{\"start\":\"{}\",\"end\":\"{}\"}}", start, end)).await;
    
    if let Ok(mut prog) = state.backfill_progress.write() {
        prog.insert(id.clone(), 0.0);
    }

    let total = intervals.len() as f32;
    for (i, dt) in intervals.into_iter().enumerate() {
        let _ = state.tx.send(ScheduleRequest {
            dag_id: id.clone(),
            triggered_by: "backfill".to_string(),
            run_type: RunType::Full,
            execution_date: Some(dt),
        }).await;
        
        if let Ok(mut prog) = state.backfill_progress.write() {
             prog.insert(id.clone(), (i as f32) / total);
        }
    }
    
    if let Ok(mut prog) = state.backfill_progress.write() {
        prog.insert(id.clone(), 1.0);
    }

    Json(json!({"message": "Backfill triggered", "start": start, "end": end, "intervals_queued": total as usize})).into_response()
}

async fn get_backfill_progress(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let progress = if let Ok(prog) = state.backfill_progress.read() {
        *prog.get(&id).unwrap_or(&0.0)
    } else {
        0.0
    };
    Json(json!({"dag_id": id, "progress": progress})).into_response()
}
async fn swarm_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({"enabled": state.swarm.enabled, "active_workers": state.swarm.active_worker_count().await, "queue_depth": state.swarm.queue_depth().await}))
}
async fn swarm_workers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(json!({"workers": state.swarm.get_workers_info().await}))
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
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

// ─── Phase 2: XCom Handlers ─────────────────────────────────────────────────

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

async fn xcom_list_handler(State(state): State<Arc<AppState>>, Path((dag_id, run_id)): Path<(String, String)>) -> Response {
    match state.xcom.xcom_pull_all(&dag_id, &run_id).await {
        Ok(entries) => Json(json!(entries)).into_response(),
        Err(e) => {
            let msg = e.to_string();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg}))).into_response()
        }
    }
}

// ─── Phase 2: Task Pool Handlers ────────────────────────────────────────────

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

// ─── Phase 2: Webhook Callback Handlers ─────────────────────────────────────

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

// ─── Phase 3: Prometheus Metrics Handler ────────────────────────────────────

async fn prometheus_metrics_handler_wrapper(State(state): State<Arc<AppState>>) -> Response {
    crate::metrics::metrics_handler(State(state.metrics.clone())).await.into_response()
}

// ─── Wave 2: Audit Log Handler ───────────────────────────────────────────────

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

// ─── Wave 2: Gantt Handler ───────────────────────────────────────────────────

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

// ─── Wave 2: Calendar Handler ────────────────────────────────────────────────

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

    // Gather all DAGs from DB
    let dags = match state.db.get_all_dags().await {
        Ok(d) => d,
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
        if let Ok(runs) = state.db.get_dag_runs(dag_id).await {
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

// ─── Wave 2: Teams Handler ───────────────────────────────────────────────────

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
    // Id in path should match the body team_id or be 'unassign'
    let target_team = if body.team_id.as_deref() == Some("unassign") { None } else { body.team_id.as_deref() };
    match state.db.assign_user_to_team(&username, target_team).await {
        Ok(()) => {
            let _ = state.db.log_audit_event(&auth_user.username, "user.assign_team", "user", &username, &format!("{{\"team_id\": {:?}}}", target_team)).await;
            Json(json!({"status": "assigned", "username": username, "team_id": target_team})).into_response()
        },
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response()
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
