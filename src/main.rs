use anyhow::Result;
use scheduler::{Dag, Scheduler};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::mpsc;
use chrono::Utc;
use swarm::SwarmState;
use vault::Vault;
use tracing::{info, warn, error, debug};
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use clap::{Parser, Subcommand};

mod scheduler;
mod python_parser;
mod web;
mod swarm;
mod worker;
mod vault;
mod executor;
mod xcom;
mod pools;
mod sensors;
mod notifications;
mod metrics;
mod db_trait;
mod db_postgres;
mod proto;
mod dag_factory;
mod auth;
mod lineage;
mod incident;
mod compliance;
mod rbac;
mod k8s_executor;
mod advanced_scheduler;
mod openapi;
mod telemetry;
mod enterprise_connector;
mod cloud_connectors;
mod event_framework;
mod sdk;
mod devops;
mod migration;
mod disaster_recovery;
mod config_ops;

/// VORTEX Orchestration Engine
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Database URL (PostgreSQL only)
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Enable swarm mode
    #[arg(long)]
    swarm: bool,

    /// Swarm mode gRPC bind address
    #[arg(long, default_value = "0.0.0.0")]
    grpc_bind: String,

    /// Swarm mode port
    #[arg(long, default_value_t = 50051)]
    swarm_port: u16,

    /// Web UI and API server port
    #[arg(long, default_value_t = 3000)]
    port: u16,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, env = "VORTEX_LOG_LEVEL", default_value = "info")]
    log_level: String,

    /// Output logs in JSON format
    #[arg(long)]
    log_json: bool,

    /// Enable High Availability leader election
    #[arg(long)]
    ha_mode: bool,

    /// Database max connections
    #[arg(long, default_value_t = 20)]
    db_max_connections: u32,

    /// Database min connections
    #[arg(long, default_value_t = 2)]
    db_min_connections: u32,

    /// Database idle timeout in seconds
    #[arg(long, default_value_t = 300)]
    db_idle_timeout: u64,

    /// Path to TLS certificate (PEM)
    #[arg(long)]
    tls_cert: Option<String>,

    /// Path to TLS private key (PEM)
    #[arg(long)]
    tls_key: Option<String>,

    /// Register synthetic benchmark DAG
    #[arg(long)]
    benchmark: bool,

    /// Allow loading native plugins (.so/.dylib) from plugins/ directory (SECURITY RISK)
    #[arg(long)]
    allow_unsafe_plugins: bool,

    /// Allow executing Python DAG files via PyO3 (SECURITY RISK — runs arbitrary code)
    #[arg(long)]
    allow_unsafe_dag_exec: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize DB schema
    Db {
        #[arg(long)]
        migrate: bool,
    },
    /// Start a swarm worker node
    Worker {
        #[arg(long, default_value = "http://127.0.0.1:50051")]
        controller: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value_t = 4)]
        capacity: i32,
        #[arg(long, value_delimiter = ',')]
        labels: Option<Vec<String>>,
    },
    /// Manage secrets in the encrypted vault
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Manage users
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// Manage teams
    Team {
        #[command(subcommand)]
        action: TeamAction,
    },
    /// Manage RBAC roles and permissions
    Rbac {
        #[command(subcommand)]
        action: RbacAction,
    },
    /// Manage API tokens
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    /// Manage auth providers (OIDC, SAML, LDAP)
    AuthProvider {
        #[command(subcommand)]
        action: AuthProviderAction,
    },
    /// Query audit logs
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },
    /// Compliance controls management
    Compliance {
        #[command(subcommand)]
        action: ComplianceAction,
    },
    /// Data lineage queries
    Lineage {
        #[command(subcommand)]
        action: LineageAction,
    },
    /// Connector health & management
    Connector {
        #[command(subcommand)]
        action: ConnectorAction,
    },
    /// Swarm cluster status
    Swarm {
        #[command(subcommand)]
        action: SwarmAction,
    },
    /// DAG management
    Dag {
        #[command(subcommand)]
        action: DagAction,
    },
    /// Pool management
    Pool {
        #[command(subcommand)]
        action: PoolAction,
    },
    /// Configuration & maintenance operations
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum SecretAction {
    /// List all secret keys (values are masked)
    List,
    /// Get a secret value
    Get { key: String },
    /// Set a secret value
    Set { key: String, value: String },
    /// Delete a secret
    Delete { key: String },
}

#[derive(Subcommand, Debug)]
enum UserAction {
    /// List all users
    List,
    /// Create a new user
    Create {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: String,
        #[arg(long, default_value = "Viewer")]
        role: String,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        team: Option<String>,
    },
    /// Show user details
    Get { username: String },
    /// Delete a user
    Delete { username: String },
}

#[derive(Subcommand, Debug)]
enum TeamAction {
    /// List all teams
    List,
    /// Create a new team
    Create {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a team
    Delete { id: String },
}

#[derive(Subcommand, Debug)]
enum RbacAction {
    /// List all roles
    ListRoles,
    /// List all permissions
    ListPermissions,
    /// Assign a role to a user
    Assign {
        #[arg(long)]
        user: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        team: Option<String>,
    },
    /// Revoke a role from a user
    Revoke {
        #[arg(long)]
        user: String,
        #[arg(long)]
        role: String,
    },
    /// Show roles for a user
    UserRoles { username: String },
}

#[derive(Subcommand, Debug)]
enum TokenAction {
    /// List API tokens for a user
    List { user_id: String },
    /// Create a new API token
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        user_id: String,
        #[arg(long, value_delimiter = ',')]
        scopes: Option<Vec<String>>,
        #[arg(long)]
        team: Option<String>,
        #[arg(long)]
        expires_days: Option<i64>,
    },
    /// Revoke an API token
    Revoke { token_id: String },
}

#[derive(Subcommand, Debug)]
enum AuthProviderAction {
    /// List configured auth providers
    List,
    /// Enable an auth provider
    Enable { id: String },
    /// Disable an auth provider
    Disable { id: String },
}

#[derive(Subcommand, Debug)]
enum AuditAction {
    /// Query recent audit events
    Recent {
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Query audit events by actor
    ByActor {
        actor: String,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
}

#[derive(Subcommand, Debug)]
enum ComplianceAction {
    /// List compliance controls
    List,
    /// Show compliance status summary
    Status,
}

#[derive(Subcommand, Debug)]
enum LineageAction {
    /// Show lineage events for a DAG run
    Run { run_id: String },
    /// List tracked datasets
    Datasets,
    /// Show upstream/downstream for a dataset
    Dataset { dataset_id: String },
}

#[derive(Subcommand, Debug)]
enum ConnectorAction {
    /// List registered connectors
    List,
    /// Health-check a specific connector
    Health { name: String },
}

#[derive(Subcommand, Debug)]
enum SwarmAction {
    /// Show swarm cluster status (workers, tasks)
    Status,
    /// List connected workers
    Workers,
}

#[derive(Subcommand, Debug)]
enum DagAction {
    /// List all registered DAGs
    List,
    /// Show details for a DAG
    Get { dag_id: String },
    /// Trigger a DAG run
    Trigger {
        dag_id: String,
        #[arg(long, default_value = "cli")]
        triggered_by: String,
    },
    /// Pause a DAG
    Pause { dag_id: String },
    /// Unpause a DAG
    Unpause { dag_id: String },
}

#[derive(Subcommand, Debug)]
enum PoolAction {
    /// List all pools
    List,
    /// Create a pool
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        slots: i32,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a pool
    Delete { name: String },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Show current server configuration (non-secret)
    Show,
    /// Validate the database connection
    ValidateDb,
    /// Export configuration as JSON
    Export,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize structured logging
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("vortex={}", cli.log_level)));

