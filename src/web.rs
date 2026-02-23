use axum::{
    extract::{Path, State, Multipart},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, patch, delete},
    Json, Router,
};
use rust_embed::RustEmbed;
use std::sync::Arc;
use crate::db::Db;
use crate::swarm::SwarmState;
use crate::vault::Vault;
use crate::python_parser;
use serde_json::json;
use serde::Deserialize;
use std::fs;
use tokio::sync::mpsc;
use rusqlite::params;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

pub struct AppState {
    pub db: Arc<Db>,
    pub tx: mpsc::Sender<(String, String)>,
    pub swarm: Arc<SwarmState>,
    pub vault: Option<Arc<Vault>>,
}

pub struct WebServer {
    db: Arc<Db>,
    tx: mpsc::Sender<(String, String)>,
    swarm: Arc<SwarmState>,
    vault: Option<Arc<Vault>>,
}

impl WebServer {
    pub fn new(db: Arc<Db>, tx: mpsc::Sender<(String, String)>, swarm: Arc<SwarmState>, vault: Option<Arc<Vault>>) -> Self {
        Self { db, tx, swarm, vault }
    }

    pub async fn run(self, port: u16) {
        let state = Arc::new(AppState {
            db: self.db,
            tx: self.tx,
            swarm: self.swarm,
            vault: self.vault,
        });

        let app = Router::new()
            .route("/api/dags", get(get_dags))
            .route("/api/dags/upload", post(upload_dag))
            .route("/api/dags/:id/tasks", get(get_dag_tasks))
            .route("/api/dags/:id/runs", get(get_dag_runs))
            .route("/api/dags/:id/trigger", post(trigger_dag))
            .route("/api/dags/:id/pause", patch(pause_dag))
            .route("/api/dags/:id/unpause", patch(unpause_dag))
            .route("/api/dags/:id/schedule", patch(update_schedule))
            .route("/api/dags/:id/backfill", post(backfill_dag))
            .route("/api/dags/:id/validate", get(validate_dag_id))
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
            .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
            .fallback(static_handler)
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
        println!("🌐 Web UI running on http://localhost:{}", port);
        axum::serve(listener, app).await.unwrap();
    }
}

async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req.headers().get("Authorization").and_then(|h| h.to_str().ok());
    let api_key = match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => &auth[7..],
        Some(auth) => auth,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    match state.db.get_user_by_api_key(api_key) {
        Ok(Some((username, role))) => {
            let path = req.uri().path();
            
            // RBAC Logic:
            // Admin: Can do everything
            // Operator: Can trigger, pause, manage DAGs. CANNOT manage Users or Secrets.
            // Viewer: Read-only.
            
            let is_admin_route = path.contains("/api/users") || path.contains("/api/secrets");
            let is_write_route = path.contains("/trigger") || path.contains("/pause") || 
                               path.contains("/unpause") || path.contains("/schedule") || 
                               path.contains("/backfill") || path.contains("/drain") ||
                               path.contains("/api/users") || path.contains("/api/secrets") ||
                               path.contains("/api/dags/upload");

            if role == "Viewer" && is_write_route {
                return Err(StatusCode::FORBIDDEN);
            }
            
            if role == "Operator" && is_admin_route {
                return Err(StatusCode::FORBIDDEN);
            }

            println!("👤 Authenticated: {} (Role: {})", username, role);
            Ok(next.run(req).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// Phase 2.4 Handlers

async fn upload_dag(State(state): State<Arc<AppState>>, mut multipart: Multipart) -> Response {
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().unwrap_or("").to_string();
        if name == "file" && file_name.ends_with(".py") {
            let data = match field.bytes().await {
                Ok(b) => b,
                Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
            };
            let content = String::from_utf8_lossy(&data);
            
            // Validate content
            match python_parser::parse_dag_file(&content) {
                Ok(dag_meta) => {
                    let dags_dir = "/Users/ashwin/vortex/dags";
                    fs::create_dir_all(dags_dir).ok();
                    let file_path = format!("{}/{}", dags_dir, file_name);
                    if let Err(e) = fs::write(&file_path, &data) {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to save file: {}", e)}))).into_response();
                    }
                    
                    // Store in DB
                    let _ = state.db.save_dag(&dag_meta.dag_id, dag_meta.schedule_interval.as_deref());
                    for task in &dag_meta.tasks {
                        let cmd = task.config.get("bash_command").and_then(|c| c.as_str()).unwrap_or("");
                        let task_type = if task.config.get("python_callable").is_some() { "python" } else { "bash" };
                        let _ = state.db.save_task(&dag_meta.dag_id, &task.task_id, &task.task_id, cmd, task_type, "{}", 0, 30);
                    }
                    
                    // Versioning
                    let _ = state.db.store_dag_version(&dag_meta.dag_id, &file_path);
                    
                    println!("🚀 DAG Uploaded: {} ({} tasks)", dag_meta.dag_id, dag_meta.tasks.len());
                    return Json(dag_meta).into_response();
                },
                Err(e) => {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error": format!("Invalid DAG file: {}", e)}))).into_response();
                }
            }
        }
    }
    (StatusCode::BAD_REQUEST, Json(json!({"error": "No .py file provided"}))).into_response()
}

async fn validate_dag_id(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    match state.db.get_latest_version(&id) {
        Ok(Some(version)) => {
            let path = version["file_path"].as_str().unwrap_or("");
            match fs::read_to_string(path) {
                Ok(content) => match python_parser::parse_dag_file(&content) {
                    Ok(meta) => Json(json!({"valid": true, "metadata": meta})).into_response(),
                    Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"valid": false, "error": e.to_string()}))).into_response(),
                },
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
            }
        },
        _ => (StatusCode::NOT_FOUND, Json(json!({"error": "DAG not found"}))).into_response(),
    }
}

