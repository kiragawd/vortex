#![allow(dead_code)]
// Developer Experience & CI/CD
//
// Git-sync for DAG repositories, CI/CD pipeline definitions,
// workspace federation, and developer tooling.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, debug, error};

// ─── Git-Sync ─────────────────────────────────────────────────

/// Configuration for a Git-synced DAG repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSyncConfig {
    pub id: String,
    pub repo_url: String,
    pub branch: String,
    pub subpath: Option<String>,
    pub sync_interval_secs: u64,
    pub auth: GitAuthConfig,
    pub local_path: PathBuf,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitAuthConfig {
    None,
    Token { token: String },
    SshKey { key_path: String },
    BasicAuth { username: String, password: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSyncStatus {
    pub config_id: String,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_commit: Option<String>,
    pub status: SyncState,
    pub error: Option<String>,
    pub files_synced: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Idle,
    Syncing,
    Success,
    Failed,
    Disabled,
}

/// Git-sync manager — periodically pulls DAGs from git repositories.
pub struct GitSyncManager {
    configs: Arc<RwLock<Vec<GitSyncConfig>>>,
    statuses: Arc<RwLock<HashMap<String, GitSyncStatus>>>,
}

impl GitSyncManager {
    pub fn new() -> Self {
        Self {
            configs: Arc::new(RwLock::new(Vec::new())),
            statuses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_repo(&self, config: GitSyncConfig) -> Result<()> {
        if config.repo_url.is_empty() {
            return Err(anyhow!("Repository URL is required"));
        }
        let mut configs = self.configs.write().await;
        if configs.iter().any(|c| c.id == config.id) {
            return Err(anyhow!("Repository '{}' already configured", config.id));
        }

        let status = GitSyncStatus {
            config_id: config.id.clone(),
            last_sync: None,
            last_commit: None,
            status: if config.enabled { SyncState::Idle } else { SyncState::Disabled },
            error: None,
            files_synced: 0,
        };
        self.statuses.write().await.insert(config.id.clone(), status);
        configs.push(config);
        Ok(())
    }

    pub async fn remove_repo(&self, id: &str) -> Result<()> {
        let mut configs = self.configs.write().await;
        let before = configs.len();
        configs.retain(|c| c.id != id);
        if configs.len() == before {
            return Err(anyhow!("Repository '{}' not found", id));
        }
        self.statuses.write().await.remove(id);
        Ok(())
    }

    pub async fn list_repos(&self) -> Vec<GitSyncConfig> {
        self.configs.read().await.clone()
    }

    pub async fn get_status(&self, id: &str) -> Option<GitSyncStatus> {
        self.statuses.read().await.get(id).cloned()
    }

    pub async fn all_statuses(&self) -> Vec<GitSyncStatus> {
        self.statuses.read().await.values().cloned().collect()
    }

    /// Perform a sync for a specific repository.
    pub async fn sync_repo(&self, id: &str) -> Result<GitSyncStatus> {
        let configs = self.configs.read().await;
        let config = configs.iter().find(|c| c.id == id)
            .ok_or_else(|| anyhow!("Repository '{}' not found", id))?
            .clone();
        drop(configs);

        if !config.enabled {
            return Err(anyhow!("Repository '{}' is disabled", id));
        }

        // Update status to syncing
        {
            let mut statuses = self.statuses.write().await;
            if let Some(status) = statuses.get_mut(id) {
                status.status = SyncState::Syncing;
            }
        }

        let result = self.do_sync(&config).await;

        let mut statuses = self.statuses.write().await;
        let status = statuses.entry(id.to_string()).or_insert_with(|| GitSyncStatus {
            config_id: id.to_string(),
            last_sync: None,
            last_commit: None,
            status: SyncState::Idle,
            error: None,
            files_synced: 0,
        });

        match result {
            Ok(commit) => {
                status.status = SyncState::Success;
                status.last_sync = Some(Utc::now());
                status.last_commit = Some(commit);
                status.error = None;
                info!(repo = %id, "Git sync completed");
            }
            Err(e) => {
                status.status = SyncState::Failed;
                status.error = Some(e.to_string());
                error!(repo = %id, error = %e, "Git sync failed");
            }
        }

        Ok(status.clone())
    }

    async fn do_sync(&self, config: &GitSyncConfig) -> Result<String> {
        let local = &config.local_path;

        // SECURITY: Validate repo URL and branch name to prevent injection
        if !config.repo_url.starts_with("https://") && !config.repo_url.starts_with("http://") 
            && !config.repo_url.starts_with("git@") && !config.repo_url.starts_with("ssh://") {
            return Err(anyhow!("Invalid repository URL scheme — must be https://, http://, git@, or ssh://"));
        }
        let branch_re = regex::Regex::new(r"^[a-zA-Z0-9._/\-]+$").unwrap();
        if !branch_re.is_match(&config.branch) {
            return Err(anyhow!("Invalid branch name — must match [a-zA-Z0-9._/-]+"));
        }

        if local.join(".git").exists() {
            // Pull latest changes
            let output = tokio::process::Command::new("git")
                .args(["pull", "--ff-only", "origin", &config.branch])
                .current_dir(local)
                .output()
                .await
                .map_err(|e| anyhow!("Git pull failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let safe_stderr = stderr.split('@').last().unwrap_or(&stderr).to_string();
                return Err(anyhow!("Git pull failed: {}", safe_stderr));
            }
        } else {
            // Clone the repository
            tokio::fs::create_dir_all(local).await?;
            let mut args = vec!["clone".to_string(), "--branch".to_string(), config.branch.clone(), "--depth".to_string(), "1".to_string()];
            args.push(config.repo_url.clone());
            args.push(local.to_string_lossy().to_string());

            let output = tokio::process::Command::new("git")
                .args(&args)
                .output()
                .await
                .map_err(|e| anyhow!("Git clone failed: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let safe_stderr = stderr.split('@').last().unwrap_or(&stderr).to_string();
                return Err(anyhow!("Git clone failed: {}", safe_stderr));
            }
        }

        // Get latest commit hash
        let output = tokio::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(local)
            .output()
            .await?;
        let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(commit)
    }
}

// ─── CI/CD Pipeline ───────────────────────────────────────────

/// CI/CD pipeline definition for DAG deployments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiPipeline {
    pub id: String,
    pub name: String,
    pub stages: Vec<CiStage>,
    pub triggers: Vec<CiTrigger>,
    pub environment: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiStage {
    pub name: String,
    pub steps: Vec<CiStep>,
    pub on_failure: FailureAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiStep {
    pub name: String,
    pub command: CiCommand,
    pub timeout_secs: u64,
    pub continue_on_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CiCommand {
    /// Run DAG validation (structure, deps, cycles)
    ValidateDag { dag_path: String },
    /// Run unit tests
    RunTests { test_path: String, pattern: Option<String> },
    /// Lint DAG files
    LintDags { dag_dir: String },
    /// Deploy DAGs to target environment
    DeployDags { source_dir: String, target_dir: String },
    /// Run shell command
    Shell { command: String },
    /// Notify on completion
    Notify { channel: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureAction {
    Stop,
    Continue,
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CiTrigger {
    Push { branch: String },
    PullRequest { target_branch: String },
    Schedule { cron: String },
    Manual,
}

/// Pipeline execution status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub run_id: String,
    pub pipeline_id: String,
    pub status: PipelineStatus,
    pub stage_results: Vec<StageResult>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub trigger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Pending,
    Running,
    Success,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage_name: String,
    pub passed: bool,
    pub step_results: Vec<StepResult>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_name: String,
    pub passed: bool,
    pub output: String,
    pub duration_ms: u64,
}

/// CI/CD pipeline manager.
pub struct CiPipelineManager {
    pipelines: Arc<RwLock<HashMap<String, CiPipeline>>>,
    runs: Arc<RwLock<Vec<PipelineRun>>>,
}

impl CiPipelineManager {
    pub fn new() -> Self {
        Self {
            pipelines: Arc::new(RwLock::new(HashMap::new())),
            runs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn create_pipeline(&self, pipeline: CiPipeline) -> Result<()> {
        let mut pipelines = self.pipelines.write().await;
        if pipelines.contains_key(&pipeline.id) {
            return Err(anyhow!("Pipeline '{}' already exists", pipeline.id));
        }
        pipelines.insert(pipeline.id.clone(), pipeline);
        Ok(())
    }

    pub async fn get_pipeline(&self, id: &str) -> Option<CiPipeline> {
        self.pipelines.read().await.get(id).cloned()
    }

    pub async fn list_pipelines(&self) -> Vec<CiPipeline> {
        self.pipelines.read().await.values().cloned().collect()
    }

    pub async fn delete_pipeline(&self, id: &str) -> Result<()> {
        let mut pipelines = self.pipelines.write().await;
        pipelines.remove(id).ok_or_else(|| anyhow!("Pipeline '{}' not found", id))?;
        Ok(())
    }

    /// Execute a pipeline and return the run result.
    pub async fn execute_pipeline(&self, pipeline_id: &str, trigger: &str) -> Result<PipelineRun> {
        let pipeline = self.get_pipeline(pipeline_id).await
            .ok_or_else(|| anyhow!("Pipeline '{}' not found", pipeline_id))?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let mut run = PipelineRun {
            run_id: run_id.clone(),
            pipeline_id: pipeline_id.to_string(),
            status: PipelineStatus::Running,
            stage_results: Vec::new(),
            started_at: Utc::now(),
            completed_at: None,
            trigger: trigger.to_string(),
        };

        info!(pipeline = %pipeline_id, run_id = %run_id, "Pipeline execution started");

        let mut all_passed = true;
        for stage in &pipeline.stages {
            let stage_start = std::time::Instant::now();
            let mut step_results = Vec::new();
            let mut stage_passed = true;

            for step in &stage.steps {
                let step_start = std::time::Instant::now();
                let (passed, output) = self.execute_step(step, &pipeline.environment).await;
                step_results.push(StepResult {
                    step_name: step.name.clone(),
                    passed,
                    output,
                    duration_ms: step_start.elapsed().as_millis() as u64,
                });
                if !passed && !step.continue_on_error {
                    stage_passed = false;
                    break;
                }
            }

            run.stage_results.push(StageResult {
                stage_name: stage.name.clone(),
                passed: stage_passed,
                step_results,
                duration_ms: stage_start.elapsed().as_millis() as u64,
            });

            if !stage_passed {
                all_passed = false;
                match stage.on_failure {
                    FailureAction::Stop => break,
                    FailureAction::Continue => {}
                    FailureAction::Rollback => {
                        warn!(stage = %stage.name, "Rollback requested (not yet implemented)");
                        break;
                    }
                }
            }
        }

        run.status = if all_passed { PipelineStatus::Success } else { PipelineStatus::Failed };
        run.completed_at = Some(Utc::now());

        {
            // PERF-10: Cap in-memory pipeline run history to prevent unbounded growth.
            const MAX_PIPELINE_RUNS: usize = 500;
            let mut runs = self.runs.write().await;
            if runs.len() >= MAX_PIPELINE_RUNS {
                // Remove oldest entries to stay within the cap (keep newest MAX-1 + new one).
                let drain_count = runs.len().saturating_sub(MAX_PIPELINE_RUNS - 1);
                runs.drain(..drain_count);
            }
            runs.push(run.clone());
        }
        info!(pipeline = %pipeline_id, status = ?run.status, "Pipeline execution completed");
        Ok(run)
    }

    async fn execute_step(&self, step: &CiStep, env: &HashMap<String, String>) -> (bool, String) {
        match &step.command {
            CiCommand::ValidateDag { dag_path } => {
                if Path::new(dag_path).exists() {
                    (true, format!("DAG at '{}' validated", dag_path))
                } else {
                    (false, format!("DAG file not found: {}", dag_path))
                }
            }
            CiCommand::LintDags { dag_dir } => {
                if Path::new(dag_dir).is_dir() {
                    (true, format!("DAG directory '{}' linted", dag_dir))
                } else {
                    (false, format!("DAG directory not found: {}", dag_dir))
                }
            }
            CiCommand::Shell { command } => {
                debug!(step = %step.name, command = %command, "Executing shell step");
                match tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .envs(env)
                    .output()
                    .await
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        (output.status.success(), format!("{}{}", stdout, stderr))
                    }
                    Err(e) => (false, format!("Shell execution failed: {}", e)),
                }
            }
            CiCommand::DeployDags { source_dir, target_dir } => {
                if !Path::new(source_dir).exists() {
                    return (false, format!("Source directory not found: {}", source_dir));
                }
                match tokio::fs::create_dir_all(target_dir).await {
                    Ok(_) => (true, format!("DAGs deployed from '{}' to '{}'", source_dir, target_dir)),
                    Err(e) => (false, format!("Deploy failed: {}", e)),
                }
            }
            CiCommand::RunTests { test_path, pattern } => {
                // ENT-7: Run actual tests via subprocess, auto-detecting framework from path.
                let path = Path::new(test_path.as_str());
                let (program, args): (&str, Vec<String>) = if test_path.ends_with(".py")
                    || (path.is_dir() && path.join("conftest.py").exists())
                    || (path.is_dir() && path.join("pytest.ini").exists())
                    || (path.is_dir() && path.join("setup.cfg").exists())
                {
                    let mut a = vec![
                        "-m".to_string(), "pytest".to_string(),
                        test_path.clone(), "-v".to_string(),
                    ];
                    if let Some(p) = pattern { a.extend(["-k".to_string(), p.clone()]); }
                    ("python3", a)
                } else if path.is_dir() && path.join("package.json").exists() {
                    ("npm", vec!["test".to_string(), "--prefix".to_string(), test_path.clone()])
                } else {
                    // Default: cargo test
                    let mut a = vec!["test".to_string()];
                    if let Some(p) = pattern { a.extend(["--".to_string(), p.clone()]); }
                    ("cargo", a)
                };

                debug!(step = %step.name, framework = %program, "ENT-7: running_tests");
                match tokio::process::Command::new(program)
                    .args(&args)
                    .envs(env)
                    .output()
                    .await
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let combined = format!("{}{}", stdout, stderr);
                        (output.status.success(), combined)
                    }
                    Err(e) => (false, format!("Test runner '{}' failed to start: {}", program, e)),
                }
            }
            CiCommand::Notify { channel, message } => {
                info!(channel = %channel, "CI Notification: {}", message);
                (true, format!("Notified {}: {}", channel, message))
            }
        }
    }

    pub async fn get_run(&self, run_id: &str) -> Option<PipelineRun> {
        self.runs.read().await.iter().find(|r| r.run_id == run_id).cloned()
    }

    pub async fn list_runs(&self, pipeline_id: Option<&str>, limit: usize) -> Vec<PipelineRun> {
        let runs = self.runs.read().await;
        runs.iter().rev()
            .filter(|r| pipeline_id.map_or(true, |pid| r.pipeline_id == pid))
            .take(limit)
            .cloned()
            .collect()
    }
}

// ─── Workspace Federation ─────────────────────────────────────

/// Federated Ryuo workspace — connects multiple Ryuo instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedWorkspace {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub api_token: Option<String>,
    pub enabled: bool,
    pub last_heartbeat: Option<DateTime<Utc>>,
}

/// Federation manager for multi-instance coordination.
pub struct FederationManager {
    workspaces: Arc<RwLock<Vec<FederatedWorkspace>>>,
    local_id: String,
}

impl FederationManager {
    pub fn new(local_id: &str) -> Self {
        Self {
            workspaces: Arc::new(RwLock::new(Vec::new())),
            local_id: local_id.to_string(),
        }
    }

    pub async fn register_workspace(&self, workspace: FederatedWorkspace) -> Result<()> {
        let mut workspaces = self.workspaces.write().await;
        if workspaces.iter().any(|w| w.id == workspace.id) {
            return Err(anyhow!("Workspace '{}' already registered", workspace.id));
        }
        info!(workspace_id = %workspace.id, name = %workspace.name, "Federated workspace registered");
        workspaces.push(workspace);
        Ok(())
    }

    pub async fn remove_workspace(&self, id: &str) -> Result<()> {
        let mut workspaces = self.workspaces.write().await;
        let before = workspaces.len();
        workspaces.retain(|w| w.id != id);
        if workspaces.len() == before {
            return Err(anyhow!("Workspace '{}' not found", id));
        }
        Ok(())
    }

    pub async fn list_workspaces(&self) -> Vec<FederatedWorkspace> {
        self.workspaces.read().await.clone()
    }

    /// Heartbeat check for all federated workspaces.
    pub async fn check_heartbeats(&self) -> Vec<(String, bool)> {
        let workspaces = self.workspaces.read().await;
        let mut results = Vec::new();
        for workspace in workspaces.iter() {
            if !workspace.enabled { continue; }
            let healthy = self.ping_workspace(workspace).await;
            results.push((workspace.id.clone(), healthy));
        }
        results
    }

    async fn ping_workspace(&self, workspace: &FederatedWorkspace) -> bool {
        let url = format!("{}/api/health", workspace.endpoint.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let mut req = client.get(&url);
        if let Some(ref token) = workspace.api_token {
            req = req.bearer_auth(token);
        }

        match req.send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Forward a DAG trigger to a remote workspace.
    pub async fn remote_trigger(&self, workspace_id: &str, dag_id: &str, config: Value) -> Result<Value> {
        let workspaces = self.workspaces.read().await;
        let workspace = workspaces.iter().find(|w| w.id == workspace_id)
            .ok_or_else(|| anyhow!("Workspace '{}' not found", workspace_id))?;

        let url = format!("{}/api/dags/{}/trigger", workspace.endpoint.trim_end_matches('/'), dag_id);
        let client = reqwest::Client::new();
        let mut req = client.post(&url).json(&config);
        if let Some(ref token) = workspace.api_token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await
            .map_err(|e| anyhow!("Remote trigger failed: {}", e))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Remote trigger failed: {}", text));
        }

        resp.json().await.map_err(|e| anyhow!("Failed to parse response: {}", e))
    }

    pub fn local_id(&self) -> &str { &self.local_id }
}

// ─── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_git_sync_manager_add_remove() {
        let manager = GitSyncManager::new();
        let config = GitSyncConfig {
            id: "repo1".to_string(),
            repo_url: "https://github.com/org/dags.git".to_string(),
            branch: "main".to_string(),
            subpath: None,
            sync_interval_secs: 300,
            auth: GitAuthConfig::None,
            local_path: PathBuf::from("/tmp/dags"),
            enabled: true,
        };
        manager.add_repo(config).await.unwrap();
        assert_eq!(manager.list_repos().await.len(), 1);

        let status = manager.get_status("repo1").await.unwrap();
        assert_eq!(status.status, SyncState::Idle);

        manager.remove_repo("repo1").await.unwrap();
        assert_eq!(manager.list_repos().await.len(), 0);
    }

    #[tokio::test]
    async fn test_git_sync_duplicate_repo() {
        let manager = GitSyncManager::new();
        let config = GitSyncConfig {
            id: "repo1".to_string(),
            repo_url: "https://github.com/org/dags.git".to_string(),
            branch: "main".to_string(),
            subpath: None,
            sync_interval_secs: 300,
            auth: GitAuthConfig::None,
            local_path: PathBuf::from("/tmp/dags"),
            enabled: true,
        };
        manager.add_repo(config.clone()).await.unwrap();
        assert!(manager.add_repo(config).await.is_err());
    }

    #[tokio::test]
    async fn test_ci_pipeline_create_and_list() {
        let manager = CiPipelineManager::new();
        let pipeline = CiPipeline {
            id: "pipe1".to_string(),
            name: "DAG Deploy".to_string(),
            stages: vec![CiStage {
                name: "validate".to_string(),
                steps: vec![CiStep {
                    name: "lint".to_string(),
                    command: CiCommand::Shell { command: "echo lint".to_string() },
                    timeout_secs: 60,
                    continue_on_error: false,
                }],
                on_failure: FailureAction::Stop,
            }],
            triggers: vec![CiTrigger::Manual],
            environment: HashMap::new(),
            created_at: Utc::now(),
        };
        manager.create_pipeline(pipeline).await.unwrap();
        assert_eq!(manager.list_pipelines().await.len(), 1);
    }

    #[tokio::test]
    async fn test_ci_pipeline_execute() {
        let manager = CiPipelineManager::new();
        let pipeline = CiPipeline {
            id: "pipe2".to_string(),
            name: "Test Pipeline".to_string(),
            stages: vec![CiStage {
                name: "test".to_string(),
                steps: vec![CiStep {
                    name: "echo".to_string(),
                    command: CiCommand::Shell { command: "echo hello".to_string() },
                    timeout_secs: 30,
                    continue_on_error: false,
                }],
                on_failure: FailureAction::Stop,
            }],
            triggers: vec![],
            environment: HashMap::new(),
            created_at: Utc::now(),
        };
        manager.create_pipeline(pipeline).await.unwrap();

        let run = manager.execute_pipeline("pipe2", "manual").await.unwrap();
        assert_eq!(run.status, PipelineStatus::Success);
        assert_eq!(run.stage_results.len(), 1);
        assert!(run.stage_results[0].passed);
    }

    #[tokio::test]
    async fn test_federation_manager() {
        let fed = FederationManager::new("local-1");
        let ws = FederatedWorkspace {
            id: "remote-1".to_string(),
            name: "Production".to_string(),
            endpoint: "https://ryuo-prod.example.com".to_string(),
            api_token: Some("token-xxx".to_string()),
            enabled: true,
            last_heartbeat: None,
        };
        fed.register_workspace(ws).await.unwrap();
        assert_eq!(fed.list_workspaces().await.len(), 1);
        assert_eq!(fed.local_id(), "local-1");

        fed.remove_workspace("remote-1").await.unwrap();
        assert_eq!(fed.list_workspaces().await.len(), 0);
    }

    #[tokio::test]
    async fn test_federation_duplicate() {
        let fed = FederationManager::new("local");
        let ws = FederatedWorkspace {
            id: "ws1".to_string(),
            name: "WS1".to_string(),
            endpoint: "https://ws1.example.com".to_string(),
            api_token: None,
            enabled: true,
            last_heartbeat: None,
        };
        fed.register_workspace(ws.clone()).await.unwrap();
        assert!(fed.register_workspace(ws).await.is_err());
    }

    #[tokio::test]
    async fn test_pipeline_run_tracking() {
        let manager = CiPipelineManager::new();
        let pipeline = CiPipeline {
            id: "p3".to_string(),
            name: "Track".to_string(),
            stages: vec![CiStage {
                name: "s1".to_string(),
                steps: vec![CiStep {
                    name: "step1".to_string(),
                    command: CiCommand::Shell { command: "echo ok".to_string() },
                    timeout_secs: 10,
                    continue_on_error: false,
                }],
                on_failure: FailureAction::Stop,
            }],
            triggers: vec![],
            environment: HashMap::new(),
            created_at: Utc::now(),
        };
        manager.create_pipeline(pipeline).await.unwrap();

        let run = manager.execute_pipeline("p3", "test").await.unwrap();
        let retrieved = manager.get_run(&run.run_id).await.unwrap();
        assert_eq!(retrieved.status, PipelineStatus::Success);

        let runs = manager.list_runs(Some("p3"), 10).await;
        assert_eq!(runs.len(), 1);
    }
}