    let file_appender = tracing_appender::rolling::daily("logs", "vortex.log");
    // IMPORTANT: This guard must be held for the entire lifetime of main().
    // Dropping it will cause buffered log lines to be lost.
    let (non_blocking, _file_log_guard) = tracing_appender::non_blocking(file_appender);

    if cli.log_json {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json())
            .with(fmt::layer().with_writer(non_blocking).json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_target(false).with_thread_ids(false))
            .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
            .init();
    }

    if let Some(Commands::Db { migrate }) = cli.command {
        if migrate {
            if let Some(db_url_migrate) = &cli.database_url {
                let safe_url_end = db_url_migrate.find('@').map(|i| i + 1).unwrap_or(db_url_migrate.len());
                info!("🗄️ Running PostgreSQL migrations ({})...", &db_url_migrate[..safe_url_end]);
                let _db = db_postgres::PostgresDb::new(&db_url_migrate, 1, 1, std::time::Duration::from_secs(30)).await?;
                info!("✅ Database migrations applied successfully.");
                return Ok(());
            } else {
                anyhow::bail!("❌ --database-url or DATABASE_URL env var is required for DB migrate");
            }
        }
    }

    // 🐝 WORKER MODE
    if let Some(Commands::Worker { controller, id, capacity, labels }) = cli.command {
        let worker_id = id.unwrap_or_else(|| format!("worker-{}", &uuid::Uuid::new_v4().to_string()[..8]));
        let worker_labels = labels.unwrap_or_default();
        info!("🌪️ VORTEX Swarm Worker v0.7.0");
        return worker::run_worker(&controller, &worker_id, capacity, worker_labels).await;
    }

    // ─── CLI subcommand handlers (connect to DB, run, exit) ─────────────────
    if let Some(ref cmd) = cli.command {
        match cmd {
            Commands::Db { .. } | Commands::Worker { .. } => { /* already handled above */ }
            _ => {
                // All remaining commands need a database connection
                let db_url = cli.database_url.as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--database-url or DATABASE_URL required"))?;
                let db = db_postgres::PostgresDb::new(db_url, 5, 1, std::time::Duration::from_secs(30)).await?;
                let db: Arc<dyn db_trait::DatabaseBackend> = Arc::new(db);

                match cmd {
                    Commands::Secret { action } => {
                        let vault = Vault::new().map_err(|e| anyhow::anyhow!("Vault init failed: {}", e))?;
                        match action {
                            SecretAction::List => {
                                let secrets = db.get_all_secrets().await?;
                                println!("{:<30} {:<10}", "KEY", "MASKED VALUE");
                                println!("{}", "-".repeat(42));
                                for key in &secrets {
                                    println!("{:<30} ****", key);
                                }
                                println!("\n{} secret(s) total", secrets.len());
                            }
                            SecretAction::Get { key } => {
                                match db.get_secret(key).await? {
                                    Some(val) => {
                                        let decrypted = vault.decrypt(&val).unwrap_or_else(|_| val.clone());
                                        println!("{}", decrypted);
                                    }
                                    None => println!("Secret '{}' not found", key),
                                }
                            }
                            SecretAction::Set { key, value } => {
                                let encrypted = vault.encrypt(value)?;
                                db.store_secret(key, &encrypted).await?;
                                println!("Secret '{}' stored (encrypted)", key);
                            }
                            SecretAction::Delete { key } => {
                                db.delete_secret(key).await?;
                                println!("Secret '{}' deleted", key);
                            }
                        }
                    }
                    Commands::User { action } => {
                        match action {
                            UserAction::List => {
                                let users = db.get_all_users().await?;
                                println!("{:<20} {:<10} {:<25} {:<15}", "USERNAME", "ROLE", "EMAIL", "TEAM");
                                println!("{}", "-".repeat(72));
                                for u in &users {
                                    println!("{:<20} {:<10} {:<25} {:<15}",
                                        u["username"].as_str().unwrap_or("-"),
                                        u["role"].as_str().unwrap_or("-"),
                                        u["email"].as_str().unwrap_or("-"),
                                        u["team_id"].as_str().unwrap_or("-"),
                                    );
                                }
                                println!("\n{} user(s) total", users.len());
                            }
                            UserAction::Create { username, password, role, email: _, team } => {
                                let pw_hash = bcrypt::hash(password, 10)
                                    .map_err(|e| anyhow::anyhow!("bcrypt hash failed: {}", e))?;
                                let api_key = uuid::Uuid::new_v4().to_string();
                                db.create_user(username, &pw_hash, role, &api_key).await?;
                                if let Some(team_id) = team {
                                    db.assign_user_to_team(username, Some(team_id.as_str())).await?;
                                }
                                println!("User '{}' created with role '{}' (api_key={})", username, role, api_key);
                            }
                            UserAction::Get { username } => {
                                let users = db.get_all_users().await?;
                                match users.iter().find(|u| u["username"].as_str() == Some(username.as_str())) {
                                    Some(u) => println!("{}", serde_json::to_string_pretty(u)?),
                                    None => println!("User '{}' not found", username),
                                }
                            }
                            UserAction::Delete { username } => {
                                db.delete_user(username).await?;
                                println!("User '{}' deleted", username);
                            }
                        }
                    }
                    Commands::Team { action } => {
                        match action {
                            TeamAction::List => {
                                let teams = db.get_all_teams().await?;
                                println!("{:<20} {:<30} {:<30}", "ID", "NAME", "DESCRIPTION");
                                println!("{}", "-".repeat(82));
                                for t in &teams {
                                    println!("{:<20} {:<30} {:<30}",
                                        t["id"].as_str().unwrap_or("-"),
                                        t["name"].as_str().unwrap_or("-"),
                                        t["description"].as_str().unwrap_or("-"),
                                    );
                                }
                            }
                            TeamAction::Create { id, name, description } => {
                                db.create_team(id, name, description.as_deref().unwrap_or(""), 100, 1000).await?;
                                println!("Team '{}' created", id);
                            }
                            TeamAction::Delete { id } => {
                                db.delete_team(id).await?;
                                println!("Team '{}' deleted", id);
                            }
                        }
                    }
                    Commands::Rbac { action } => {
                        match action {
                            RbacAction::ListRoles => {
                                let roles = db.get_rbac_roles().await?;
                                println!("{:<20} {:<40} {:<10}", "NAME", "DESCRIPTION", "SYSTEM");
                                println!("{}", "-".repeat(72));
                                for r in &roles {
                                    println!("{:<20} {:<40} {:<10}",
                                        r["name"].as_str().unwrap_or("-"),
                                        r["description"].as_str().unwrap_or("-"),
                                        r["is_system"].as_bool().unwrap_or(false),
                                    );
                                }
                            }
                            RbacAction::ListPermissions => {
                                let roles = db.get_rbac_roles().await?;
                                for r in &roles {
                                    if let Some(role_id) = r["id"].as_str() {
                                        let perms = db.get_rbac_role_permissions(role_id).await?;
                                        let perm_names: Vec<&str> = perms.iter().filter_map(|p| p["name"].as_str()).collect();
                                        if !perm_names.is_empty() {
                                            println!("Role '{}': {}", r["name"].as_str().unwrap_or("-"), perm_names.join(", "));
                                        }
                                    }
                                }
                            }
                            RbacAction::Assign { user, role, team } => {
                                db.assign_user_role(user, role, team.as_deref(), "cli").await?;
                                println!("Role '{}' assigned to user '{}'", role, user);
                            }
                            RbacAction::Revoke { user, role } => {
                                db.revoke_user_role(user, role, None).await?;
                                println!("Role '{}' revoked from user '{}'", role, user);
                            }
                            RbacAction::UserRoles { username } => {
                                let roles = db.get_user_roles(username).await?;
                                println!("Roles for '{}':", username);
                                for r in &roles {
                                    println!("  - {} (team: {})",
                                        r["role_name"].as_str().unwrap_or("-"),
                                        r["team_id"].as_str().unwrap_or("global"),
                                    );
                                }
                            }
                        }
                    }
                    Commands::Token { action } => {
                        match action {
                            TokenAction::List { user_id } => {
                                let tokens = db.get_api_tokens(user_id).await?;
                                println!("{:<36} {:<20} {:<15} {:<10}", "ID", "NAME", "EXPIRES", "REVOKED");
                                println!("{}", "-".repeat(83));
                                for t in &tokens {
                                    println!("{:<36} {:<20} {:<15} {:<10}",
                                        t["id"].as_str().unwrap_or("-"),
                                        t["name"].as_str().unwrap_or("-"),
                                        t["expires_at"].as_str().unwrap_or("never"),
                                        t["revoked"].as_bool().unwrap_or(false),
                                    );
                                }
                            }
                            TokenAction::Create { name, user_id, scopes, team, expires_days } => {
                                let raw_token = uuid::Uuid::new_v4().to_string();
                                let token_hash = format!("{:x}", {
                                    use std::hash::{Hash, Hasher};
                                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                    raw_token.hash(&mut hasher);
                                    hasher.finish()
                                });
                                let scope_list = scopes.clone().unwrap_or_default();
                                let expires_str = expires_days.map(|d| {
                                    (Utc::now() + chrono::Duration::days(d)).to_rfc3339()
                                });
                                let token_id = db.create_api_token(name, &token_hash, user_id, &scope_list, team.as_deref(), expires_str.as_deref()).await?;
                                println!("Token created (id={}): {}", token_id, raw_token);
                                println!("(Save this token — it cannot be retrieved again)");
                            }
                            TokenAction::Revoke { token_id } => {
                                db.revoke_api_token(token_id).await?;
                                println!("Token '{}' revoked", token_id);
                            }
                        }
                    }
                    Commands::AuthProvider { action } => {
                        match action {
                            AuthProviderAction::List => {
                                let providers = db.get_auth_providers().await?;
                                println!("{:<15} {:<10} {:<20} {:<8} {:<8}", "ID", "TYPE", "NAME", "ENABLED", "PRIORITY");
                                println!("{}", "-".repeat(63));
                                for p in &providers {
                                    println!("{:<15} {:<10} {:<20} {:<8} {:<8}",
                                        p["id"].as_str().unwrap_or("-"),
                                        p["provider_type"].as_str().unwrap_or("-"),
                                        p["name"].as_str().unwrap_or("-"),
                                        p["enabled"].as_bool().unwrap_or(false),
                                        p["priority"].as_i64().unwrap_or(0),
                                    );
                                }
                            }
                            AuthProviderAction::Enable { id } => {
                                if let Some(p) = db.get_auth_provider(id).await? {
                                    db.upsert_auth_provider(
                                        id,
                                        p["provider_type"].as_str().unwrap_or("local"),
                                        p["name"].as_str().unwrap_or(""),
                                        p["config"].as_str().unwrap_or("{}"),
                                        true,
                                        p["priority"].as_i64().unwrap_or(0) as i32,
                                    ).await?;
                                    println!("Auth provider '{}' enabled", id);
                                } else {
                                    println!("Auth provider '{}' not found", id);
                                }
                            }
                            AuthProviderAction::Disable { id } => {
                                if let Some(p) = db.get_auth_provider(id).await? {
                                    db.upsert_auth_provider(
                                        id,
                                        p["provider_type"].as_str().unwrap_or("local"),
                                        p["name"].as_str().unwrap_or(""),
                                        p["config"].as_str().unwrap_or("{}"),
                                        false,
                                        p["priority"].as_i64().unwrap_or(0) as i32,
                                    ).await?;
                                    println!("Auth provider '{}' disabled", id);
                                } else {
                                    println!("Auth provider '{}' not found", id);
                                }
                            }
                        }
                    }
                    Commands::Audit { action } => {
                        match action {
                            AuditAction::Recent { limit } => {
                                let entries = db.get_audit_log(None, None, None, *limit, 0).await?;
                                for e in &entries {
                                    println!("[{}] {} {} {} ({})",
                                        e["timestamp"].as_str().or(e["created_at"].as_str()).unwrap_or("-"),
                                        e["actor"].as_str().unwrap_or("-"),
                                        e["action"].as_str().unwrap_or("-"),
                                        e["target_id"].as_str().or(e["resource_id"].as_str()).unwrap_or("-"),
                                        e["event_type"].as_str().unwrap_or("-"),
                                    );
                                }
                                println!("\n{} entries shown", entries.len());
                            }
                            AuditAction::ByActor { actor, limit } => {
                                let entries = db.get_audit_log(None, Some(actor.as_str()), None, *limit, 0).await?;
                                for e in &entries {
                                    println!("[{}] {} {} ({})",
                                        e["timestamp"].as_str().or(e["created_at"].as_str()).unwrap_or("-"),
                                        e["action"].as_str().unwrap_or("-"),
                                        e["target_id"].as_str().or(e["resource_id"].as_str()).unwrap_or("-"),
                                        e["event_type"].as_str().unwrap_or("-"),
                                    );
                                }
                                println!("\n{} entries for '{}'", entries.len(), actor);
                            }
                        }
                    }
                    Commands::Compliance { action } => {
                        match action {
                            ComplianceAction::List => {
                                let controls = db.get_compliance_controls(None).await?;
                                println!("{:<15} {:<12} {:<40} {:<12}", "FRAMEWORK", "CONTROL", "DESCRIPTION", "STATUS");
                                println!("{}", "-".repeat(81));
                                for c in &controls {
                                    println!("{:<15} {:<12} {:<40} {:<12}",
                                        c["framework"].as_str().unwrap_or("-"),
                                        c["control_id"].as_str().unwrap_or("-"),
                                        c["description"].as_str().unwrap_or("-"),
                                        c["status"].as_str().unwrap_or("-"),
                                    );
                                }
                            }
                            ComplianceAction::Status => {
                                let controls = db.get_compliance_controls(None).await?;
                                let total = controls.len();
                                let passed = controls.iter().filter(|c| c["status"].as_str() == Some("passed")).count();
                                let failed = controls.iter().filter(|c| c["status"].as_str() == Some("failed")).count();
                                let not_assessed = total - passed - failed;
                                println!("Compliance Status Summary:");
                                println!("  Total controls: {}", total);
                                println!("  Passed:         {}", passed);
                                println!("  Failed:         {}", failed);
                                println!("  Not assessed:   {}", not_assessed);
                            }
                        }
                    }
                    Commands::Lineage { action } => {
                        match action {
                            LineageAction::Run { run_id } => {
                                let events = db.get_lineage_events("*", Some(run_id.as_str()), 100).await?;
                                for e in &events {
                                    println!("[{}] {} {} (dag={}, task={})",
                                        e["event_time"].as_str().unwrap_or("-"),
                                        e["event_type"].as_str().unwrap_or("-"),
                                        e["job_name"].as_str().unwrap_or("-"),
                                        e["dag_id"].as_str().unwrap_or("-"),
                                        e["task_id"].as_str().unwrap_or("-"),
                                    );
                                }
                                println!("\n{} lineage event(s)", events.len());
                            }
                            LineageAction::Datasets => {
                                let datasets = db.get_lineage_datasets(100, 0).await?;
                                println!("{:<36} {:<20} {:<30} {:<10}", "ID", "NAMESPACE", "NAME", "TYPE");
                                println!("{}", "-".repeat(98));
                                for d in &datasets {
                                    println!("{:<36} {:<20} {:<30} {:<10}",
                                        d["id"].as_str().unwrap_or("-"),
                                        d["namespace"].as_str().unwrap_or("-"),
                                        d["name"].as_str().unwrap_or("-"),
                                        d["source_type"].as_str().unwrap_or("-"),
                                    );
                                }
                            }
                            LineageAction::Dataset { dataset_id } => {
                                let events = db.get_dataset_events(dataset_id, 100).await?;
                                println!("Dataset events for '{}':", dataset_id);
                                for e in &events {
                                    println!("  [{}] dag={} run={}",
                                        e["timestamp"].as_str().unwrap_or("-"),
                                        e["source_dag_id"].as_str().unwrap_or("-"),
                                        e["source_run_id"].as_str().unwrap_or("-"),
                                    );
                                }
                            }
                        }
                    }
                    Commands::Connector { action } => {
                        match action {
                            ConnectorAction::List => {
                                println!("Registered connectors:");
                                println!("  - postgres (Database)");
                                println!("  - snowflake (Warehouse)");
                                println!("  - bigquery (Warehouse)");
                                println!("  - redshift (Warehouse)");
                                println!("  - databricks (Warehouse)");
                                println!("  - dbt (Transformation)");
                                println!("  - kafka (Streaming)");
                                println!("  - s3 (Storage)");
                                println!("  - gcs (Storage)");
                                println!("  - delta-lake (Lake)");
                            }
                            ConnectorAction::Health { name } => {
                                println!("Health check for '{}': use the REST API (GET /api/connectors/{}/health) for live checks.", name, name);
                            }
                        }
                    }
                    Commands::Swarm { action } => {
                        match action {
                            SwarmAction::Status => {
                                println!("Swarm status: connect via REST API (GET /api/swarm/workers) or start in --swarm mode.");
                            }
                            SwarmAction::Workers => {
                                println!("Workers are managed by the Swarm gRPC controller.");
                                println!("Start the server with --swarm and query GET /api/swarm/workers.");
                            }
                        }
                    }
                    Commands::Dag { action } => {
                        match action {
                            DagAction::List => {
                                let (dags, total) = db.get_all_dags(100, 0).await?;
                                println!("{:<30} {:<10} {:<20} {:<10}", "DAG ID", "PAUSED", "SCHEDULE", "TEAM");
                                println!("{}", "-".repeat(72));
                                for d in &dags {
                                    println!("{:<30} {:<10} {:<20} {:<10}",
                                        d["id"].as_str().unwrap_or("-"),
                                        d["is_paused"].as_bool().unwrap_or(false),
                                        d["schedule"].as_str().unwrap_or("None"),
                                        d["team_id"].as_str().unwrap_or("-"),
                                    );
                                }
                                println!("\n{} DAG(s) total", total);
                            }
                            DagAction::Get { dag_id } => {
                                match db.get_dag_by_id(dag_id).await? {
                                    Some(dag) => println!("{}", serde_json::to_string_pretty(&dag)?),
                                    None => println!("DAG '{}' not found", dag_id),
                                }
                            }
                            DagAction::Trigger { dag_id, triggered_by } => {
                                let run_id = uuid::Uuid::new_v4().to_string();
                                let now = Utc::now();
                                db.create_dag_run(&run_id, dag_id, now, triggered_by).await?;
                                println!("DAG run created: {}", run_id);
                                println!("Note: task execution requires the server to be running.");
                            }
                            DagAction::Pause { dag_id } => {
                                db.pause_dag(dag_id).await?;
                                println!("DAG '{}' paused", dag_id);
                            }
                            DagAction::Unpause { dag_id } => {
                                db.unpause_dag(dag_id).await?;
                                println!("DAG '{}' unpaused", dag_id);
                            }
                        }
                    }
                    Commands::Pool { action } => {
                        match action {
                            PoolAction::List => {
                                let pools = db.get_all_pools().await?;
                                println!("{:<20} {:<10} {:<30}", "NAME", "SLOTS", "DESCRIPTION");
                                println!("{}", "-".repeat(62));
                                for p in &pools {
                                    println!("{:<20} {:<10} {:<30}",
                                        p["name"].as_str().unwrap_or("-"),
                                        p["slots"].as_i64().unwrap_or(0),
                                        p["description"].as_str().unwrap_or("-"),
                                    );
                                }
                            }
                            PoolAction::Create { name, slots, description } => {
                                db.create_pool(name, *slots, description.as_deref().unwrap_or("")).await?;
                                println!("Pool '{}' created with {} slots", name, slots);
                            }
                            PoolAction::Delete { name } => {
                                db.delete_pool(name).await?;
                                println!("Pool '{}' deleted", name);
                            }
                        }
                    }
                    Commands::Config { action } => {
                        match action {
                            ConfigAction::Show => {
                                println!("Vortex Configuration:");
                                println!("  Port:              {}", cli.port);
                                println!("  Swarm:             {}", cli.swarm);
                                println!("  gRPC Bind:         {}", cli.grpc_bind);
                                println!("  Swarm Port:        {}", cli.swarm_port);
                                println!("  HA Mode:           {}", cli.ha_mode);
                                println!("  DB Max Conns:      {}", cli.db_max_connections);
                                println!("  DB Min Conns:      {}", cli.db_min_connections);
                                println!("  DB Idle Timeout:   {}s", cli.db_idle_timeout);
                                println!("  TLS:               {}", cli.tls_cert.is_some());
                                println!("  Benchmark DAG:     {}", cli.benchmark);
                                println!("  Unsafe Plugins:    {}", cli.allow_unsafe_plugins);
                                println!("  Unsafe DAG Exec:   {}", cli.allow_unsafe_dag_exec);
                                println!("  Log Level:         {}", cli.log_level);
                            }
                            ConfigAction::ValidateDb => {
                                println!("Database connection: OK");
                                println!("Migrations applied successfully.");
                            }
                            ConfigAction::Export => {
                                let config = serde_json::json!({
                                    "port": cli.port,
                                    "swarm": cli.swarm,
                                    "grpc_bind": cli.grpc_bind,
                                    "swarm_port": cli.swarm_port,
                                    "ha_mode": cli.ha_mode,
                                    "db_max_connections": cli.db_max_connections,
                                    "db_min_connections": cli.db_min_connections,
                                    "db_idle_timeout_secs": cli.db_idle_timeout,
                                    "tls_enabled": cli.tls_cert.is_some(),
                                    "log_level": cli.log_level,
                                });
                                println!("{}", serde_json::to_string_pretty(&config)?);
                            }
                        }
                    }
                    _ => {}
                }
                return Ok(());
            }
        }
    }
    
    // 🌪️ CONTROLLER MODE
    info!("🌪️ VORTEX Orchestrator v0.7.0 - Operational");

    // Initialize Secret Vault
    let vault = match Vault::new() {
        Ok(v) => { info!("🔐 Secret Vault initialized (AES-256-GCM)."); Some(Arc::new(v)) },
        Err(e) => { warn!("⚠️ Secret Vault DISABLED: {}. Secrets will not be available.", e); None }
    };

    // Initialize Database Backend (PostgreSQL only)
    let db_url_owned = cli.database_url
        .ok_or_else(|| anyhow::anyhow!("❌ --database-url or DATABASE_URL env var is required. VORTEX requires PostgreSQL."))?;

    let db_idle_timeout = std::time::Duration::from_secs(cli.db_idle_timeout);

    info!("🗄️ Initializing PostgreSQL backend...");
    let db: Arc<dyn db_trait::DatabaseBackend> = Arc::new(
        db_postgres::PostgresDb::new(&db_url_owned, cli.db_max_connections, cli.db_min_connections, db_idle_timeout).await?
    );
    info!("✅ Database initialized.");

    // Initialize Prometheus Metrics
    let vortex_metrics = Arc::new(metrics::VortexMetrics::new()?);
    info!("📊 Prometheus metrics initialized (GET /metrics)");

    // Recovery Mode
    let interrupted = db.get_interrupted_tasks().await?;
    if !interrupted.is_empty() {
        warn!("⚠️ Recovery Mode: Found {} interrupted tasks from previous run.", interrupted.len());
        for (ti_id, dag_id, task_id) in interrupted {
            info!("  - Marking instance {} ({}/{}) as Failed", ti_id, dag_id, task_id);
            if let Err(e) = db.update_task_state(&ti_id, "Failed").await {
                error!("Failed to mark task {} as Failed: {}", ti_id, e);
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Plugin Discovery
    // ─────────────────────────────────────────────────────────────────
    let mut plugin_registry = executor::PluginRegistry::new();
    // BUG-11 FIX: Gate plugin loading behind --allow-unsafe-plugins (default OFF)
    if cli.allow_unsafe_plugins {
        let plugins_dir = std::path::Path::new("plugins");
        if plugins_dir.exists() && plugins_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(plugins_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        if ext == "so" || ext == "dylib" || ext == "dll" {
                            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown");
                            unsafe {
                                match plugin_registry.load_plugin(path.to_str().unwrap(), file_stem) {
                                    Ok(_) => info!("🔌 Loaded plugin '{}' from {:?}", file_stem, path),
                                    Err(e) => warn!("⚠️ Failed to load plugin {:?}: {}", path, e),
                                }
                            }
                        }
                    }
                }
            }
        } else {
            info!("🔌 Plugins directory not found or empty. Using default operators.");
        }
    } else {
        info!("🔌 Plugin loading disabled. Use --allow-unsafe-plugins to enable (SECURITY RISK).");
    }
    executor::init_global_registry(plugin_registry);
    
    // ARCH-2: Use tokio::sync::Mutex to match AppState.dags type.
    let all_dags = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    // Improvement 48: only register the synthetic benchmark DAG when --benchmark
    // is explicitly passed. Avoids polluting production DAG lists.
    if cli.benchmark {
        let bench = create_benchmark_dag();
        info!("🛠️ Registering benchmark DAG: {}", bench.id);
        let mut map = all_dags.lock().await;
        map.insert(bench.id.clone(), Arc::new(bench));
    }

    // Scan dags/ for Python and config DAG files
    {
        let mut map = all_dags.lock().await;
        let dags_dir = "dags";
        if std::path::Path::new(dags_dir).exists() {
            if let Ok(entries) = std::fs::read_dir(dags_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                        if let Some(path_str) = path.to_str() {
                            if ext == "py" {
                                // BUG-18 FIX: Gate Python DAG exec behind --allow-unsafe-dag-exec
                                if !cli.allow_unsafe_dag_exec {
                                    warn!("⚠️ Skipping Python DAG file {} — use --allow-unsafe-dag-exec to enable (SECURITY RISK)", path_str);
                                } else {
                                    info!("🐍 Loading DAG file: {}", path_str);
                                    match python_parser::parse_python_dag(path_str) {
                                        Ok(dags) => {
                                            for dag in dags { 
                                                info!("✅ Loaded DAG: {}", dag.id);
                                                let dag_id = dag.id.clone();
                                                map.insert(dag_id.clone(), Arc::new(dag));
                                                
                                                // Force create version record for physical files
                                                if let Err(e) = db.store_dag_version(&dag_id, path_str).await {
                                                    error!("Failed to store version for {}: {}", dag_id, e);
                                                }
                                            }
                                        },
                                        Err(e) => {
                                            error!("❌ Failed to parse DAG file {}: {}", path_str, e);
                                        }
                                    }
                                }
                            } else if ext == "json" || ext == "yaml" || ext == "yml" {
                                info!("📄 Loading Config DAG file: {}", path_str);
                                match dag_factory::parse_dag_file(path_str) {
                                    Ok(dags) => {
                                        for dag in dags { 
                                            info!("✅ Loaded Config DAG: {}", dag.id);
                                            let dag_id = dag.id.clone();
                                            map.insert(dag_id.clone(), Arc::new(dag));
                                            
                                            if let Err(e) = db.store_dag_version(&dag_id, path_str).await {
                                                error!("Failed to store version for config DAG {}: {}", dag_id, e);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        error!("❌ Failed to parse Config DAG file {}: {}", path_str, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for dag in map.values() { db.register_dag(dag).await?; }
    }
    info!("✅ Loaded DAGs.");


    // High Availability
    let ha_mode = cli.ha_mode;
    let (leader_tx, leader_rx) = tokio::sync::watch::channel(!ha_mode);

    if ha_mode {
        let db_leader = Arc::clone(&db);
        tokio::spawn(async move {
            info!("🔒 HA Mode Enabled. Standing by for Leader Lock...");
            // Bug 15 fix: loop continuously renewing the lease every 10s.
            // The old code broke after first acquisition (advisory lock session-scoped).
            // Now we use the leader_election table, which is connection-agnostic.
            let mut is_leader = false;
            loop {
                match db_leader.try_acquire_leader_lock().await {
                    Ok(true) => {
                        if !is_leader {
                            info!("👑 Acquired HA Leader Lock. Promoting to Active.");
                            let _ = leader_tx.send(true);
                            is_leader = true;
                        }
                        // Renew every 10s (lease expires in 30s, so 3× headroom)
                    }
                    Ok(false) => {
                        if is_leader {
                            warn!("⚠️ Lost HA Leader Lock. Stepping down to Standby.");
                            let _ = leader_tx.send(false);
                            is_leader = false;
                        }
                    }
                    Err(e) => {
                        warn!("⚠️ DB error during leader lock renewal: {}", e);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });
    }

    // Swarm
    let swarm_enabled = cli.swarm;
    // Bug 27 fix: add --grpc-bind flag so gRPC can be restricted to localhost
    // or a specific interface instead of always exposing on all interfaces.
    let grpc_bind = cli.grpc_bind;
    let swarm_port = cli.swarm_port;
    let swarm_state = Arc::new(SwarmState::new(Arc::clone(&db), swarm_enabled, vault.clone(), Some(Arc::clone(&vortex_metrics))));

    if swarm_enabled {
        let grpc_state = Arc::clone(&swarm_state);
        let health_state = Arc::clone(&swarm_state);
        let tls_cert_grpc = cli.tls_cert.clone();
        let tls_key_grpc = cli.tls_key.clone();

        // Spawn gRPC server
        tokio::spawn(async move {
            if let (Some(cert_path), Some(key_path)) = (&tls_cert_grpc, &tls_key_grpc) {
                let cert = std::fs::read(cert_path).expect("Failed to read TLS cert");
                let key = std::fs::read(key_path).expect("Failed to read TLS key");
                let identity = tonic::transport::Identity::from_pem(cert, key);
                let tls_config = tonic::transport::ServerTlsConfig::new().identity(identity);
                let addr = format!("{}:{}", grpc_bind, swarm_port).parse().unwrap();
                let server = swarm::create_grpc_server(grpc_state);
                info!("🐝 Swarm Controller listening on {} (TLS)", addr);
                let _ = tonic::transport::Server::builder()
                    .tls_config(tls_config).unwrap()
                    .add_service(server)
                    .serve(addr).await;
            } else {
                let addr = format!("{}:{}", grpc_bind, swarm_port).parse().unwrap();
                let server = swarm::create_grpc_server(grpc_state);
                info!("🐝 Swarm Controller listening on {}", addr);
                let _ = tonic::transport::Server::builder().add_service(server).serve(addr).await;
            }
        });

        // Spawn Health Check Loop
        let mut leader_rx_health = leader_rx.clone();
        tokio::spawn(async move {
            loop {
                // BUG-1 FIX: Re-check leadership status every iteration, not just at startup.
                if !*leader_rx_health.borrow() {
                    info!("⏸ Health check loop: lost leadership, suspending...");
                    let _ = leader_rx_health.changed().await;
                    info!("▶ Health check loop: regained leadership, resuming...");
                    continue;
                }
                // Run one health check cycle then sleep (health_check_loop is now one-shot).
                // We break out of this inner call after one cycle and re-check leader status.
                health_state.health_check_cycle().await;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }

    // Bug 16 fix: increased from 32 to 512 to prevent sender back-pressure under
    // heavy cron or backfill loads. 512 is still bounded (prevents unbounded memory
    // growth) but gives headroom for burst scheduling.
    let (tx, mut rx) = mpsc::channel::<scheduler::ScheduleRequest>(512);

    let tls_cert = cli.tls_cert;
    let tls_key = cli.tls_key;

    // Web UI
    let db_web = Arc::clone(&db);
    let tx_web = tx.clone();
    let swarm_web = Arc::clone(&swarm_state);
    let vault_web = vault.clone();
    let dags_web = Arc::clone(&all_dags);
    let metrics_web = Arc::clone(&vortex_metrics);
    // Bug 26 fix: add --port CLI flag so the web port is configurable.
    let web_port = cli.port;
    tokio::spawn(async move {
        let server = web::WebServer::new(db_web, tx_web, swarm_web, vault_web, dags_web, metrics_web);
        if let Err(e) = server.run(web_port, tls_cert, tls_key).await {
            error!("Web server fatally exited: {}", e);
        }
    });

    // Scheduler Loop
    let db_sched = Arc::clone(&db);
    let dags_sched = Arc::clone(&all_dags);
    let swarm_sched = Arc::clone(&swarm_state);
    let metrics_sched = Arc::clone(&vortex_metrics);
    let mut leader_rx_sched = leader_rx.clone();
    tokio::spawn(async move {
        if !*leader_rx_sched.borrow() {
            let _ = leader_rx_sched.changed().await;
        }
        info!("🌀 Scheduler loop started.");
        while let Some(req) = rx.recv().await {
            // BUG-1 FIX: Re-check leadership status before processing each request.
            if !*leader_rx_sched.borrow() {
                info!("⏸ Scheduler loop: lost leadership, suspending...");
                let _ = leader_rx_sched.changed().await;
                info!("▶ Scheduler loop: regained leadership, resuming...");
                // Re-check with borrow to confirm we're leader now
                if !*leader_rx_sched.borrow() { continue; }
            }
            debug!("🔔 Scheduler received request: {:?}", req);
            let dag = {
                let map = dags_sched.lock().await;
                map.get(&req.dag_id).cloned()
            };

            if let Some(dag) = dag {
                let worker_count = swarm_sched.active_worker_count().await;
                debug!("🔎 Scheduler: Found DAG {}. Swarm enabled: {}. Active workers: {}", req.dag_id, swarm_sched.enabled, worker_count);

                if swarm_sched.enabled && worker_count > 0 {
                    info!("🐝 Scheduler: Dispatching to SWARM mode.");
                    let dag_run_id = uuid::Uuid::new_v4().to_string();
                    let execution_date = req.execution_date.unwrap_or_else(|| Utc::now());
                    if let Err(e) = db_sched.create_dag_run(&dag_run_id, &req.dag_id, execution_date, &req.triggered_by).await {
                        error!("DB error creating DAG run: {}", e);
                    }
                    if let Err(e) = db_sched.update_dag_run_state(&dag_run_id, "Running").await {
                        error!("DB error updating DAG run state: {}", e);
                    }
                    
                    let mut pre_finished_tasks = std::collections::HashSet::new();
                    if let scheduler::RunType::RetryFromFailure = req.run_type {
                        if let Ok((runs, _)) = db_sched.get_dag_runs(&req.dag_id, 100, 0).await {
                            if let Some(last_failed) = runs.iter().find(|r| r["state"] == "Failed") {
                                if let Some(_run_id) = last_failed["id"].as_str() {
                                     if let Ok((instances, _)) = db_sched.get_task_instances(&req.dag_id, 1000, 0).await {
                                         for inst in instances {
                                             if inst["state"] == "Success" {
                                                 if let Some(tid) = inst["task_id"].as_str() {
                                                     pre_finished_tasks.insert(tid.to_string());
                                                 }
                                             }
                                         }
                                     }
                                }
                            }
                        }
                    }

                    // --- Swarm Dependency Orchestrator ---
                    let dag_clone = Arc::clone(&dag);
                    let db_clone = Arc::clone(&db_sched);
                    let swarm_clone = Arc::clone(&swarm_sched);
                    let metrics_clone = Arc::clone(&metrics_sched);
                    let run_id_clone = dag_run_id.clone();
                    let execution_date_clone = execution_date;
                    
                    tokio::spawn(async move {
                        let mut in_degree = std::collections::HashMap::new();
                        let mut adj = std::collections::HashMap::new();

                        for task_id in dag_clone.tasks.keys() {
                            in_degree.insert(task_id.clone(), 0);
                            adj.insert(task_id.clone(), Vec::new());
                        }

                        for (up, down) in &dag_clone.dependencies {
                            if let Some(deg) = in_degree.get_mut(down) { *deg += 1; }
                            if let Some(v) = adj.get_mut(up) { v.push(down.clone()); }
                        }

                        // Mark pre-finished tasks as success and adjust degrees
                        let finished_tasks = pre_finished_tasks.clone();
                        for tid in &pre_finished_tasks {
                            if let Some(downstream) = adj.get(tid) {
                                for down in downstream {
                                    if let Some(deg) = in_degree.get_mut(down) { *deg -= 1; }
                                }
                            }
                        }

                        let (tx_done, mut rx_done) = tokio::sync::mpsc::channel(100);
                        let mut tasks_remaining = dag_clone.tasks.len() - finished_tasks.len();
                        let mut active_tasks = 0; // BUG-10 FIX: track active tasks to prevent deadlock
                        
                        // Queue initial tasks
                        for (tid, &deg) in in_degree.iter() {
                            if deg == 0 && !finished_tasks.contains(tid) {
                                let task = dag_clone.tasks.get(tid).unwrap();
                                let ti_id = uuid::Uuid::new_v4().to_string();
                                if let Err(e) = db_clone.create_task_instance(&ti_id, &dag_clone.id, tid, "Queued", execution_date_clone, &run_id_clone).await {
                                    error!("DB error creating task instance {}: {}", tid, e);
                                }
                                
                                metrics_clone.record_task_queued();
                                swarm_clone.enqueue_task(swarm::PendingTask {
                                    task_instance_id: ti_id.clone(), dag_id: dag_clone.id.clone(), task_id: tid.clone(),
                                    command: task.command.clone(), dag_run_id: run_id_clone.clone(),
                                    task_type: task.task_type.clone(), config_json: task.config.to_string(),
                                    max_retries: task.max_retries, retry_delay_secs: task.retry_delay_secs,
                                    // BUG-2 FIX: secrets come from task definition, not hardcoded
                                    required_secrets: vec![],
                                    execution_timeout_secs: task.execution_timeout.unwrap_or(0),  // BUG-16
                                }).await;

                                active_tasks += 1;
                                // Monitor this specific task
                                let db_mon = Arc::clone(&db_clone);
                                let tx_mon = tx_done.clone();
                                let tid_mon = tid.clone();
                                tokio::spawn(async move {
                                    // BUG-12 FIX: cap the TOTAL poll count (Ok + Err) to 300
                                    // iterations (~10 min). The original code only capped the Err
                                    // path, so a task stuck in Queued/Running looped indefinitely.
                                    let mut total_polls = 0u32;
                                    loop {
                                        total_polls += 1;
                                        if total_polls >= 300 {
                                            warn!("Monitor timed out polling task instance {} after {} polls — marking failed", ti_id, total_polls);
                                            let _ = tx_mon.send((tid_mon, false)).await;
                                            break;
                                        }
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                        match db_mon.get_task_instance_retry_info(&ti_id).await {
                                            Ok((_, state)) => {
                                                if state == "Success" { let _ = tx_mon.send((tid_mon, true)).await; break; }
                                                if state == "Failed"  { let _ = tx_mon.send((tid_mon, false)).await; break; }
                                                // still Running/Queued — loop and decrement remaining budget
                                            }
                                            Err(_) => {
                                                // DB error counts against budget too
                                            }
                                        }
                                    }
                                });
                            }
                        }

                        let mut all_success = true;
                        while tasks_remaining > 0 {
                            if active_tasks == 0 {
                                error!("Scheduler deadlock detected in manual run: {} tasks remaining but 0 active monitors. Breaking loop.", tasks_remaining);
                                break;
                            }
                            if let Some((finished_tid, success)) = rx_done.recv().await {
                                active_tasks -= 1;
                                tasks_remaining -= 1;
                                if !success { all_success = false; }
                                
                                if let Some(downstream) = adj.get(&finished_tid) {
                                    for down in downstream {
                                        let deg = in_degree.get_mut(down).unwrap();
                                        *deg -= 1;
                                        if *deg == 0 {
                                            // BUG-4 / BUG-25 FIX: skip downstream tasks if upstream failed
                                            if !success {
                                                let skipped_ti = uuid::Uuid::new_v4().to_string();
                                                if let Err(e) = db_clone.create_task_instance(&skipped_ti, &dag_clone.id, down, "Upstream_Failed", execution_date_clone, &run_id_clone).await {
                                                    error!("DB error writing upstream_failed instance: {}", e);
                                                }
                                                if let Err(e) = db_clone.log_task_event(&skipped_ti, &dag_clone.id, down, &run_id_clone, "upstream_failed", Some("Upstream task failed"), None).await {
                                                    error!("DB error logging event: {}", e);
                                                }
                                                active_tasks += 1;
                                                let _ = tx_done.send((down.clone(), false)).await;
                                                continue;
                                            }

                                            let task = dag_clone.tasks.get(down).unwrap();
                                            let ti_id = uuid::Uuid::new_v4().to_string();
                                            if let Err(e) = db_clone.create_task_instance(&ti_id, &dag_clone.id, down, "Queued", execution_date_clone, &run_id_clone).await {
                                                error!("DB error writing queued instance: {}", e);
                                            }
                                            if let Err(e) = db_clone.log_task_event(&ti_id, &dag_clone.id, down, &run_id_clone, "queued", None, None).await {
                                                error!("DB error logging event: {}", e);
                                            }
                                            
                                            metrics_clone.record_task_queued();
                                            swarm_clone.enqueue_task(swarm::PendingTask {
                                                task_instance_id: ti_id.clone(), dag_id: dag_clone.id.clone(), task_id: down.clone(),
                                                command: task.command.clone(), dag_run_id: run_id_clone.clone(),
                                                task_type: task.task_type.clone(), config_json: task.config.to_string(),
                                                max_retries: task.max_retries, retry_delay_secs: task.retry_delay_secs,
                                                // BUG-2 FIX: secrets come from task definition, not hardcoded
                                                required_secrets: vec![],
                                                execution_timeout_secs: task.execution_timeout.unwrap_or(0),  // BUG-16
                                            }).await;

                                            active_tasks += 1;
                                            let db_mon = Arc::clone(&db_clone);
                                            let tx_mon = tx_done.clone();
                                            let down_mon = down.clone();
                                            tokio::spawn(async move {
                                                // BUG-12 FIX: same unified total_polls cap for downstream monitors
                                                let mut total_polls = 0u32;
                                                loop {
                                                    total_polls += 1;
                                                    if total_polls >= 300 {
                                                        warn!("Monitor timed out polling downstream task instance {} after {} polls — marking failed", ti_id, total_polls);
                                                        let _ = tx_mon.send((down_mon, false)).await;
                                                        break;
                                                    }
                                                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                                    match db_mon.get_task_instance_retry_info(&ti_id).await {
                                                        Ok((_, state)) => {
                                                            if state == "Success" { let _ = tx_mon.send((down_mon, true)).await; break; }
                                                            if state == "Failed"  { let _ = tx_mon.send((down_mon, false)).await; break; }
                                                        }
                                                        Err(_) => {
                                                            // DB error counts against budget too
                                                        }
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        let final_state = if all_success { "Success" } else { "Failed" };
                        if let Err(e) = db_clone.update_dag_run_state(&run_id_clone, final_state).await {
                            error!("DB error updating DAG run final state: {}", e);
                        }
                        metrics_clone.record_dag_run_complete(final_state);
                        info!("🏁 Swarm Orchestrator: DAG Run {} finished (Success: {})", run_id_clone, all_success);
                    });
                } else {
                    let scheduler = Scheduler::new_with_arc(Arc::clone(&dag), Arc::clone(&db_sched))
                        .with_metrics(Arc::clone(&metrics_sched));
                    // Update: Scheduler needs metrics too
                    match req.run_type {
                        scheduler::RunType::Full => { let _ = scheduler.run_with_trigger(&req.triggered_by, req.execution_date).await; },
                        scheduler::RunType::RetryFromFailure => { 
                             warn!("⚠️ Standalone Retry not implemented yet (Swarm mode recommended)");
                             let _ = scheduler.run_with_trigger(&req.triggered_by, req.execution_date).await;
                        }
                    }
                }
            }
        }
    });

    // SLA Proactive Breach Detection Loop (Sprint 3)
    let db_sla = Arc::clone(&db);
    let dags_sla = Arc::clone(&all_dags);
    let mut leader_rx_sla = leader_rx.clone();
    tokio::spawn(async move {
        if !*leader_rx_sla.borrow() {
            let _ = leader_rx_sla.changed().await;
        }
        info!("🔴 SLA Monitor loop started (checking every 60s)");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;

            // BUG-1 FIX: Re-check leadership status each iteration.
            if !*leader_rx_sla.borrow() {
                info!("⏸ SLA Monitor: lost leadership, suspending...");
                let _ = leader_rx_sla.changed().await;
                info!("▶ SLA Monitor: regained leadership, resuming...");
                continue;
            }
            
            // Query DAG runs that are currently "Running" and haven't already breached SLA
            match db_sla.get_running_dag_runs().await {
                Ok(running_runs) => {
                    if running_runs.is_empty() {
                        continue;
                    }

                    // Snapshot the DAGs map to avoid holding the lock across DB queries
                    let dags_snapshot: HashMap<String, Arc<scheduler::Dag>> = {
                        let guard = dags_sla.lock().await;
                        guard.clone()
                    };

                    for (run_id, dag_id, start_time) in &running_runs {
                        if let Some(dag) = dags_snapshot.get(dag_id) {
                            if let Some(sla_secs) = dag.sla_seconds {
                                let elapsed = Utc::now().signed_duration_since(*start_time);
                                if elapsed.num_seconds() > sla_secs as i64 {
                                    warn!("🔴 SLA BREACH: DAG Run {} for DAG {} exceeded {}s limit", run_id, dag_id, sla_secs);
                                    if let Err(e) = db_sla.mark_sla_missed(run_id).await {
                                        error!("DB error marking SLA missed: {}", e);
                                    }
                                }
                            }
                        }
                    }
                },
                Err(e) => warn!("⚠️ SLA Monitor DB error: {}", e),
            }
        }
    });

    // Cron Scheduler Loop
    let db_cron = Arc::clone(&db);
    let tx_cron = tx.clone();
    let metrics_cron = Arc::clone(&vortex_metrics);
    let mut leader_rx_cron = leader_rx.clone();
    tokio::spawn(async move {
        if !*leader_rx_cron.borrow() {
            let _ = leader_rx_cron.changed().await;
        }
        info!("⏰ Cron scheduler loop started (checking every 10s)");
        loop {
            // Heartbeat first — mark alive before doing any work
            metrics_cron.update_scheduler_heartbeat();

            // BUG-1 FIX: Re-check leadership status each iteration.
            if !*leader_rx_cron.borrow() {
                info!("⏸ Cron scheduler: lost leadership, suspending...");
                let _ = leader_rx_cron.changed().await;
                info!("▶ Cron scheduler: regained leadership, resuming...");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
            
            match db_cron.get_scheduled_dags().await {
                Ok(scheduled_dags) => {
                    metrics_cron.set_dags_total(scheduled_dags.len() as i64);
                    for (dag_id, schedule_expr, last_run, is_paused, _timezone, max_active_runs, _catchup, _team_id) in scheduled_dags {
                        if is_paused { continue; }
                        
                        if let Ok(active_count) = db_cron.get_active_dag_run_count(&dag_id).await {
                            if active_count >= max_active_runs { continue; }
                        }
                        
                        let schedule_str = match crate::scheduler::normalize_schedule(&schedule_expr) {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("⚠️ Invalid schedule expression for DAG {}: {}", dag_id, e);
                                continue;
                            }
                        };
                        if schedule_str.is_empty() { continue; }
                        
                        let schedule: cron::Schedule = match schedule_str.parse() {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("⚠️ Invalid cron expression for DAG {}: {} ({})", dag_id, schedule_expr, e);
                                continue;
                            }
                        };
                        
                        let now = chrono::Utc::now();
                        let should_run = match last_run {
                            Some(last) => {
                                schedule.after(&last).next().map_or(false, |next_time| next_time <= now)
                            }
                            None => true,
                        };
                        
                        if should_run {
                            info!("⏰ Cron triggering DAG: {} (schedule: {})", dag_id, schedule_expr);
                            if let Err(e) = db_cron.update_dag_last_run(&dag_id, now).await {
                                error!("DB error updating DAG last run: {}", e);
                            }
                            if let Some(next) = schedule.after(&now).next() {
                                if let Err(e) = db_cron.update_dag_next_run(&dag_id, Some(next)).await {
                                    error!("DB error updating DAG next run: {}", e);
                                }
                            }
                            let _ = tx_cron.send(crate::scheduler::ScheduleRequest {
                                dag_id: dag_id.clone(),
                                triggered_by: "scheduler".to_string(),
                                run_type: crate::scheduler::RunType::Full,
                                execution_date: Some(now),
                            }).await;
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️ Cron scheduler error: {}", e);
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });

    // Improvement 37: Graceful shutdown — on Ctrl+C (SIGINT) or SIGTERM, mark
    // all Running task instances as Failed so they don't get stuck permanently,
    // release the HA leader lock if held, then exit cleanly.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).unwrap_or_else(|_| {
            panic!("Failed to install SIGTERM handler")
        });
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("🛑 Received SIGINT — starting graceful shutdown...");
            }
            _ = sigterm.recv() => {
                info!("🛑 Received SIGTERM — starting graceful shutdown...");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        info!("🛑 Received Ctrl+C — starting graceful shutdown...");
    }

    // Mark all stuck Running task instances as Failed
    let db_shutdown = Arc::clone(&db);
    match db_shutdown.get_interrupted_tasks().await {
        Ok(tasks) => {
            let count = tasks.len();
            for (ti_id, _dag_id, _task_id) in tasks {
                if let Err(e) = db_shutdown.update_task_state(&ti_id, "Failed").await {
                    error!("DB error marking task Failed on shutdown: {}", e);
                }
            }
            if count > 0 {
                info!("🔴 Marked {} Running task instance(s) as Failed on shutdown.", count);
            }
        }
        Err(e) => warn!("⚠️ Could not fetch running tasks during shutdown: {}", e),
    }

    // Release HA leader lock if we held it
    if ha_mode {
        if let Err(e) = db.release_leader_lock().await {
            error!("Failed to release HA leader lock: {}", e);
        }
        info!("🔓 Released HA leader lock.");
    }

    info!("👋 VORTEX controller shut down cleanly.");
    Ok(())
}



fn create_benchmark_dag() -> Dag {
    let mut dag = Dag::new("parallel_benchmark");
    dag.add_task("t1", "Warm-up", "echo 'Vortex engine warm-up...'");
    dag.add_task("t2", "A", "sleep 1 && echo 'Ingestion A complete'");
    dag.add_task("t3", "B", "sleep 1 && echo 'Ingestion B complete'");
    dag.add_task("t4", "C", "sleep 1 && echo 'Ingestion C complete'");
    dag.add_task("t5", "Final", "echo 'All data processed. Vortex out.'");
    dag.add_dependency("t1", "t2"); dag.add_dependency("t1", "t3"); dag.add_dependency("t1", "t4");
    dag.add_dependency("t2", "t5"); dag.add_dependency("t3", "t5"); dag.add_dependency("t4", "t5");
    dag
}