// RBAC: User Handlers

async fn get_users(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_all_users() {
        Ok(users) => Json(users),
        Err(_) => Json(vec![]),
    }
}

#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    password_hash: String, // Simplified for now (plain storage or pre-hashed)
    role: String,
}

async fn create_user(State(state): State<Arc<AppState>>, Json(body): Json<CreateUserRequest>) -> Response {
    let api_key = format!("vx_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
    match state.db.create_user(&body.username, &body.password_hash, &body.role, &api_key) {
        Ok(_) => Json(json!({ "message": "User created", "api_key": api_key })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn delete_user(Path(username): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    if username == "admin" {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Cannot delete primary admin" }))).into_response();
    }
    match state.db.delete_user(&username) {
        Ok(_) => Json(json!({ "message": "User deleted" })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// Pillar 3: Secrets Handlers

async fn get_secrets(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_all_secrets() {
        Ok(keys) => Json(json!({ "secrets": keys })),
        Err(_) => Json(json!({ "secrets": [] })),
    }
}

#[derive(Deserialize)]
struct SecretRequest {
    key: String,
    value: String,
}

async fn store_secret(State(state): State<Arc<AppState>>, Json(body): Json<SecretRequest>) -> Response {
    let vault = match &state.vault {
        Some(v) => v,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Secret Vault is not initialized" }))).into_response(),
    };

    match vault.encrypt(&body.value) {
        Ok(encrypted) => {
            if let Err(e) = state.db.store_secret(&body.key, &encrypted) {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB Error: {}", e) }))).into_response();
            }
            println!("🔐 Secret stored: {}", body.key);
            Json(json!({ "message": "Secret stored successfully" })).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("Encryption failure: {}", e) }))).into_response(),
    }
}

async fn delete_secret(Path(key): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    match state.db.delete_secret(&key) {
        Ok(_) => Json(json!({ "message": "Secret deleted" })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// Existing Handlers
async fn get_dags(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.get_all_dags() { Ok(dags) => Json(dags), Err(_) => Json(vec![]), }
}
async fn get_dag_tasks(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let tasks = state.db.get_dag_tasks(&id).unwrap_or_default();
    let instances = state.db.get_task_instances(&id).unwrap_or_default();
    let dag = state.db.get_dag_by_id(&id).ok().flatten();
    Json(json!({"dag_id": id, "tasks": tasks, "instances": instances, "dag": dag}))
}
async fn get_dag_runs(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let runs = state.db.get_dag_runs(&id).unwrap_or_default();
    Json(json!({"dag_id": id, "runs": runs}))
}
async fn get_task_logs(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // 1. Try DB first
    let mut db_stdout = None;
    let mut db_stderr = None;
    {
        let conn = state.db.conn.lock().unwrap();
        if let Ok((stdout, stderr)) = conn.query_row(
            "SELECT stdout, stderr FROM task_instances WHERE id = ?1",
            params![id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<String>>(1)?))
        ) {
            db_stdout = stdout;
            db_stderr = stderr;
        }
    }

    if db_stdout.is_some() || db_stderr.is_some() {
        return Json(json!({ 
            "stdout": db_stdout.unwrap_or_default(), 
            "stderr": db_stderr.unwrap_or_default() 
        })).into_response();
    }

    // 2. Fallback to file logs
    if let Ok(Some((dag_id, task_id, execution_date))) = state.db.get_task_instance(&id) {
        let log_path = format!("logs/{}/{}/{}.log", dag_id, task_id, execution_date.format("%Y-%m-%d"));
        if let Ok(content) = fs::read_to_string(log_path) { 
            return Json(json!({ "stdout": content, "stderr": "" })).into_response(); 
        }
    }
    
    (StatusCode::NOT_FOUND, Json(json!({ "error": "Log not found" }))).into_response()
}
async fn trigger_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let _ = state.tx.send((id, "manual".to_string())).await;
    Json(json!({ "message": "Triggered" }))
}
async fn pause_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let _ = state.db.pause_dag(&id); Json(json!({"message": "Paused"}))
}
async fn unpause_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let _ = state.db.unpause_dag(&id); Json(json!({"message": "Unpaused"}))
}

#[derive(Deserialize)]
struct ScheduleUpdate { schedule_interval: Option<String>, timezone: Option<String>, max_active_runs: Option<i32>, catchup: Option<bool>, }
async fn update_schedule(Path(id): Path<String>, State(state): State<Arc<AppState>>, Json(body): Json<ScheduleUpdate>) -> impl IntoResponse {
    if let Ok(Some(current)) = state.db.get_dag_by_id(&id) {
        let schedule = body.schedule_interval.or_else(|| current["schedule_interval"].as_str().map(|s| s.to_string()));
        let timezone = body.timezone.unwrap_or_else(|| current["timezone"].as_str().unwrap_or("UTC").to_string());
        let max_active = body.max_active_runs.unwrap_or_else(|| current["max_active_runs"].as_i64().unwrap_or(1) as i32);
        let catchup = body.catchup.unwrap_or_else(|| current["catchup"].as_bool().unwrap_or(false));
        let _ = state.db.update_dag_config(&id, schedule.as_deref(), &timezone, max_active, catchup);
    }
    Json(json!({"message": "Updated"}))
}
#[derive(Deserialize)]
struct BackfillRequest { start_date: String, end_date: String, }
async fn backfill_dag(Path(id): Path<String>, State(state): State<Arc<AppState>>, Json(_body): Json<BackfillRequest>) -> impl IntoResponse {
    let _ = state.tx.send((id, "backfill".to_string())).await;
    Json(json!({"message": "Backfill triggered"}))
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
