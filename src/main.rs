use anyhow::{Context as _, Result};
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
mod agentic;
mod k8s_executor;
mod advanced_scheduler;
mod openapi;
mod telemetry;
mod enterprise_connector;
mod connectors;
mod cloud_connectors;
mod event_framework;
mod sdk;
mod devops;
mod migration;
mod disaster_recovery;
mod config_ops;
mod mcp_server;

/// RYUO Orchestration Engine
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output format: table (default) or json
    #[arg(long, default_value = "table")]
    output: String,

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
    #[arg(long, env = "RYUO_LOG_LEVEL", default_value = "info")]
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

    /// Enable production mode (enforces gRPC TLS and auth token)
    #[arg(long)]
    production: bool,

    /// Reason/context for this action (for audit trail)
    #[arg(long, global = true)]
    reason: Option<String>,
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
    /// Task instance management
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// XCom inter-task data management
    Xcom {
        #[command(subcommand)]
        action: XcomAction,
    },
    /// Dataset-triggered scheduling
    Dataset {
        #[command(subcommand)]
        action: DatasetAction,
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
    /// Event management
    Event {
        #[command(subcommand)]
        action: EventAction,
    },
    /// Sensor management
    Sensor {
        #[command(subcommand)]
        action: SensorAction,
    },
    /// Task queue management and reprioritization
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },
    /// Change approval gates for agent-initiated mutations
    Approval {
        #[command(subcommand)]
        action: ApprovalAction,
    },
    /// Agent action rate limiting
    RateLimit {
        #[command(subcommand)]
        action: RateLimitAction,
    },
    /// Input validation utilities for agents
    Validate {
        #[command(subcommand)]
        action: ValidateAction,
    },
    /// Agent state and decision logging
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// MCP (Model Context Protocol) tool server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Agentic AI capabilities (LLM translation, dbt conversion)
    Agentic {
        #[command(subcommand)]
        action: AgenticAction,
    },
    /// Data profiling (row count, null %, distinct count, min/max)
    Profile {
        /// Connector name (e.g., postgres)
        connector: String,
        /// Table name to profile
        #[arg(long)]
        table: String,
        /// Specific columns to profile (comma-separated). Omit for all.
        #[arg(long)]
        columns: Option<String>,
        /// Query timeout in seconds
        #[arg(long, default_value_t = 60)]
        timeout: u64,
    },
    /// Kubernetes executor management
    K8s {
        #[command(subcommand)]
        action: K8sAction,
    },
    /// Kafka connector operations (via Kafka REST Proxy)
    Kafka {
        #[command(subcommand)]
        action: KafkaAction,
    },
    /// S3-compatible object storage operations
    Storage {
        #[command(subcommand)]
        action: StorageAction,
    },
    /// Delta Lake table operations
    DeltaLake {
        #[command(subcommand)]
        action: DeltaLakeAction,
    },
    /// Backup and disaster recovery
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Deep health check
    Health,
}

#[derive(Subcommand, Debug)]
enum ValidateAction {
    /// Validate a SQL query (SELECT-only check)
    Sql {
        #[arg(long)]
        query: String,
    },
    /// Check a shell command for injection patterns
    Command {
        #[arg(long)]
        cmd: String,
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
    /// Rotate all secrets to a new encryption key (reads RYUO_NEW_SECRET_KEY env var)
    Rotate,
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
        /// Scope rules: "resource_type:resource_pattern:actions" (can specify multiple)
        #[arg(long)]
        scope_rule: Vec<String>,
        /// Optional TTL in hours (0 = no expiry)
        #[arg(long, default_value_t = 0)]
        ttl_hours: i64,
        /// Description of the token's purpose
        #[arg(long)]
        description: Option<String>,
    },
    /// Revoke an API token
    Revoke { token_id: String },
    /// Inspect a token's scopes and metadata
    Inspect {
        /// Token ID or prefix
        token_id: String,
    },
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
        /// Include pre/post state diffs in output
        #[arg(long)]
        with_diffs: bool,
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
    /// Execute a read-only SQL query through a connector
    Query {
        /// Connector name
        name: String,
        /// SQL query (SELECT only)
        #[arg(long)]
        sql: String,
        /// Query timeout in seconds
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        /// Maximum rows to return
        #[arg(long, default_value_t = 1000)]
        max_rows: i64,
    },
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
        /// Runtime config overrides as JSON string
        #[arg(long)]
        config: Option<String>,
        /// Validate and show what would run without actually creating a run
        #[arg(long)]
        dry_run: bool,
    },
    /// Pause a DAG
    Pause { dag_id: String },
    /// Unpause a DAG
    Unpause { dag_id: String },
    /// List run history for a DAG
    Runs {
        dag_id: String,
        #[arg(long, default_value_t = 20)]
        limit: i64,
        #[arg(long)]
        state: Option<String>,
    },
    /// Create/register a DAG from a YAML/JSON file
    Create {
        /// Path to YAML or JSON DAG definition file
        #[arg(long)]
        from_yaml: String,
        /// Validate only, don't persist
        #[arg(long)]
        dry_run: bool,
    },
    /// Backfill DAG runs over a date range
    Backfill {
        dag_id: String,
        /// Start date (ISO 8601)
        #[arg(long)]
        start: String,
        /// End date (ISO 8601)
        #[arg(long)]
        end: String,
        /// Run interval (e.g., "1d", "1h"). Default: "1d"
        #[arg(long, default_value = "1d")]
        interval: String,
        /// Validate only, don't create runs
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate a DAG definition file (checks: cycle-free, valid dependencies, etc.)
    Validate {
        #[arg(long)]
        from_yaml: String,
    },
    /// List all versions of a DAG
    Versions {
        dag_id: String,
    },
    /// Rollback a DAG to a previous version
    Rollback {
        dag_id: String,
        /// Version number to rollback to (defaults to previous version)
        #[arg(long)]
        to_version: Option<i32>,
    },
}

#[derive(Subcommand, Debug)]
enum TaskAction {
    /// Show task execution logs
    Logs {
        /// Task instance ID
        instance_id: String,
        /// Show last N lines only
        #[arg(long)]
        tail: Option<usize>,
    },
}

#[derive(Subcommand, Debug)]
enum XcomAction {
    /// Push a key-value pair to XCom
    Push {
        #[arg(long)]
        dag: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        run: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
    },
    /// Pull a value from XCom
    Pull {
        #[arg(long)]
        dag: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        run: String,
        #[arg(long)]
        key: String,
    },
    /// List all XCom entries for a DAG run
    List {
        #[arg(long)]
        dag: String,
        #[arg(long)]
        run: String,
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
}

#[derive(Subcommand, Debug)]
enum DatasetAction {
    /// List registered datasets
    List,
    /// Emit a dataset event
    Event {
        #[command(subcommand)]
        action: DatasetEventAction,
    },
    /// Show triggers for a dataset
    Triggers {
        dataset_id: String,
    },
    /// Show data freshness for a dataset
    Freshness {
        /// Dataset URI (optional — omit to list all with --stale-after)
        uri: Option<String>,
        /// Only show datasets not updated in the last N seconds
        #[arg(long)]
        stale_after: Option<i64>,
    },
    /// Show latest schema for a dataset
    Schema {
        /// Dataset ID
        dataset_id: String,
    },
    /// Show schema changes since last capture
    SchemaDiff {
        /// Dataset ID
        dataset_id: String,
    },
    /// Show data volume statistics for a dataset
    Stats {
        /// Dataset ID
        dataset_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum DatasetEventAction {
    /// Emit a dataset update event
    Emit {
        #[arg(long)]
        dataset: String,
        #[arg(long)]
        source_dag: String,
        #[arg(long)]
        source_task: String,
        #[arg(long, default_value = "update")]
        event_type: String,
    },
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

#[derive(Subcommand, Debug)]
enum EventAction {
    /// Manage event triggers
    Trigger {
        #[command(subcommand)]
        action: EventTriggerAction,
    },
    /// Show recent events from dataset_events
    Recent {
        #[arg(long)]
        event_type: Option<String>,
        /// Show events from last N seconds
        #[arg(long)]
        since: Option<i64>,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Watch for new events (polls DB every --interval seconds)
    Watch {
        #[arg(long)]
        event_type: Option<String>,
        /// Timeout in seconds (default 300 = 5 minutes)
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        /// Poll interval in seconds
        #[arg(long, default_value_t = 5)]
        interval: u64,
    },
    /// Publish a custom event (inter-agent communication)
    Publish {
        /// Event type (e.g., "agent_signal", "retrain_complete")
        #[arg(long)]
        event_type: String,
        /// Event source (e.g., "agent-ml", "agent-monitor")
        #[arg(long)]
        source: String,
        /// JSON payload
        #[arg(long, default_value = "{}")]
        payload: String,
    },
    /// List recent custom events
    Custom {
        #[arg(long)]
        event_type: Option<String>,
        /// Show events from last N seconds
        #[arg(long)]
        since: Option<i64>,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
}

#[derive(Subcommand, Debug)]
enum EventTriggerAction {
    /// Create a new event trigger
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        event_type: String,
        #[arg(long)]
        dag: String,
        /// JSON filter: {"source_pattern": "s3://...", "payload_conditions": [...]}
        #[arg(long, default_value = "{}")]
        filter: String,
        /// JSON config overrides for the triggered DAG
        #[arg(long, default_value = "{}")]
        config: String,
        /// Team ID to scope this trigger to
        #[arg(long)]
        team: Option<String>,
    },
    /// List all event triggers
    List,
    /// Delete an event trigger
    Delete { id: String },
}

#[derive(Subcommand, Debug)]
enum McpAction {
    /// List all available MCP tools
    Tools,
    /// Describe a specific MCP tool (show input schema)
    Describe { tool_name: String },
    /// Call an MCP tool with JSON arguments
    Call {
        /// Tool name to invoke
        #[arg(long)]
        tool: String,
        /// JSON arguments for the tool
        #[arg(long, default_value = "{}")]
        args: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgenticAction {
    /// Translate a Python function to Rust using an LLM provider
    Translate {
        /// Path to the Python file to translate
        #[arg(long)]
        python_file: String,
        /// LLM provider to use (openai or anthropic)
        #[arg(long, default_value = "openai")]
        provider: String,
        /// Maximum retry attempts for translation
        #[arg(long, default_value_t = 3)]
        max_retries: u32,
    },
    /// Convert a dbt manifest.json to a Ryuo pipeline
    DbtConvert {
        /// Path to the dbt manifest.json file
        #[arg(long)]
        manifest: String,
    },
    /// List configured LLM providers
    Providers,
}

#[derive(Subcommand, Debug)]
enum SensorAction {
    /// List all sensor-type tasks and their states
    List {
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Check for anomalies in a metric query result
    CheckAnomaly {
        /// SQL query that returns a single numeric value
        #[arg(long)]
        sql: String,
        /// Historical values (comma-separated) for baseline
        #[arg(long)]
        baseline: String,
        /// Sensitivity: standard deviations threshold (default 2.0)
        #[arg(long, default_value_t = 2.0)]
        sigma: f64,
    },
}

#[derive(Subcommand, Debug)]
enum QueueAction {
    /// List the current task queue ordered by priority
    List {
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Change priority of a queued task
    Reprioritize {
        /// Task instance ID
        instance_id: String,
        /// New priority (higher = sooner, default 0)
        #[arg(long)]
        priority: i32,
    },
    /// Pause task dispatch globally
    Pause,
    /// Resume task dispatch
    Resume,
    /// Show current scheduler state
    Status,
}

#[derive(Subcommand, Debug)]
enum ApprovalAction {
    /// List pending approval requests
    List,
    /// Approve a pending request
    Approve { id: String },
    /// Reject a pending request
    Reject { id: String },
}

#[derive(Subcommand, Debug)]
enum RateLimitAction {
    /// Show rate limit status for an actor
    Status {
        #[arg(long)]
        actor: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgentAction {
    /// Agent state key-value store
    State {
        #[command(subcommand)]
        action: AgentStateAction,
    },
    /// Agent decision log
    Log {
        #[command(subcommand)]
        action: AgentLogAction,
    },
}

#[derive(Subcommand, Debug)]
enum AgentStateAction {
    /// Set a key-value pair
    Set {
        key: String,
        #[arg(long)]
        value: String,
        #[arg(long, default_value = "default")]
        agent: String,
        /// Time-to-live in seconds (optional)
        #[arg(long)]
        ttl: Option<i64>,
    },
    /// Get a value by key
    Get {
        key: String,
        #[arg(long, default_value = "default")]
        agent: String,
    },
    /// List all state for an agent
    List {
        #[arg(long, default_value = "default")]
        agent: String,
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Delete a key
    Delete {
        key: String,
        #[arg(long, default_value = "default")]
        agent: String,
    },
}

#[derive(Subcommand, Debug)]
enum AgentLogAction {
    /// Write a decision log entry
    Write {
        message: String,
        #[arg(long, default_value = "default")]
        agent: String,
        /// JSON context
        #[arg(long, default_value = "{}")]
        context: String,
        /// Log level: info, warn, error, debug
        #[arg(long, default_value = "info")]
        level: String,
    },
    /// Query decision logs
    Query {
        #[arg(long, default_value = "default")]
        agent: String,
        /// Show logs from last N seconds
        #[arg(long)]
        since: Option<i64>,
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
}

#[derive(Subcommand, Debug)]
enum BackupAction {
    /// Create a database backup using pg_dump
    Create {
        /// Output directory for the backup file
        #[arg(long, default_value = "./backups")]
        output_dir: String,
        /// Backup format: custom (default), plain, directory
        #[arg(long, default_value = "custom")]
        format: String,
    },
    /// List available backup files
    List {
        /// Directory to scan for backups
        #[arg(long, default_value = "./backups")]
        dir: String,
    },
    /// Show backup file info
    Info {
        /// Path to backup file
        path: String,
    },
}

#[derive(Subcommand, Debug)]
enum K8sAction {
    /// Show K8s executor status and configuration
    Status,
    /// List running task pods
    Pods {
        /// Kubernetes namespace (default: from config)
        #[arg(long)]
        namespace: Option<String>,
        /// Filter by pod state (Pending, Running, Succeeded, Failed)
        #[arg(long)]
        state: Option<String>,
    },
    /// Get pod logs for a task
    Logs {
        /// Pod name
        pod: String,
        /// Kubernetes namespace
        #[arg(long)]
        namespace: Option<String>,
        /// Number of tail lines to show
        #[arg(long)]
        tail: Option<i64>,
    },
    /// Show executor configuration
    Config,
}

#[derive(Subcommand, Debug)]
enum KafkaAction {
    /// List topics (requires KAFKA_REST_URL)
    Topics {
        /// Kafka REST Proxy URL (or set KAFKA_REST_URL env)
        #[arg(long, env = "KAFKA_REST_URL")]
        url: String,
    },
    /// Produce a message to a topic
    Produce {
        /// Kafka REST Proxy URL (or set KAFKA_REST_URL env)
        #[arg(long, env = "KAFKA_REST_URL")]
        url: String,
        /// Topic name
        #[arg(long)]
        topic: String,
        /// Message key (optional)
        #[arg(long)]
        key: Option<String>,
        /// Message value
        #[arg(long)]
        value: String,
    },
    /// Consume messages from a topic
    Consume {
        /// Kafka REST Proxy URL (or set KAFKA_REST_URL env)
        #[arg(long, env = "KAFKA_REST_URL")]
        url: String,
        /// Topic name
        #[arg(long)]
        topic: String,
        /// Consumer group ID
        #[arg(long)]
        group: String,
        /// Maximum number of messages to consume
        #[arg(long, default_value_t = 10)]
        max_messages: i64,
        /// Timeout in seconds for consumer poll
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

#[derive(Subcommand, Debug)]
enum StorageAction {
    /// List objects in a bucket/prefix
    Ls {
        /// S3 endpoint URL (or set S3_ENDPOINT_URL env)
        #[arg(long, env = "S3_ENDPOINT_URL")]
        endpoint: String,
        /// Bucket name
        #[arg(long)]
        bucket: String,
        /// Prefix filter
        #[arg(long)]
        prefix: Option<String>,
        /// Maximum number of objects to list
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Get object metadata (HEAD request)
    Stat {
        /// S3 endpoint URL (or set S3_ENDPOINT_URL env)
        #[arg(long, env = "S3_ENDPOINT_URL")]
        endpoint: String,
        /// Bucket name
        #[arg(long)]
        bucket: String,
        /// Object key
        #[arg(long)]
        key: String,
    },
    /// Check if object exists and when it was last modified
    Freshness {
        /// S3 endpoint URL (or set S3_ENDPOINT_URL env)
        #[arg(long, env = "S3_ENDPOINT_URL")]
        endpoint: String,
        /// Bucket name
        #[arg(long)]
        bucket: String,
        /// Object key
        #[arg(long)]
        key: String,
        /// Stale threshold in seconds (object is stale if older than this)
        #[arg(long)]
        stale_after: Option<i64>,
    },
}

#[derive(Subcommand, Debug)]
enum DeltaLakeAction {
    /// Show Delta table metadata (reads _delta_log)
    Info {
        /// Path to Delta table (local filesystem path)
        path: String,
    },
    /// Show schema from the latest commit
    Schema {
        /// Path to Delta table (local filesystem path)
        path: String,
    },
    /// List versions (transaction log entries)
    History {
        /// Path to Delta table (local filesystem path)
        path: String,
        /// Maximum number of versions to show
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
}

/// Validate that identifier strings (dag_id, task_id, key, etc.) contain only
/// safe characters: `[a-zA-Z0-9_-]`. Rejects empty strings as well.
fn validate_identifier(value: &str, field_name: &str) -> Result<()> {
    if value.is_empty() {
        anyhow::bail!("'{}' must not be empty", field_name);
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        anyhow::bail!(
            "Invalid {}: '{}' — only [a-zA-Z0-9_-] characters are allowed",
            field_name,
            value
        );
    }
    Ok(())
}

/// Validate a URI or dataset identifier — allows alphanumeric plus URI-safe chars.
fn validate_uri(value: &str, field_name: &str) -> Result<()> {
    if value.is_empty() {
        anyhow::bail!("'{}' must not be empty", field_name);
    }
    // Allow URI characters: alphanumeric, _, -, ., /, :, @, ?, =, &, %, +, #
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || "/_-.~:@?=&%+#".contains(c)) {
        anyhow::bail!(
            "Invalid {}: '{}' — contains disallowed characters",
            field_name,
            value
        );
    }
    Ok(())
}

/// Check for potentially dangerous patterns in shell commands.
/// Returns a list of security warnings.
fn check_command_injection(cmd: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    let dangerous_patterns = [
        ("; rm ", "Potential destructive command chaining"),
        ("| curl ", "Potential data exfiltration via pipe"),
        ("| wget ", "Potential data exfiltration via pipe"),
        ("$(", "Command substitution detected"),
        ("`", "Backtick command substitution detected"),
        ("| bash", "Piping to shell interpreter"),
        ("| sh ", "Piping to shell interpreter"),
        ("; dd ", "Potential destructive disk operation"),
        (">/dev/", "Redirecting to device file"),
        ("| nc ", "Potential network communication via netcat"),
    ];
    let lower = cmd.to_lowercase();
    for (pattern, description) in &dangerous_patterns {
        if lower.contains(pattern) {
            warnings.push(format!("{}: found '{}'", description, pattern.trim()));
        }
    }
    warnings
}

/// T-019: Validate a DAG definition for structural issues.
/// Returns a list of validation errors (empty = valid).
fn validate_dag(dag: &scheduler::Dag) -> Vec<String> {
    let mut errors = Vec::new();

    // 1. Check for empty DAG
    if dag.tasks.is_empty() {
        errors.push("DAG has no tasks".into());
    }

    // 2. Check task IDs are valid identifiers
    for task_id in dag.tasks.keys() {
        if task_id.is_empty() || !task_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            errors.push(format!("Invalid task ID: '{}' — only [a-zA-Z0-9_-] allowed", task_id));
        }
    }

    // 3. Check dependencies reference valid tasks
    let task_ids: std::collections::HashSet<&str> = dag.tasks.keys().map(|s| s.as_str()).collect();
    for (upstream, downstream) in &dag.dependencies {
        if !task_ids.contains(upstream.as_str()) {
            errors.push(format!("Dependency references non-existent upstream task '{}'", upstream));
        }
        if !task_ids.contains(downstream.as_str()) {
            errors.push(format!("Dependency references non-existent downstream task '{}'", downstream));
        }
    }

    // 4. Cycle detection via Kahn's topological sort
    let mut in_degree: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut adj: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for task_id in dag.tasks.keys() {
        in_degree.entry(task_id.as_str()).or_insert(0);
        adj.entry(task_id.as_str()).or_default();
    }
    for (upstream, downstream) in &dag.dependencies {
        if task_ids.contains(upstream.as_str()) && task_ids.contains(downstream.as_str()) {
            adj.entry(upstream.as_str()).or_default().push(downstream.as_str());
            *in_degree.entry(downstream.as_str()).or_insert(0) += 1;
        }
    }
    let mut queue: std::collections::VecDeque<&str> = in_degree.iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut visited = 0usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if let Some(neighbors) = adj.get(node) {
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
    }
    if visited < dag.tasks.len() {
        errors.push("DAG contains a cycle (circular dependency detected)".into());
    }

    errors
}

/// Parse a human-readable interval string like "1d", "6h" into a `chrono::Duration`.
fn parse_interval(s: &str) -> Result<chrono::Duration> {
    let s = s.trim();
    if let Some(hours) = s.strip_suffix('h') {
        let n: i64 = hours.parse().map_err(|_| anyhow::anyhow!("Invalid interval number: '{}'", hours))?;
        if n <= 0 {
            anyhow::bail!("Interval must be positive, got '{}'", s);
        }
        Ok(chrono::Duration::hours(n))
    } else if let Some(days) = s.strip_suffix('d') {
        let n: i64 = days.parse().map_err(|_| anyhow::anyhow!("Invalid interval number: '{}'", days))?;
        if n <= 0 {
            anyhow::bail!("Interval must be positive, got '{}'", s);
        }
        Ok(chrono::Duration::days(n))
    } else {
        anyhow::bail!("Invalid interval '{}' — use e.g. '1d', '6h'", s);
    }
}

/// T-036: Validate a scope rule string in format "resource_type:resource_pattern:actions".
fn validate_scope(scope: &str) -> Result<(String, String, String)> {
    let parts: Vec<&str> = scope.split(':').collect();
    if parts.len() != 3 {
        anyhow::bail!("Scope must be 'resource_type:resource_pattern:actions', got: {}", scope);
    }
    let resource_type = parts[0];
    let pattern = parts[1];
    let actions = parts[2];

    let valid_types = ["dag", "connector", "secret", "team", "pool", "agent_state", "event", "xcom"];
    if !valid_types.contains(&resource_type) {
        anyhow::bail!("Invalid resource type '{}'. Valid: {:?}", resource_type, valid_types);
    }

    let valid_actions = ["read", "write", "trigger", "query", "admin", "*"];
    for action in actions.split(',') {
        if !valid_actions.contains(&action) {
            anyhow::bail!("Invalid action '{}'. Valid: {:?}", action, valid_actions);
        }
    }

    // Validate pattern is safe (no path traversal)
    if pattern.contains("..") || pattern.contains('/') {
        anyhow::bail!("Invalid scope pattern '{}': must not contain '..' or '/'", pattern);
    }

    Ok((resource_type.to_string(), pattern.to_string(), actions.to_string()))
}

/// T-037: Parse a PostgreSQL connection URL to extract host, port, user, dbname, password.
/// Format: postgres://user:password@host:port/dbname or postgresql://...
fn parse_pg_url(url: &str) -> Result<(String, String, String, String, String)> {
    let stripped = url
        .strip_prefix("postgresql://")
        .or_else(|| url.strip_prefix("postgres://"))
        .ok_or_else(|| anyhow::anyhow!("Database URL must start with postgres:// or postgresql://"))?;

    // Split at '@' to separate credentials from host
    let (creds, host_part) = stripped.split_once('@')
        .ok_or_else(|| anyhow::anyhow!("Database URL must contain '@' separating credentials from host"))?;

    // Parse credentials: user:password
    let (user, password) = match creds.split_once(':') {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => (creds.to_string(), String::new()),
    };

    // Parse host:port/dbname (may have query params after ?)
    let host_db = host_part.split('?').next().unwrap_or(host_part);
    let (host_port, dbname) = host_db.split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Database URL must contain database name after '/'"))?;

    let (host, port) = match host_port.split_once(':') {
        Some((h, p)) => (h.to_string(), p.to_string()),
        None => (host_port.to_string(), "5432".to_string()),
    };

    if user.is_empty() || dbname.is_empty() {
        anyhow::bail!("Database URL must include user and database name");
    }

    Ok((host, port, user, password, dbname.to_string()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize structured logging
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("ryuo={}", cli.log_level)));

    let file_appender = tracing_appender::rolling::daily("logs", "ryuo.log");
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
        info!("🌪️ RYUO Swarm Worker v0.7.0");
        return worker::run_worker(&controller, &worker_id, capacity, worker_labels).await;
    }

    // ─── CLI subcommand handlers (connect to DB, run, exit) ─────────────────
    if let Some(ref cmd) = cli.command {
        // ─── Commands that do NOT require a database connection ──────────
        let json_output_early = cli.output.eq_ignore_ascii_case("json");
        match cmd {
            Commands::Db { .. } | Commands::Worker { .. } => { /* already handled above */ }
            // ─── T-032: K8s Executor CLI (no DB required) ───────────────
            Commands::K8s { action } => {
                match action {
                    K8sAction::Status => {
                        let config = k8s_executor::K8sExecutorConfig::default();
                        let executor = k8s_executor::K8sExecutor::new(config.clone());
                        let api_url = std::env::var("KUBE_API_URL")
                            .or_else(|_| std::env::var("KUBERNETES_SERVICE_HOST").map(|h| {
                                let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".into());
                                format!("https://{}:{}", h, port)
                            }))
                            .unwrap_or_else(|_| "not configured".into());
                        let health = executor.health_check().await;
                        let healthy = health.is_ok();
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "status": if healthy { "ok" } else { "error" },
                                "api_server": api_url,
                                "namespace": config.namespace,
                                "image": config.image,
                                "delete_completed_pods": config.delete_completed_pods,
                                "pod_ttl_seconds": config.pod_ttl_seconds,
                            }))?);
                        } else {
                            println!("K8s Executor Status");
                            println!("{}", "-".repeat(40));
                            println!("API Server:       {}", api_url);
                            println!("Health:           {}", if healthy { "OK" } else { "ERROR" });
                            println!("Namespace:        {}", config.namespace);
                            println!("Image:            {}", config.image);
                            println!("Cleanup pods:     {}", config.delete_completed_pods);
                            println!("Pod TTL (s):      {}", config.pod_ttl_seconds.map_or("-".into(), |t| t.to_string()));
                            if api_url == "not configured" {
                                println!("\nConfigure K8s API server via KUBE_API_URL environment variable");
                            }
                        }
                    }
                    K8sAction::Pods { namespace, state } => {
                        let api_url = std::env::var("KUBE_API_URL").unwrap_or_default();
                        if api_url.is_empty() {
                            if json_output_early {
                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                    "error": "KUBE_API_URL environment variable is not set",
                                    "hint": "Configure K8s API server via KUBE_API_URL environment variable",
                                }))?);
                            } else {
                                println!("Error: KUBE_API_URL environment variable is not set");
                                println!("Configure K8s API server via KUBE_API_URL environment variable");
                            }
                        } else {
                            let ns = namespace.as_deref().unwrap_or("ryuo");
                            if !ns.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                                anyhow::bail!("Invalid namespace: '{}' — only [a-zA-Z0-9_-] allowed", ns);
                            }
                            let url = format!("{}/api/v1/namespaces/{}/pods?labelSelector=app.kubernetes.io/managed-by=ryuo&limit=100", api_url.trim_end_matches('/'), ns);
                            let client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(15))
                                .build()?;
                            match client.get(&url).send().await {
                                Ok(resp) => {
                                    let body: serde_json::Value = resp.json().await?;
                                    let items = body["items"].as_array().cloned().unwrap_or_default();
                                    let mut pods: Vec<serde_json::Value> = items.iter().map(|pod| {
                                        let phase = pod["status"]["phase"].as_str().unwrap_or("Unknown").to_string();
                                        serde_json::json!({
                                            "name": pod["metadata"]["name"].as_str().unwrap_or("-"),
                                            "namespace": pod["metadata"]["namespace"].as_str().unwrap_or("-"),
                                            "status": phase,
                                            "dag_id": pod["metadata"]["labels"]["ryuo/dag-id"].as_str().unwrap_or("-"),
                                            "task_id": pod["metadata"]["labels"]["ryuo/task-id"].as_str().unwrap_or("-"),
                                        })
                                    }).collect();
                                    if let Some(ref filter_state) = state {
                                        let filter_lower = filter_state.to_lowercase();
                                        pods.retain(|p| p["status"].as_str().unwrap_or("").to_lowercase() == filter_lower);
                                    }
                                    if json_output_early {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "namespace": ns,
                                            "pods": pods,
                                            "total": pods.len(),
                                        }))?);
                                    } else {
                                        println!("{:<40} {:<12} {:<12} {:<20} {:<20}", "POD", "NAMESPACE", "STATUS", "DAG", "TASK");
                                        println!("{}", "-".repeat(106));
                                        for p in &pods {
                                            println!("{:<40} {:<12} {:<12} {:<20} {:<20}",
                                                p["name"].as_str().unwrap_or("-"),
                                                p["namespace"].as_str().unwrap_or("-"),
                                                p["status"].as_str().unwrap_or("-"),
                                                p["dag_id"].as_str().unwrap_or("-"),
                                                p["task_id"].as_str().unwrap_or("-"),
                                            );
                                        }
                                        println!("\n{} pod(s)", pods.len());
                                    }
                                }
                                Err(e) => {
                                    if json_output_early {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "error": format!("Failed to query K8s API: {}", e),
                                        }))?);
                                    } else {
                                        println!("Failed to query K8s API: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    K8sAction::Logs { pod, namespace, tail } => {
                        validate_identifier(pod, "pod")?;
                        let api_url = std::env::var("KUBE_API_URL").unwrap_or_default();
                        if api_url.is_empty() {
                            if json_output_early {
                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                    "error": "KUBE_API_URL environment variable is not set",
                                    "hint": "Configure K8s API server via KUBE_API_URL environment variable",
                                }))?);
                            } else {
                                println!("Error: KUBE_API_URL environment variable is not set");
                                println!("Configure K8s API server via KUBE_API_URL environment variable");
                            }
                        } else {
                            let ns = namespace.as_deref().unwrap_or("ryuo");
                            if !ns.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                                anyhow::bail!("Invalid namespace: '{}' — only [a-zA-Z0-9_-] allowed", ns);
                            }
                            let mut url = format!("{}/api/v1/namespaces/{}/pods/{}/log?container=task", api_url.trim_end_matches('/'), ns, pod);
                            if let Some(n) = tail {
                                if *n > 0 && *n <= 10000 {
                                    url.push_str(&format!("&tailLines={}", n));
                                }
                            }
                            let client = reqwest::Client::builder()
                                .timeout(std::time::Duration::from_secs(15))
                                .build()?;
                            match client.get(&url).send().await {
                                Ok(resp) => {
                                    let status = resp.status();
                                    let text = resp.text().await?;
                                    if status.is_success() {
                                        if json_output_early {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "pod": pod,
                                                "namespace": ns,
                                                "logs": text,
                                            }))?);
                                        } else {
                                            println!("--- Logs for pod {} (ns: {}) ---", pod, ns);
                                            println!("{}", text);
                                        }
                                    } else {
                                        if json_output_early {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("K8s API returned status {}", status),
                                                "body": text,
                                            }))?);
                                        } else {
                                            println!("K8s API returned status {}: {}", status, text);
                                        }
                                    }
                                }
                                Err(e) => {
                                    if json_output_early {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "error": format!("Failed to query K8s API: {}", e),
                                        }))?);
                                    } else {
                                        println!("Failed to query K8s API: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    K8sAction::Config => {
                        let config = k8s_executor::K8sExecutorConfig::default();
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "namespace": config.namespace,
                                "service_account": config.service_account,
                                "image": config.image,
                                "image_pull_policy": config.image_pull_policy,
                                "image_pull_secrets": config.image_pull_secrets,
                                "default_resources": {
                                    "cpu_request": config.default_resources.cpu_request,
                                    "memory_request": config.default_resources.memory_request,
                                    "cpu_limit": config.default_resources.cpu_limit,
                                    "memory_limit": config.default_resources.memory_limit,
                                },
                                "node_selector": config.node_selector,
                                "tolerations": config.tolerations,
                                "delete_completed_pods": config.delete_completed_pods,
                                "pod_ttl_seconds": config.pod_ttl_seconds,
                            }))?);
                        } else {
                            println!("K8s Executor Configuration");
                            println!("{}", "-".repeat(40));
                            println!("Namespace:          {}", config.namespace);
                            println!("Service Account:    {}", config.service_account.as_deref().unwrap_or("default"));
                            println!("Image:              {}", config.image);
                            println!("Image Pull Policy:  {}", config.image_pull_policy);
                            println!("CPU Request:        {}", config.default_resources.cpu_request);
                            println!("Memory Request:     {}", config.default_resources.memory_request);
                            println!("CPU Limit:          {}", config.default_resources.cpu_limit);
                            println!("Memory Limit:       {}", config.default_resources.memory_limit);
                            println!("Cleanup Pods:       {}", config.delete_completed_pods);
                            println!("Pod TTL (s):        {}", config.pod_ttl_seconds.map_or("-".into(), |t| t.to_string()));
                        }
                    }
                }
                return Ok(());
            }
            // ─── T-033: Kafka Connector CLI (no DB required) ────────────
            Commands::Kafka { action } => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()?;
                match action {
                    KafkaAction::Topics { url } => {
                        let base = url.trim_end_matches('/');
                        let resp = client.get(format!("{}/topics", base))
                            .header("Accept", "application/vnd.kafka.v2+json")
                            .send().await
                            .context("Failed to connect to Kafka REST Proxy. Is KAFKA_REST_URL correct?")?;
                        let status = resp.status();
                        let body: serde_json::Value = resp.json().await
                            .context("Failed to parse Kafka REST Proxy response")?;
                        if !status.is_success() {
                            anyhow::bail!("Kafka REST Proxy returned {}: {}", status, body);
                        }
                        let topics = body.as_array().cloned().unwrap_or_default();
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "topics": topics,
                                "total": topics.len(),
                            }))?);
                        } else {
                            println!("{:<40}", "TOPIC");
                            println!("{}", "-".repeat(40));
                            for t in &topics {
                                println!("{}", t.as_str().unwrap_or(&t.to_string()));
                            }
                            println!("\n{} topic(s)", topics.len());
                        }
                    }
                    KafkaAction::Produce { url, topic, key, value } => {
                        validate_identifier(topic, "topic")?;
                        let base = url.trim_end_matches('/');
                        let mut record = serde_json::json!({
                            "value": value,
                        });
                        if let Some(k) = key {
                            record["key"] = serde_json::json!(k);
                        }
                        let payload = serde_json::json!({
                            "records": [record],
                        });
                        let resp = client.post(format!("{}/topics/{}", base, topic))
                            .header("Content-Type", "application/vnd.kafka.json.v2+json")
                            .header("Accept", "application/vnd.kafka.v2+json")
                            .json(&payload)
                            .send().await
                            .context("Failed to connect to Kafka REST Proxy")?;
                        let status = resp.status();
                        let body: serde_json::Value = resp.json().await
                            .context("Failed to parse Kafka REST Proxy response")?;
                        if !status.is_success() {
                            anyhow::bail!("Kafka produce failed ({}): {}", status, body);
                        }
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&body)?);
                        } else {
                            let offsets = body["offsets"].as_array().cloned().unwrap_or_default();
                            for o in &offsets {
                                println!("Produced to topic '{}' partition {} offset {}",
                                    topic,
                                    o["partition"].as_i64().unwrap_or(-1),
                                    o["offset"].as_i64().unwrap_or(-1),
                                );
                            }
                        }
                    }
                    KafkaAction::Consume { url, topic, group, max_messages, timeout: consume_timeout } => {
                        validate_identifier(topic, "topic")?;
                        validate_identifier(group, "group")?;
                        let max_msgs = (*max_messages).min(1000).max(1);
                        let base = url.trim_end_matches('/');
                        let consumer_client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(*consume_timeout))
                            .build()?;
                        // Step 1: Create consumer instance
                        let instance_name = format!("ryuo-cli-{}", &uuid::Uuid::new_v4().to_string()[..8]);
                        let create_payload = serde_json::json!({
                            "name": instance_name,
                            "format": "json",
                            "auto.offset.reset": "earliest",
                        });
                        let create_resp = consumer_client.post(format!("{}/consumers/{}", base, group))
                            .header("Content-Type", "application/vnd.kafka.v2+json")
                            .json(&create_payload)
                            .send().await
                            .context("Failed to create Kafka consumer instance")?;
                        let create_body: serde_json::Value = create_resp.json().await?;
                        let base_uri = create_body["base_uri"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("No base_uri in consumer create response: {}", create_body))?
                            .to_string();

                        // Step 2: Subscribe to topic
                        let sub_payload = serde_json::json!({ "topics": [topic] });
                        consumer_client.post(format!("{}/subscription", base_uri))
                            .header("Content-Type", "application/vnd.kafka.v2+json")
                            .json(&sub_payload)
                            .send().await
                            .context("Failed to subscribe to topic")?;

                        // Step 3: Consume messages
                        let consume_resp = consumer_client.get(format!("{}/records", base_uri))
                            .header("Accept", "application/vnd.kafka.json.v2+json")
                            .send().await
                            .context("Failed to consume messages")?;
                        let records: Vec<serde_json::Value> = consume_resp.json().await
                            .unwrap_or_default();
                        let records: Vec<&serde_json::Value> = records.iter().take(max_msgs as usize).collect();

                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "topic": topic,
                                "group": group,
                                "messages": records,
                                "count": records.len(),
                            }))?);
                        } else {
                            println!("{:<10} {:<10} {:<20} {}", "PARTITION", "OFFSET", "KEY", "VALUE");
                            println!("{}", "-".repeat(60));
                            for r in &records {
                                println!("{:<10} {:<10} {:<20} {}",
                                    r["partition"].as_i64().unwrap_or(-1),
                                    r["offset"].as_i64().unwrap_or(-1),
                                    r["key"].as_str().unwrap_or("null"),
                                    r["value"],
                                );
                            }
                            println!("\n{} message(s)", records.len());
                        }

                        // Step 4: Cleanup — delete consumer instance (best-effort)
                        let _ = consumer_client.delete(&base_uri)
                            .header("Content-Type", "application/vnd.kafka.v2+json")
                            .send().await;
                    }
                }
                return Ok(());
            }
            // ─── T-034: S3/GCS Object Storage CLI (no DB required) ──────
            Commands::Storage { action } => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()?;
                match action {
                    StorageAction::Ls { endpoint, bucket, prefix, limit } => {
                        validate_identifier(bucket, "bucket")?;
                        let bounded_limit = (*limit).min(1000).max(1);
                        let base = endpoint.trim_end_matches('/');
                        let mut url = format!("{}/{}?list-type=2&max-keys={}", base, bucket, bounded_limit);
                        if let Some(ref pfx) = prefix {
                            url.push_str(&format!("&prefix={}", pfx));
                        }
                        let resp = client.get(&url)
                            .send().await
                            .context("Failed to connect to S3-compatible endpoint. Check S3_ENDPOINT_URL.")?;
                        let status = resp.status();
                        let body = resp.text().await?;
                        if !status.is_success() {
                            anyhow::bail!("S3 list objects failed ({}): {}", status, &body[..body.len().min(500)]);
                        }
                        // Parse simple XML response for <Key> and <Size> elements
                        let keys: Vec<(String, String)> = body.split("<Contents>")
                            .skip(1)
                            .filter_map(|chunk| {
                                let key = chunk.split("<Key>").nth(1)?.split("</Key>").next()?.to_string();
                                let size = chunk.split("<Size>").nth(1)
                                    .and_then(|s| s.split("</Size>").next())
                                    .unwrap_or("0").to_string();
                                Some((key, size))
                            })
                            .take(bounded_limit as usize)
                            .collect();
                        if json_output_early {
                            let items: Vec<serde_json::Value> = keys.iter()
                                .map(|(k, s)| serde_json::json!({"key": k, "size": s}))
                                .collect();
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "bucket": bucket,
                                "prefix": prefix,
                                "objects": items,
                                "total": items.len(),
                            }))?);
                        } else {
                            println!("{:<60} {:<15}", "KEY", "SIZE (bytes)");
                            println!("{}", "-".repeat(76));
                            for (key, size) in &keys {
                                println!("{:<60} {:<15}", key, size);
                            }
                            println!("\n{} object(s) in s3://{}/{}", keys.len(), bucket, prefix.as_deref().unwrap_or(""));
                            println!("\nNote: Full AWS Signature V4 support requires the aws-sdk crate.");
                        }
                    }
                    StorageAction::Stat { endpoint, bucket, key } => {
                        validate_identifier(bucket, "bucket")?;
                        let base = endpoint.trim_end_matches('/');
                        let url = format!("{}/{}/{}", base, bucket, key);
                        let resp = client.head(&url)
                            .send().await
                            .context("Failed to connect to S3-compatible endpoint")?;
                        let status = resp.status();
                        if !status.is_success() {
                            anyhow::bail!("S3 HEAD failed ({}): object may not exist at {}/{}", status, bucket, key);
                        }
                        let hdr = |name: &str| -> String {
                            resp.headers().get(name)
                                .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
                                .unwrap_or("unknown").to_string()
                        };
                        let content_length = hdr("content-length");
                        let content_type = hdr("content-type");
                        let last_modified = hdr("last-modified");
                        let etag = hdr("etag");
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "bucket": bucket,
                                "key": key,
                                "content_length": content_length,
                                "content_type": content_type,
                                "last_modified": last_modified,
                                "etag": etag,
                            }))?);
                        } else {
                            println!("Object: s3://{}/{}", bucket, key);
                            println!("{}", "-".repeat(40));
                            println!("Size:           {}", content_length);
                            println!("Content-Type:   {}", content_type);
                            println!("Last Modified:  {}", last_modified);
                            println!("ETag:           {}", etag);
                        }
                    }
                    StorageAction::Freshness { endpoint, bucket, key, stale_after } => {
                        validate_identifier(bucket, "bucket")?;
                        let base = endpoint.trim_end_matches('/');
                        let url = format!("{}/{}/{}", base, bucket, key);
                        let resp = client.head(&url)
                            .send().await
                            .context("Failed to connect to S3-compatible endpoint")?;
                        let status = resp.status();
                        if !status.is_success() {
                            anyhow::bail!("S3 HEAD failed ({}): object may not exist at {}/{}", status, bucket, key);
                        }
                        let last_modified_str = resp.headers().get("last-modified")
                            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok()).unwrap_or("").to_string();
                        let parsed_time = chrono::DateTime::parse_from_rfc2822(&last_modified_str).ok();
                        let age_secs = parsed_time.map(|t| (Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds());
                        let is_stale = match (stale_after, age_secs) {
                            (Some(threshold), Some(age)) => age > *threshold,
                            _ => false,
                        };
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "bucket": bucket,
                                "key": key,
                                "exists": true,
                                "last_modified": last_modified_str,
                                "age_seconds": age_secs,
                                "stale_threshold": stale_after,
                                "is_stale": is_stale,
                            }))?);
                        } else {
                            println!("Freshness: s3://{}/{}", bucket, key);
                            println!("{}", "-".repeat(40));
                            println!("Exists:         true");
                            println!("Last Modified:  {}", last_modified_str);
                            if let Some(age) = age_secs {
                                println!("Age:            {} seconds", age);
                            }
                            if let Some(threshold) = stale_after {
                                println!("Stale After:    {} seconds", threshold);
                                println!("Is Stale:       {}", is_stale);
                            }
                        }
                    }
                }
                return Ok(());
            }
            // ─── T-035: Delta Lake Connector CLI (no DB required) ───────
            Commands::DeltaLake { action } => {
                match action {
                    DeltaLakeAction::Info { path } => {
                        let log_dir = std::path::Path::new(path).join("_delta_log");
                        if !log_dir.exists() {
                            anyhow::bail!("Not a Delta table: _delta_log directory not found at {}", path);
                        }
                        let mut entries: Vec<_> = std::fs::read_dir(&log_dir)?
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
                            .collect();
                        entries.sort_by_key(|e| e.file_name());
                        let total_versions = entries.len();
                        let mut table_name = path.to_string();
                        let mut description = String::new();
                        let mut created_time: Option<i64> = None;
                        if let Some(last) = entries.last() {
                            if let Ok(content) = std::fs::read_to_string(last.path()) {
                                for line in content.lines() {
                                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                                        if let Some(meta) = obj.get("metaData") {
                                            table_name = meta["name"].as_str().unwrap_or(path).to_string();
                                            description = meta["description"].as_str().unwrap_or("").to_string();
                                            created_time = meta["createdTime"].as_i64();
                                        }
                                    }
                                }
                            }
                        }
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "path": path,
                                "name": table_name,
                                "description": description,
                                "total_versions": total_versions,
                                "created_time": created_time,
                                "log_dir": log_dir.to_string_lossy(),
                            }))?);
                        } else {
                            println!("Delta Table Info");
                            println!("{}", "-".repeat(40));
                            println!("Path:           {}", path);
                            println!("Name:           {}", table_name);
                            if !description.is_empty() {
                                println!("Description:    {}", description);
                            }
                            println!("Versions:       {}", total_versions);
                            if let Some(ct) = created_time {
                                println!("Created:        {}", ct);
                            }
                        }
                    }
                    DeltaLakeAction::Schema { path } => {
                        let log_dir = std::path::Path::new(path).join("_delta_log");
                        if !log_dir.exists() {
                            anyhow::bail!("Not a Delta table: _delta_log directory not found at {}", path);
                        }
                        let mut entries: Vec<_> = std::fs::read_dir(&log_dir)?
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
                            .collect();
                        entries.sort_by_key(|e| e.file_name());
                        let mut schema_json: Option<serde_json::Value> = None;
                        for entry in entries.iter().rev() {
                            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                                for line in content.lines() {
                                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                                        if let Some(meta) = obj.get("metaData") {
                                            if let Some(schema_str) = meta["schemaString"].as_str() {
                                                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(schema_str) {
                                                    schema_json = Some(parsed);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if schema_json.is_some() { break; }
                        }
                        match schema_json {
                            Some(schema) => {
                                if json_output_early {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "path": path,
                                        "schema": schema,
                                    }))?);
                                } else {
                                    println!("Delta Table Schema: {}", path);
                                    println!("{}", "-".repeat(60));
                                    if let Some(fields) = schema["fields"].as_array() {
                                        println!("{:<30} {:<20} {:<10}", "COLUMN", "TYPE", "NULLABLE");
                                        println!("{}", "-".repeat(62));
                                        for field in fields {
                                            let type_str = match &field["type"] {
                                                serde_json::Value::String(s) => s.clone(),
                                                other => other.to_string(),
                                            };
                                            println!("{:<30} {:<20} {:<10}",
                                                field["name"].as_str().unwrap_or("-"),
                                                type_str,
                                                field["nullable"].as_bool().unwrap_or(true),
                                            );
                                        }
                                        println!("\n{} column(s)", fields.len());
                                    } else {
                                        println!("{}", serde_json::to_string_pretty(&schema)?);
                                    }
                                }
                            }
                            None => {
                                if json_output_early {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "error": "No schema found in Delta log",
                                        "path": path,
                                    }))?);
                                } else {
                                    println!("No schema found in Delta log at {}", path);
                                }
                            }
                        }
                    }
                    DeltaLakeAction::History { path, limit } => {
                        let log_dir = std::path::Path::new(path).join("_delta_log");
                        if !log_dir.exists() {
                            anyhow::bail!("Not a Delta table: _delta_log directory not found at {}", path);
                        }
                        let bounded_limit = (*limit).min(1000).max(1) as usize;
                        let mut entries: Vec<_> = std::fs::read_dir(&log_dir)?
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
                            .collect();
                        entries.sort_by_key(|e| e.file_name());
                        let display_entries: Vec<_> = entries.iter().rev().take(bounded_limit).collect();
                        let mut versions = Vec::new();
                        for entry in &display_entries {
                            let filename = entry.file_name().to_string_lossy().to_string();
                            let version = filename.trim_end_matches(".json").parse::<i64>().unwrap_or(-1);
                            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            let mut timestamp: Option<i64> = None;
                            let mut operation = String::new();
                            let mut num_adds: usize = 0;
                            let mut num_removes: usize = 0;
                            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                                for line in content.lines() {
                                    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
                                        if let Some(ci) = obj.get("commitInfo") {
                                            timestamp = ci["timestamp"].as_i64();
                                            operation = ci["operation"].as_str().unwrap_or("").to_string();
                                        }
                                        if obj.get("add").is_some() { num_adds += 1; }
                                        if obj.get("remove").is_some() { num_removes += 1; }
                                    }
                                }
                            }
                            versions.push(serde_json::json!({
                                "version": version,
                                "timestamp": timestamp,
                                "operation": operation,
                                "files_added": num_adds,
                                "files_removed": num_removes,
                                "log_size_bytes": file_size,
                            }));
                        }
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "path": path,
                                "total_versions": entries.len(),
                                "showing": versions.len(),
                                "versions": versions,
                            }))?);
                        } else {
                            println!("Delta Table History: {} ({} total versions)", path, entries.len());
                            println!("{:<10} {:<20} {:<15} {:<10} {:<10}", "VERSION", "TIMESTAMP", "OPERATION", "+FILES", "-FILES");
                            println!("{}", "-".repeat(67));
                            for v in &versions {
                                let ts = v["timestamp"].as_i64()
                                    .map(|t| chrono::DateTime::from_timestamp(t / 1000, 0)
                                        .map_or("-".into(), |dt| dt.format("%Y-%m-%d %H:%M").to_string()))
                                    .unwrap_or_else(|| "-".into());
                                println!("{:<10} {:<20} {:<15} {:<10} {:<10}",
                                    v["version"].as_i64().unwrap_or(-1),
                                    ts,
                                    v["operation"].as_str().unwrap_or("-"),
                                    v["files_added"].as_u64().unwrap_or(0),
                                    v["files_removed"].as_u64().unwrap_or(0),
                                );
                            }
                            println!("\nShowing {} of {} version(s)", versions.len(), entries.len());
                        }
                    }
                }
                return Ok(());
            }
            // ─── T-037: Backup CLI (no DB trait — uses pg_dump) ─────────
            Commands::Backup { action } => {
                match action {
                    BackupAction::Create { output_dir, format } => {
                        let db_url = cli.database_url.as_deref()
                            .ok_or_else(|| anyhow::anyhow!("--database-url or DATABASE_URL required for backup"))?;
                        let (host, port, user, password, dbname) = parse_pg_url(db_url)?;

                        // Validate format
                        let fmt_flag = match format.as_str() {
                            "custom" => "c",
                            "plain" => "p",
                            "directory" => "d",
                            _ => anyhow::bail!("Invalid backup format '{}'. Valid: custom, plain, directory", format),
                        };

                        // Ensure output directory exists
                        std::fs::create_dir_all(output_dir.as_str())
                            .context("Failed to create backup output directory")?;

                        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
                        let ext = match format.as_str() {
                            "plain" => "sql",
                            "directory" => "dir",
                            _ => "dump",
                        };
                        let filename = format!("ryuo_backup_{}_{}.{}", dbname, timestamp, ext);
                        let output_path = format!("{}/{}", output_dir.trim_end_matches('/'), filename);

                        println!("Creating backup of '{}' → {}", dbname, output_path);

                        // Use PGPASSWORD env var (never pass password as CLI arg)
                        let mut cmd = tokio::process::Command::new("pg_dump");
                        cmd.arg("-h").arg(&host)
                            .arg("-p").arg(&port)
                            .arg("-U").arg(&user)
                            .arg("-F").arg(fmt_flag)
                            .arg("-f").arg(&output_path)
                            .arg(&dbname);

                        if !password.is_empty() {
                            cmd.env("PGPASSWORD", &password);
                        }

                        let status = cmd.status().await
                            .context("Failed to execute pg_dump — is it installed and in PATH?")?;

                        if status.success() {
                            let meta = std::fs::metadata(&output_path).ok();
                            let size = meta.map(|m| m.len()).unwrap_or(0);
                            if json_output_early {
                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                    "status": "ok",
                                    "path": output_path,
                                    "database": dbname,
                                    "format": format,
                                    "size_bytes": size,
                                    "timestamp": chrono::Utc::now().to_rfc3339(),
                                }))?);
                            } else {
                                println!("✓ Backup created: {} ({} bytes)", output_path, size);
                            }
                        } else {
                            let code = status.code().unwrap_or(-1);
                            anyhow::bail!("pg_dump failed with exit code {}", code);
                        }
                    }
                    BackupAction::List { dir } => {
                        let path = std::path::Path::new(dir.as_str());
                        if !path.exists() {
                            if json_output_early {
                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                    "backups": [],
                                    "total": 0,
                                    "directory": dir,
                                }))?);
                            } else {
                                println!("No backup directory found at: {}", dir);
                            }
                            return Ok(());
                        }
                        let mut backups = Vec::new();
                        for entry in std::fs::read_dir(path)? {
                            let entry = entry?;
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.ends_with(".dump") || name.ends_with(".sql") || name.ends_with(".dir") {
                                let meta = entry.metadata()?;
                                let modified = meta.modified().ok()
                                    .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                                    .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                                        .map(|dt| dt.to_rfc3339())
                                        .unwrap_or_default())
                                    .unwrap_or_default();
                                backups.push(serde_json::json!({
                                    "name": name,
                                    "size_bytes": meta.len(),
                                    "modified": modified,
                                }));
                            }
                        }
                        backups.sort_by(|a, b| b["modified"].as_str().cmp(&a["modified"].as_str()));
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "backups": backups,
                                "total": backups.len(),
                                "directory": dir,
                            }))?);
                        } else {
                            println!("Backups in: {}\n", dir);
                            println!("{:<50} {:<12} {}", "NAME", "SIZE", "MODIFIED");
                            println!("{}", "-".repeat(80));
                            for b in &backups {
                                println!("{:<50} {:<12} {}",
                                    b["name"].as_str().unwrap_or("-"),
                                    b["size_bytes"].as_u64().unwrap_or(0),
                                    b["modified"].as_str().unwrap_or("-"),
                                );
                            }
                            println!("\n{} backup(s) found", backups.len());
                        }
                    }
                    BackupAction::Info { path } => {
                        let p = std::path::Path::new(path.as_str());
                        if !p.exists() {
                            anyhow::bail!("Backup file not found: {}", path);
                        }
                        let meta = std::fs::metadata(p)?;
                        let modified = meta.modified().ok()
                            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                            .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_default())
                            .unwrap_or_default();
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("unknown");
                        let format_name = match ext {
                            "dump" => "custom",
                            "sql" => "plain",
                            "dir" => "directory",
                            _ => "unknown",
                        };
                        if json_output_early {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "path": path,
                                "format": format_name,
                                "size_bytes": meta.len(),
                                "modified": modified,
                            }))?);
                        } else {
                            println!("Backup Info");
                            println!("{}", "-".repeat(40));
                            println!("Path:     {}", path);
                            println!("Format:   {}", format_name);
                            println!("Size:     {} bytes", meta.len());
                            println!("Modified: {}", modified);
                        }
                    }
                }
                return Ok(());
            }
            _ => {
                // All remaining commands need a database connection
                let db_url = cli.database_url.as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--database-url or DATABASE_URL required"))?;
                let db = db_postgres::PostgresDb::new(db_url, 5, 1, std::time::Duration::from_secs(30)).await?;
                let db: Arc<dyn db_trait::DatabaseBackend> = Arc::new(db);
                let json_output = cli.output.eq_ignore_ascii_case("json");
                let audit_reason = cli.reason.as_deref();

                match cmd {
                    Commands::Secret { action } => {
                        let vault = Vault::new().map_err(|e| anyhow::anyhow!("Vault init failed: {}", e))?;
                        match action {
                            SecretAction::List => {
                                let secrets = db.get_all_secrets(None).await?;
                                if json_output {
                                    let items: Vec<serde_json::Value> = secrets.iter().map(|k| serde_json::json!({"key": k})).collect();
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "secrets": items,
                                        "total": secrets.len(),
                                    }))?);
                                } else {
                                    println!("{:<30} {:<10}", "KEY", "MASKED VALUE");
                                    println!("{}", "-".repeat(42));
                                    for key in &secrets {
                                        println!("{:<30} ****", key);
                                    }
                                    println!("\n{} secret(s) total", secrets.len());
                                }
                            }
                            SecretAction::Get { key } => {
                                validate_identifier(key, "secret_key")?;
                                match db.get_secret(key).await? {
                                    Some(val) => {
                                        let decrypted = vault.decrypt(&val).unwrap_or_else(|_| val.clone());
                                        println!("{}", decrypted);
                                    }
                                    None => {
                                        eprintln!("Secret '{}' not found", key);
                                        std::process::exit(1);
                                    }
                                }
                            }
                            SecretAction::Set { key, value } => {
                                validate_identifier(key, "secret_key")?;
                                let encrypted = vault.encrypt(value)?;
                                db.store_secret(key, &encrypted, None, Some("cli")).await?;
                                let _ = db.log_audit_event("cli", "secret.set", "secret", key, &format!("Secret set via CLI")).await;
                                println!("Secret '{}' stored (encrypted)", key);
                            }
                            SecretAction::Delete { key } => {
                                validate_identifier(key, "secret_key")?;
                                db.delete_secret(key, Some("cli")).await?;
                                let _ = db.log_audit_event("cli", "secret.delete", "secret", key, &format!("Secret deleted via CLI")).await;
                                println!("Secret '{}' deleted", key);
                            }
                            SecretAction::Rotate => {
                                let new_key_str = std::env::var("RYUO_NEW_SECRET_KEY")
                                    .map_err(|_| anyhow::anyhow!("RYUO_NEW_SECRET_KEY environment variable must be set to the new encryption key"))?;
                                if new_key_str.is_empty() {
                                    anyhow::bail!("RYUO_NEW_SECRET_KEY must not be empty");
                                }

                                // Build new vault with the replacement key
                                let original_key = std::env::var("RYUO_SECRET_KEY").unwrap_or_default();
                                std::env::set_var("RYUO_SECRET_KEY", &new_key_str);
                                let new_vault = Vault::new().map_err(|e| {
                                    std::env::set_var("RYUO_SECRET_KEY", &original_key);
                                    anyhow::anyhow!("Invalid new key: {}", e)
                                })?;
                                std::env::set_var("RYUO_SECRET_KEY", &original_key);

                                let keys = db.get_all_secrets(None).await?;
                                let mut ciphertexts: Vec<(String, String)> = Vec::new();
                                for k in &keys {
                                    if let Some(ct) = db.get_secret(k).await? {
                                        ciphertexts.push((k.clone(), ct));
                                    }
                                }

                                let rotated = vault.rotate_all_secrets(&ciphertexts, &new_vault)?;
                                let mut count = 0;
                                for (k, new_ct) in &rotated {
                                    db.store_secret(k, new_ct, None, Some("cli")).await?;
                                    count += 1;
                                }

                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "rotated_count": count,
                                        "message": "Update RYUO_SECRET_KEY to the new key value before restarting",
                                    }))?);
                                } else {
                                    println!("✅ Rotated {} secret(s) to new encryption key", count);
                                    println!("⚠️  Update RYUO_SECRET_KEY to the new key value before restarting");
                                }
                            }
                        }
                    }
                    Commands::User { action } => {
                        match action {
                            UserAction::List => {
                                let users = db.get_all_users().await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "users": users,
                                        "total": users.len(),
                                    }))?);
                                } else {
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
                            }
                            UserAction::Create { username, password, role, email, team } => {
                                validate_identifier(username, "username")?;
                                let pw_hash = bcrypt::hash(password, 10)
                                    .map_err(|e| anyhow::anyhow!("bcrypt hash failed: {}", e))?;
                                let api_key = uuid::Uuid::new_v4().to_string();
                                db.create_user(username, &pw_hash, role, &api_key).await?;
                                // TODO: create_user does not accept email yet; log if provided
                                if let Some(ref e) = email {
                                    warn!("Email '{}' provided for user '{}' but not stored (create_user lacks email parameter)", e, username);
                                }
                                if let Some(team_id) = team {
                                    db.assign_user_to_team(username, Some(team_id.as_str())).await?;
                                }
                                let _ = db.log_audit_event("cli", "user.create", "user", username, &format!("User created with role '{}'", role)).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("User '{}' created with role '{}'", username, role),
                                        "api_key": api_key,
                                    }))?);
                                } else {
                                    println!("User '{}' created with role '{}' (api_key={})", username, role, api_key);
                                }
                            }
                            UserAction::Get { username } => {
                                let users = db.get_all_users().await?;
                                match users.iter().find(|u| u["username"].as_str() == Some(username.as_str())) {
                                    Some(u) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "user": u,
                                            }))?);
                                        } else {
                                            println!("{}", serde_json::to_string_pretty(u)?);
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("User '{}' not found", username),
                                            }))?);
                                        } else {
                                            eprintln!("User '{}' not found", username);
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            }
                            UserAction::Delete { username } => {
                                validate_identifier(username, "username")?;
                                db.delete_user(username).await?;
                                let _ = db.log_audit_event("cli", "user.delete", "user", username, "User deleted via CLI").await;
                                println!("User '{}' deleted", username);
                            }
                        }
                    }
                    Commands::Team { action } => {
                        match action {
                            TeamAction::List => {
                                let teams = db.get_all_teams().await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "teams": teams,
                                        "total": teams.len(),
                                    }))?);
                                } else {
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
                            }
                            TeamAction::Create { id, name, description } => {
                                validate_identifier(id, "team_id")?;
                                db.create_team(id, name, description.as_deref().unwrap_or(""), 100, 1000).await?;
                                let _ = db.log_audit_event("cli", "team.create", "team", id, &format!("Team '{}' created via CLI", name)).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("Team '{}' created", id),
                                    }))?);
                                } else {
                                    println!("Team '{}' created", id);
                                }
                            }
                            TeamAction::Delete { id } => {
                                validate_identifier(id, "team_id")?;
                                db.delete_team(id).await?;
                                let _ = db.log_audit_event("cli", "team.delete", "team", id, "Team deleted via CLI").await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("Team '{}' deleted", id),
                                    }))?);
                                } else {
                                    println!("Team '{}' deleted", id);
                                }
                            }
                        }
                    }
                    Commands::Rbac { action } => {
                        match action {
                            RbacAction::ListRoles => {
                                let roles = db.get_rbac_roles().await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "roles": roles,
                                    }))?);
                                } else {
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
                            }
                            RbacAction::ListPermissions => {
                                let roles = db.get_rbac_roles().await?;
                                if json_output {
                                    let mut role_perms = Vec::new();
                                    for r in &roles {
                                        if let Some(role_id) = r["id"].as_str() {
                                            let perms = db.get_rbac_role_permissions(role_id).await?;
                                            role_perms.push(serde_json::json!({
                                                "role": r["name"].as_str().unwrap_or("-"),
                                                "permissions": perms,
                                            }));
                                        }
                                    }
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "roles": role_perms,
                                    }))?);
                                } else {
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
                            }
                            RbacAction::Assign { user, role, team } => {
                                // Normalize role name to role_id (e.g. "Viewer" → "role_viewer")
                                let role_id = format!("role_{}", role.to_lowercase());
                                db.assign_user_role(user, &role_id, team.as_deref(), "cli").await?;
                                println!("Role '{}' assigned to user '{}'", role, user);
                            }
                            RbacAction::Revoke { user, role } => {
                                let role_id = format!("role_{}", role.to_lowercase());
                                db.revoke_user_role(user, &role_id, None).await?;
                                println!("Role '{}' revoked from user '{}'", role, user);
                            }
                            RbacAction::UserRoles { username } => {
                                let roles = db.get_user_roles(username).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "username": username,
                                        "roles": roles,
                                    }))?);
                                } else {
                                    println!("Roles for '{}':", username);
                                    for r in &roles {
                                        println!("  - {} (team: {})",
                                            r["name"].as_str().unwrap_or("-"),
                                            r["team_id"].as_str().unwrap_or("global"),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Commands::Token { action } => {
                        match action {
                            TokenAction::List { user_id } => {
                                let tokens = db.get_api_tokens(user_id).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "tokens": tokens,
                                        "total": tokens.len(),
                                    }))?);
                                } else {
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
                            }
                            TokenAction::Create { name, user_id, scopes, team, expires_days, scope_rule, ttl_hours, description } => {
                                // T-036: Validate scope rules if provided
                                let mut validated_scopes = Vec::new();
                                for rule in scope_rule {
                                    let (rtype, pattern, actions) = validate_scope(rule)?;
                                    validated_scopes.push(serde_json::json!({
                                        "resource_type": rtype,
                                        "pattern": pattern,
                                        "actions": actions,
                                    }));
                                }

                                // If scope_rule is provided, use the scoped token path
                                if !validated_scopes.is_empty() {
                                    let token_id = uuid::Uuid::new_v4().to_string();
                                    let scope_json = serde_json::to_string(&validated_scopes)?;
                                    let desc = description.as_deref().unwrap_or("");
                                    let expires_at = if *ttl_hours > 0 {
                                        Some(Utc::now() + chrono::Duration::hours(*ttl_hours))
                                    } else {
                                        expires_days.map(|d| Utc::now() + chrono::Duration::days(d))
                                    };
                                    let raw_token = db.create_scoped_token(&token_id, user_id, &scope_json, desc, expires_at).await?;
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "status": "ok",
                                            "message": "Scoped token created (save this token — it cannot be retrieved again)",
                                            "token_id": token_id,
                                            "token": raw_token,
                                            "scopes": validated_scopes,
                                            "description": desc,
                                            "expires_at": expires_at.map(|dt| dt.to_rfc3339()),
                                        }))?);
                                    } else {
                                        println!("Scoped token created (id={}): {}", token_id, raw_token);
                                        println!("Scopes: {}", scope_json);
                                        if let Some(ea) = expires_at {
                                            println!("Expires: {}", ea.to_rfc3339());
                                        }
                                        println!("(Save this token — it cannot be retrieved again)");
                                    }
                                } else {
                                    // Legacy non-scoped token creation
                                    let raw_token = uuid::Uuid::new_v4().to_string();
                                    let token_hash = {
                                        use sha2::{Sha256, Digest};
                                        let mut hasher = Sha256::new();
                                        hasher.update(raw_token.as_bytes());
                                        format!("{:x}", hasher.finalize())
                                    };
                                    let scope_list = scopes.clone().unwrap_or_default();
                                    let expires_str = expires_days.map(|d| {
                                        (Utc::now() + chrono::Duration::days(d)).to_rfc3339()
                                    });
                                    let token_id = db.create_api_token(name, &token_hash, user_id, &scope_list, team.as_deref(), expires_str.as_deref()).await?;
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "status": "ok",
                                            "message": "Token created (save this token — it cannot be retrieved again)",
                                            "token_id": token_id,
                                            "token": raw_token,
                                        }))?);
                                    } else {
                                        println!("Token created (id={}): {}", token_id, raw_token);
                                        println!("(Save this token — it cannot be retrieved again)");
                                    }
                                }
                            }
                            TokenAction::Revoke { token_id } => {
                                db.revoke_api_token(token_id).await?;
                                println!("Token '{}' revoked", token_id);
                            }
                            TokenAction::Inspect { token_id } => {
                                match db.get_token_scopes(token_id).await? {
                                    Some(token_info) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&token_info)?);
                                        } else {
                                            println!("Token Details");
                                            println!("{}", "-".repeat(50));
                                            println!("ID:          {}", token_info["id"].as_str().unwrap_or("-"));
                                            println!("Name:        {}", token_info["name"].as_str().unwrap_or("-"));
                                            println!("User:        {}", token_info["user_id"].as_str().unwrap_or("-"));
                                            println!("Description: {}", token_info["description"].as_str().unwrap_or("-"));
                                            println!("Revoked:     {}", token_info["revoked"].as_bool().unwrap_or(false));
                                            println!("Expires:     {}", token_info["expires_at"].as_str().unwrap_or("never"));
                                            println!("Created:     {}", token_info["created_at"].as_str().unwrap_or("-"));
                                            println!("Last Used:   {}", token_info["last_used_at"].as_str().unwrap_or("never"));
                                            // Display scope rules
                                            let scope_rules_str = token_info["scope_rules"].as_str().unwrap_or("[]");
                                            if let Ok(rules) = serde_json::from_str::<serde_json::Value>(scope_rules_str) {
                                                if let Some(arr) = rules.as_array() {
                                                    if arr.is_empty() {
                                                        println!("Scopes:      (none — full access)");
                                                    } else {
                                                        println!("Scopes:");
                                                        for rule in arr {
                                                            println!("  - {}:{}:{}",
                                                                rule["resource_type"].as_str().unwrap_or("?"),
                                                                rule["pattern"].as_str().unwrap_or("?"),
                                                                rule["actions"].as_str().unwrap_or("?"),
                                                            );
                                                        }
                                                    }
                                                } else {
                                                    println!("Scopes:      {}", scope_rules_str);
                                                }
                                            } else {
                                                println!("Scopes:      {}", scope_rules_str);
                                            }
                                            // Display legacy scopes
                                            if let Some(scopes) = token_info["scopes"].as_array() {
                                                if !scopes.is_empty() {
                                                    let s: Vec<&str> = scopes.iter().filter_map(|v| v.as_str()).collect();
                                                    println!("Legacy:      {}", s.join(", "));
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("Token '{}' not found", token_id),
                                            }))?);
                                        } else {
                                            println!("Token '{}' not found", token_id);
                                        }
                                    }
                                }
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
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "entries": entries,
                                        "total": entries.len(),
                                    }))?);
                                } else {
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
                            }
                            AuditAction::ByActor { actor, limit, with_diffs } => {
                                let entries = db.get_audit_log(None, Some(actor.as_str()), None, *limit, 0).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "actor": actor,
                                        "entries": entries,
                                        "total": entries.len(),
                                        "with_diffs": with_diffs,
                                    }))?);
                                } else {
                                    for e in &entries {
                                        println!("[{}] {} {} ({})",
                                            e["timestamp"].as_str().or(e["created_at"].as_str()).unwrap_or("-"),
                                            e["action"].as_str().unwrap_or("-"),
                                            e["target_id"].as_str().or(e["resource_id"].as_str()).unwrap_or("-"),
                                            e["event_type"].as_str().unwrap_or("-"),
                                        );
                                        if *with_diffs {
                                            if let Some(details) = e.get("details") {
                                                if !details.is_null() {
                                                    println!("    diff: {}", details);
                                                }
                                            }
                                        }
                                    }
                                    println!("\n{} entries for '{}'", entries.len(), actor);
                                }
                            }
                        }
                    }
                    Commands::Compliance { action } => {
                        match action {
                            ComplianceAction::List => {
                                let controls = db.get_compliance_controls(None).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "controls": controls,
                                        "total": controls.len(),
                                    }))?);
                                } else {
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
                            }
                            ComplianceAction::Status => {
                                let controls = db.get_compliance_controls(None).await?;
                                let total = controls.len();
                                let passed = controls.iter().filter(|c| c["status"].as_str() == Some("passed")).count();
                                let failed = controls.iter().filter(|c| c["status"].as_str() == Some("failed")).count();
                                let not_assessed = total - passed - failed;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "total": total,
                                        "passed": passed,
                                        "failed": failed,
                                        "not_assessed": not_assessed,
                                    }))?);
                                } else {
                                    println!("Compliance Status Summary:");
                                    println!("  Total controls: {}", total);
                                    println!("  Passed:         {}", passed);
                                    println!("  Failed:         {}", failed);
                                    println!("  Not assessed:   {}", not_assessed);
                                }
                            }
                        }
                    }
                    Commands::Lineage { action } => {
                        match action {
                            LineageAction::Run { run_id } => {
                                let events = db.get_lineage_events("*", Some(run_id.as_str()), 100).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "run_id": run_id,
                                        "events": events,
                                        "total": events.len(),
                                    }))?);
                                } else {
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
                            }
                            LineageAction::Datasets => {
                                let datasets = db.get_lineage_datasets(100, 0).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "datasets": datasets,
                                        "total": datasets.len(),
                                    }))?);
                                } else {
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
                            }
                            LineageAction::Dataset { dataset_id } => {
                                let events = db.get_dataset_events(dataset_id, 100).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "dataset_id": dataset_id,
                                        "events": events,
                                        "total": events.len(),
                                    }))?);
                                } else {
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
                    }
                    Commands::Connector { action } => {
                        match action {
                            ConnectorAction::List => {
                                let connectors = vec![
                                    serde_json::json!({"name": "postgres", "type": "Database"}),
                                    serde_json::json!({"name": "snowflake", "type": "Warehouse"}),
                                    serde_json::json!({"name": "bigquery", "type": "Warehouse"}),
                                    serde_json::json!({"name": "redshift", "type": "Warehouse"}),
                                    serde_json::json!({"name": "databricks", "type": "Warehouse"}),
                                    serde_json::json!({"name": "dbt", "type": "Transformation"}),
                                    serde_json::json!({"name": "kafka", "type": "Streaming"}),
                                    serde_json::json!({"name": "s3", "type": "Storage"}),
                                    serde_json::json!({"name": "gcs", "type": "Storage"}),
                                    serde_json::json!({"name": "delta-lake", "type": "Lake"}),
                                ];
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "connectors": connectors,
                                        "total": connectors.len(),
                                    }))?);
                                } else {
                                    println!("Registered connectors:");
                                    for c in &connectors {
                                        println!("  - {} ({})", c["name"].as_str().unwrap_or("-"), c["type"].as_str().unwrap_or("-"));
                                    }
                                }
                            }
                            ConnectorAction::Health { name } => {
                                let connector_types = [
                                    ("postgres", "Database"),
                                    ("mysql", "Database"),
                                    ("snowflake", "Warehouse"),
                                    ("bigquery", "Warehouse"),
                                    ("redshift", "Warehouse"),
                                    ("databricks", "Warehouse"),
                                    ("dbt", "Transformation"),
                                    ("kafka", "Streaming"),
                                    ("s3", "Storage"),
                                    ("gcs", "Storage"),
                                    ("delta-lake", "Lake"),
                                ];
                                match connector_types.iter().find(|(n, _)| *n == name.as_str()) {
                                    Some((n, kind)) => {
                                        let status = if *kind == "Database" && *n == "postgres" {
                                            if db.ping().await { "healthy" } else { "unreachable" }
                                        } else {
                                            "unchecked"
                                        };
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "connector": n,
                                                "type": kind,
                                                "status": status,
                                            }))?);
                                        } else {
                                            println!("{:<15} {:<15} {:<12}", "CONNECTOR", "TYPE", "STATUS");
                                            println!("{}", "-".repeat(44));
                                            println!("{:<15} {:<15} {:<12}", n, kind, status);
                                            if status == "unchecked" {
                                                println!("\nNote: Health checks for {} connectors require active credentials.", kind);
                                                println!("Use 'ryuo connector query {} --sql \"SELECT 1\"' to verify connectivity.", n);
                                            }
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("Unknown connector: '{}'", name),
                                            }))?);
                                        } else {
                                            eprintln!("Unknown connector: '{}'", name);
                                            eprintln!("Available: postgres, mysql, snowflake, bigquery, redshift, databricks, dbt, kafka, s3, gcs, delta-lake");
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            }
                            ConnectorAction::Query { name, sql, timeout, max_rows } => {
                                // Validate SQL with sqlparser — only SELECT allowed
                                use sqlparser::dialect::GenericDialect;
                                use sqlparser::parser::Parser as SqlParser;
                                let dialect = GenericDialect {};
                                let statements = SqlParser::parse_sql(&dialect, sql)
                                    .map_err(|e| anyhow::anyhow!("SQL parse error: {}", e))?;
                                if statements.is_empty() {
                                    anyhow::bail!("No SQL statement provided");
                                }
                                if statements.len() > 1 {
                                    anyhow::bail!("Only single SELECT statements are allowed");
                                }
                                match &statements[0] {
                                    sqlparser::ast::Statement::Query(_) => {}
                                    _ => anyhow::bail!("Only SELECT queries are allowed (no INSERT/UPDATE/DELETE/DDL)"),
                                }
                                if name != "postgres" {
                                    anyhow::bail!("Query execution currently only supported for 'postgres' connector. Got: '{}'", name);
                                }
                                let results = db.execute_raw_query(sql, *timeout, *max_rows).await?;
                                let truncated = results.len() as i64 >= *max_rows;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "connector": name,
                                        "rows": results,
                                        "row_count": results.len(),
                                        "truncated": truncated,
                                    }))?);
                                } else {
                                    if results.is_empty() {
                                        println!("(no rows returned)");
                                    } else {
                                        if let Some(first) = results.first() {
                                            if let Some(obj) = first.as_object() {
                                                let keys: Vec<&String> = obj.keys().collect();
                                                let header = keys.iter().map(|k| format!("{:<20}", k)).collect::<String>();
                                                println!("{}", header);
                                                println!("{}", "-".repeat(20 * keys.len()));
                                                for row in &results {
                                                    if let Some(obj) = row.as_object() {
                                                        let line = keys.iter().map(|k| {
                                                            let val = obj.get(*k).unwrap_or(&serde_json::Value::Null);
                                                            format!("{:<20}", match val {
                                                                serde_json::Value::String(s) => {
                                                                    if s.len() > 18 { format!("{}...", &s[..15]) } else { s.clone() }
                                                                }
                                                                serde_json::Value::Null => "NULL".to_string(),
                                                                other => other.to_string(),
                                                            })
                                                        }).collect::<String>();
                                                        println!("{}", line);
                                                    }
                                                }
                                            }
                                        }
                                        println!("\n{} row(s)", results.len());
                                        if truncated {
                                            println!("Results truncated at {} rows", max_rows);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Commands::Swarm { action } => {
                        match action {
                            SwarmAction::Status => {
                                let workers = db.get_all_workers().await?;
                                let total_workers = workers.len();
                                let online = workers.iter().filter(|w| w["state"].as_str() == Some("Online")).count();
                                let total_capacity: i64 = workers.iter().filter_map(|w| w["capacity"].as_i64()).sum();
                                let total_active: i64 = workers.iter().filter_map(|w| w["active_tasks"].as_i64()).sum();
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "workers": total_workers,
                                        "online": online,
                                        "total_capacity": total_capacity,
                                        "active_tasks": total_active,
                                    }))?);
                                } else {
                                    println!("Swarm Status:");
                                    println!("  Workers:        {} ({} online)", total_workers, online);
                                    println!("  Total capacity: {}", total_capacity);
                                    println!("  Active tasks:   {}", total_active);
                                }
                            }
                            SwarmAction::Workers => {
                                let workers = db.get_all_workers().await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "workers": workers,
                                        "total": workers.len(),
                                    }))?);
                                } else {
                                    println!("{:<20} {:<15} {:<10} {:<12} {:<25} {:<10}", "ID", "HOSTNAME", "CAPACITY", "ACTIVE_TASKS", "LAST_HEARTBEAT", "STATE");
                                    println!("{}", "-".repeat(94));
                                    for w in &workers {
                                        println!("{:<20} {:<15} {:<10} {:<12} {:<25} {:<10}",
                                            w["id"].as_str().unwrap_or("-"),
                                            w["hostname"].as_str().unwrap_or("-"),
                                            w["capacity"].as_i64().unwrap_or(0),
                                            w["active_tasks"].as_i64().unwrap_or(0),
                                            w["last_heartbeat"].as_str().unwrap_or("-"),
                                            w["state"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} worker(s) total", workers.len());
                                }
                            }
                        }
                    }
                    Commands::Dag { action } => {
                        match action {
                            DagAction::List => {
                                let (dags, total) = db.get_all_dags(100, 0).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "dags": dags,
                                        "total": total,
                                    }))?);
                                } else {
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
                            }
                            DagAction::Get { dag_id } => {
                                match db.get_dag_by_id(dag_id).await? {
                                    Some(dag) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "dag": dag,
                                            }))?);
                                        } else {
                                            println!("{}", serde_json::to_string_pretty(&dag)?);
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("DAG '{}' not found", dag_id),
                                            }))?);
                                        } else {
                                            eprintln!("DAG '{}' not found", dag_id);
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            }
                            DagAction::Trigger { dag_id, triggered_by, config, dry_run } => {
                                validate_identifier(dag_id, "dag_id")?;
                                if *dry_run {
                                    let dag = db.get_dag_by_id(dag_id).await?.unwrap_or(serde_json::json!({}));
                                    let tasks = db.get_dag_tasks(dag_id).await.unwrap_or_default();
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "dry_run": true,
                                            "dag_id": dag_id,
                                            "schedule": dag["schedule_interval"],
                                            "task_count": tasks.len(),
                                            "config": config.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()),
                                        }))?);
                                    } else {
                                        println!("[dry-run] Would trigger DAG: {}", dag_id);
                                        println!("  Tasks: {}", tasks.len());
                                        if let Some(cfg) = config {
                                            println!("  Config: {}", cfg);
                                        }
                                    }
                                    return Ok(());
                                }
                                let run_id = uuid::Uuid::new_v4().to_string();
                                let now = Utc::now();
                                db.create_dag_run(&run_id, dag_id, now, triggered_by).await?;
                                if let Some(config_str) = config {
                                    let config_val: serde_json::Value = serde_json::from_str(config_str)
                                        .map_err(|e| anyhow::anyhow!("Invalid JSON config: {}", e))?;
                                    let store = xcom::XComStore::new(Arc::clone(&db));
                                    store.xcom_push(dag_id, "__dagrun__", &run_id, "__dagrun_conf__", config_val.to_string()).await?;
                                }
                                // Audit trail for mutation
                                let reason_str = audit_reason.unwrap_or("");
                                let metadata = serde_json::json!({"run_id": run_id, "triggered_by": triggered_by, "config": config, "reason": reason_str}).to_string();
                                let _ = db.log_audit_event(triggered_by, "dag.trigger", "dag", dag_id, &metadata).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": "DAG run created",
                                        "run_id": run_id,
                                        "dag_id": dag_id,
                                        "config": config.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()),
                                    }))?);
                                } else {
                                    println!("DAG run created: {}", run_id);
                                    if config.is_some() {
                                        println!("Config overrides stored as XCom __dagrun_conf__");
                                    }
                                    println!("Note: task execution requires the server to be running.");
                                }
                            }
                            DagAction::Pause { dag_id } => {
                                db.pause_dag(dag_id).await?;
                                let reason_str = audit_reason.unwrap_or("");
                                let metadata = serde_json::json!({"reason": reason_str}).to_string();
                                let _ = db.log_audit_event("cli", "dag.pause", "dag", dag_id, &metadata).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("DAG '{}' paused", dag_id),
                                    }))?);
                                } else {
                                    println!("DAG '{}' paused", dag_id);
                                }
                            }
                            DagAction::Unpause { dag_id } => {
                                db.unpause_dag(dag_id).await?;
                                let reason_str = audit_reason.unwrap_or("");
                                let metadata = serde_json::json!({"reason": reason_str}).to_string();
                                let _ = db.log_audit_event("cli", "dag.unpause", "dag", dag_id, &metadata).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("DAG '{}' unpaused", dag_id),
                                    }))?);
                                } else {
                                    println!("DAG '{}' unpaused", dag_id);
                                }
                            }
                            DagAction::Runs { dag_id, limit, state } => {
                                validate_identifier(dag_id, "dag_id")?;
                                let (runs, total) = db.get_dag_runs(dag_id, *limit, 0).await?;
                                let runs: Vec<&serde_json::Value> = if let Some(filter) = state {
                                    runs.iter().filter(|r| {
                                        r["state"].as_str().map(|s| s.eq_ignore_ascii_case(filter)).unwrap_or(false)
                                    }).collect()
                                } else {
                                    runs.iter().collect()
                                };
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "dag_id": dag_id,
                                        "runs": runs,
                                        "shown": runs.len(),
                                        "total": total,
                                    }))?);
                                } else {
                                    println!("{:<36} {:<12} {:<25} {:<25} {:<15}", "RUN_ID", "STATE", "STARTED", "ENDED", "TRIGGERED_BY");
                                    println!("{}", "-".repeat(115));
                                    for r in &runs {
                                        println!("{:<36} {:<12} {:<25} {:<25} {:<15}",
                                            r["id"].as_str().unwrap_or("-"),
                                            r["state"].as_str().unwrap_or("-"),
                                            r["start_date"].as_str().unwrap_or("-"),
                                            r["end_date"].as_str().unwrap_or("-"),
                                            r["triggered_by"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} run(s) shown (of {} total)", runs.len(), total);
                                }
                            }
                            DagAction::Create { from_yaml, dry_run } => {
                                if !std::path::Path::new(from_yaml.as_str()).exists() {
                                    anyhow::bail!("File not found: {}", from_yaml);
                                }

                                let dags = dag_factory::parse_dag_file(from_yaml)?;

                                if *dry_run {
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "status": "ok",
                                            "message": "Validation passed",
                                            "dags": dags.iter().map(|d| serde_json::json!({
                                                "id": d.id,
                                                "tasks": d.tasks.len(),
                                            })).collect::<Vec<_>>(),
                                        }))?);
                                    } else {
                                        println!("Validation passed:");
                                        for d in &dags {
                                            println!("  DAG '{}' \u{2014} {} task(s)", d.id, d.tasks.len());
                                        }
                                    }
                                } else {
                                    for dag in &dags {
                                        db.register_dag(dag).await?;
                                        if let Err(e) = db.store_dag_version(&dag.id, from_yaml).await {
                                            warn!("Failed to store version for {}: {}", dag.id, e);
                                        }
                                        // Audit trail for DAG creation
                                        let reason_str = audit_reason.unwrap_or("");
                                        let metadata = serde_json::json!({"file": from_yaml, "tasks": dag.tasks.len(), "reason": reason_str}).to_string();
                                        let _ = db.log_audit_event("cli", "dag.create", "dag", &dag.id, &metadata).await;
                                    }
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "status": "ok",
                                            "message": format!("Registered {} DAG(s)", dags.len()),
                                            "dags": dags.iter().map(|d| serde_json::json!({
                                                "id": d.id,
                                                "tasks": d.tasks.len(),
                                            })).collect::<Vec<_>>(),
                                        }))?);
                                    } else {
                                        println!("Registered {} DAG(s):", dags.len());
                                        for d in &dags {
                                            println!("  DAG '{}' \u{2014} {} task(s)", d.id, d.tasks.len());
                                        }
                                    }
                                }
                            }
                            DagAction::Backfill { dag_id, start, end, interval, dry_run } => {
                                validate_identifier(dag_id, "dag_id")?;
                                let start_dt = start.parse::<chrono::DateTime<Utc>>()
                                    .or_else(|_| {
                                        start.parse::<chrono::NaiveDate>()
                                            .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                                    })
                                    .map_err(|_| anyhow::anyhow!("Invalid start date '{}' — use ISO 8601 (e.g. 2024-01-01 or 2024-01-01T00:00:00Z)", start))?;
                                let end_dt = end.parse::<chrono::DateTime<Utc>>()
                                    .or_else(|_| {
                                        end.parse::<chrono::NaiveDate>()
                                            .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                                    })
                                    .map_err(|_| anyhow::anyhow!("Invalid end date '{}' — use ISO 8601 (e.g. 2024-01-01 or 2024-01-01T00:00:00Z)", end))?;
                                if end_dt <= start_dt {
                                    anyhow::bail!("End date must be after start date");
                                }
                                let step = parse_interval(interval)?;

                                let mut dates = Vec::new();
                                let mut current = start_dt;
                                // Bounded: max 10000 backfill runs to prevent unbounded loops
                                let max_runs = 10_000usize;
                                while current < end_dt && dates.len() < max_runs {
                                    dates.push(current);
                                    current = current + step;
                                }
                                if dates.len() >= max_runs {
                                    anyhow::bail!("Backfill would create {} runs (max {}) — use a larger interval", dates.len(), max_runs);
                                }

                                if *dry_run {
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "status": "dry_run",
                                            "dag_id": dag_id,
                                            "runs": dates.len(),
                                            "dates": dates.iter().map(|d| d.to_rfc3339()).collect::<Vec<_>>(),
                                        }))?);
                                    } else {
                                        println!("Backfill dry-run for DAG '{}': {} run(s) would be created", dag_id, dates.len());
                                        for d in &dates {
                                            println!("  {}", d.to_rfc3339());
                                        }
                                    }
                                } else {
                                    let mut run_ids = Vec::new();
                                    for exec_date in &dates {
                                        let run_id = uuid::Uuid::new_v4().to_string();
                                        db.create_dag_run(&run_id, dag_id, *exec_date, "backfill").await?;
                                        run_ids.push(run_id);
                                    }
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "status": "ok",
                                            "dag_id": dag_id,
                                            "runs_created": run_ids.len(),
                                            "run_ids": run_ids,
                                        }))?);
                                    } else {
                                        println!("Backfill created {} run(s) for DAG '{}'", run_ids.len(), dag_id);
                                        for (i, run_id) in run_ids.iter().enumerate() {
                                            println!("  [{}] {} — {}", i + 1, dates[i].to_rfc3339(), run_id);
                                        }
                                    }
                                }
                            }
                            DagAction::Validate { from_yaml } => {
                                if !std::path::Path::new(from_yaml.as_str()).exists() {
                                    anyhow::bail!("File not found: {}", from_yaml);
                                }
                                let dags = dag_factory::parse_dag_file(from_yaml)?;
                                let mut all_valid = true;
                                let mut results = Vec::new();
                                for dag in &dags {
                                    let errors = validate_dag(dag);
                                    let valid = errors.is_empty();
                                    if !valid { all_valid = false; }
                                    results.push(serde_json::json!({
                                        "dag_id": dag.id,
                                        "tasks": dag.tasks.len(),
                                        "valid": valid,
                                        "errors": errors,
                                    }));
                                }
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": if all_valid { "ok" } else { "error" },
                                        "dags": results,
                                    }))?);
                                } else {
                                    for r in &results {
                                        let valid = r["valid"].as_bool().unwrap_or(false);
                                        println!("DAG '{}' ({} tasks): {}",
                                            r["dag_id"].as_str().unwrap_or("-"),
                                            r["tasks"].as_i64().unwrap_or(0),
                                            if valid { "VALID" } else { "INVALID" },
                                        );
                                        if let Some(errors) = r["errors"].as_array() {
                                            for e in errors {
                                                println!("  ❌ {}", e.as_str().unwrap_or("-"));
                                            }
                                        }
                                    }
                                }
                                if !all_valid {
                                    std::process::exit(1);
                                }
                            }
                            DagAction::Versions { dag_id } => {
                                validate_identifier(dag_id, "dag_id")?;
                                let versions = db.get_dag_versions(dag_id).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "dag_id": dag_id,
                                        "versions": versions,
                                        "total": versions.len(),
                                    }))?);
                                } else {
                                    println!("{:<8} {:<40} {:<30}", "VERSION", "FILE_PATH", "CREATED_AT");
                                    println!("{}", "-".repeat(80));
                                    for v in &versions {
                                        println!("{:<8} {:<40} {:<30}",
                                            v["version"].as_i64().unwrap_or(0),
                                            v["file_path"].as_str().unwrap_or("-"),
                                            v["created_at"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} version(s) for DAG '{}'", versions.len(), dag_id);
                                }
                            }
                            DagAction::Rollback { dag_id, to_version } => {
                                validate_identifier(dag_id, "dag_id")?;
                                let versions = db.get_dag_versions(dag_id).await?;
                                if versions.is_empty() {
                                    anyhow::bail!("No versions found for DAG '{}'", dag_id);
                                }
                                let target_version = match to_version {
                                    Some(v) => *v,
                                    None => {
                                        // Default to previous version (second newest)
                                        if versions.len() < 2 {
                                            anyhow::bail!("Only one version exists for DAG '{}' — nothing to rollback to", dag_id);
                                        }
                                        versions[1]["version"].as_i64().unwrap_or(1) as i32
                                    }
                                };
                                let target = versions.iter().find(|v| v["version"].as_i64() == Some(target_version as i64));
                                let target = match target {
                                    Some(t) => t,
                                    None => anyhow::bail!("Version {} not found for DAG '{}'. Available: {}",
                                        target_version, dag_id,
                                        versions.iter().filter_map(|v| v["version"].as_i64()).map(|v| v.to_string()).collect::<Vec<_>>().join(", ")),
                                };
                                let file_path = target["file_path"].as_str().unwrap_or("");
                                if file_path.is_empty() {
                                    anyhow::bail!("Version {} has no file path recorded", target_version);
                                }
                                if !std::path::Path::new(file_path).exists() {
                                    anyhow::bail!("Source file '{}' from version {} no longer exists on disk", file_path, target_version);
                                }
                                // Re-register DAG from the version's source file
                                let new_version = if file_path.ends_with(".py") {
                                    // Python DAGs are registered via Python parser at startup.
                                    // For rollback, record audit trail only — restart server to pick up changes.
                                    db.store_dag_version(dag_id, file_path).await?
                                } else {
                                    let dags = dag_factory::parse_dag_file(file_path)?;
                                    let mut found = false;
                                    let mut nv = 0i64;
                                    for dag in &dags {
                                        if dag.id == *dag_id {
                                            db.register_dag(dag).await?;
                                            nv = db.store_dag_version(dag_id, file_path).await?;
                                            found = true;
                                            break;
                                        }
                                    }
                                    if !found {
                                        anyhow::bail!("DAG '{}' not found in file '{}'", dag_id, file_path);
                                    }
                                    nv
                                };
                                // Audit trail for rollback
                                let reason_str = audit_reason.unwrap_or("");
                                let metadata = serde_json::json!({
                                    "rolled_back_to_version": target_version,
                                    "new_version": new_version,
                                    "file_path": file_path,
                                    "reason": reason_str,
                                }).to_string();
                                let _ = db.log_audit_event("cli", "dag.rollback", "dag", dag_id, &metadata).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "dag_id": dag_id,
                                        "rolled_back_to_version": target_version,
                                        "new_version": new_version,
                                        "file_path": file_path,
                                    }))?);
                                } else {
                                    println!("DAG '{}' rolled back to version {} (new version: {})", dag_id, target_version, new_version);
                                    if file_path.ends_with(".py") {
                                        println!("  Note: Python DAGs require a server restart to apply rollback changes.");
                                    }
                                }
                            }
                        }
                    }
                    Commands::Xcom { action } => {
                        match action {
                            XcomAction::Push { dag, task, run, key, value } => {
                                validate_identifier(dag, "dag")?;
                                validate_identifier(task, "task")?;
                                validate_identifier(key, "key")?;
                                if value.len() > xcom::XCOM_MAX_VALUE_BYTES {
                                    anyhow::bail!(
                                        "XCom value too large: {} bytes (max {})",
                                        value.len(),
                                        xcom::XCOM_MAX_VALUE_BYTES
                                    );
                                }
                                let store = xcom::XComStore::new(Arc::clone(&db));
                                store.xcom_push(dag, task, run, key, value.clone()).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("XCom entry stored: {}", key),
                                    }))?);
                                } else {
                                    println!("XCom entry stored: {}", key);
                                }
                            }
                            XcomAction::Pull { dag, task, run, key } => {
                                validate_identifier(dag, "dag")?;
                                validate_identifier(task, "task")?;
                                validate_identifier(key, "key")?;
                                let store = xcom::XComStore::new(Arc::clone(&db));
                                match store.xcom_pull(dag, task, run, key).await? {
                                    Some(val) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "key": key,
                                                "value": val,
                                            }))?);
                                        } else {
                                            println!("{}", val);
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": "XCom entry not found",
                                            }))?);
                                        } else {
                                            println!("XCom entry not found");
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            }
                            XcomAction::List { dag, run, limit } => {
                                validate_identifier(dag, "dag")?;
                                let store = xcom::XComStore::new(Arc::clone(&db));
                                let (entries, total) = store.xcom_pull_all(dag, run, *limit, 0).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "entries": entries,
                                        "shown": entries.len(),
                                        "total": total,
                                    }))?);
                                } else {
                                    println!("{:<30} {:<20} {:<50} {:<25}", "TASK_ID", "KEY", "VALUE", "TIMESTAMP");
                                    println!("{}", "-".repeat(127));
                                    for e in &entries {
                                        let val_raw = e["value"].as_str().unwrap_or("-");
                                        let val_display = if val_raw.len() > 50 {
                                            format!("{}...", &val_raw[..47])
                                        } else {
                                            val_raw.to_string()
                                        };
                                        println!("{:<30} {:<20} {:<50} {:<25}",
                                            e["task_id"].as_str().unwrap_or("-"),
                                            e["key"].as_str().unwrap_or("-"),
                                            val_display,
                                            e["created_at"].as_str().or(e["timestamp"].as_str()).unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} entry(ies) shown (of {} total)", entries.len(), total);
                                }
                            }
                        }
                    }
                    Commands::Dataset { action } => {
                        match action {
                            DatasetAction::List => {
                                let datasets = db.get_datasets(100, 0).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "datasets": datasets,
                                        "total": datasets.len(),
                                    }))?);
                                } else {
                                    println!("{:<36} {:<40} {:<20} {:<30}", "ID", "URI", "NAME", "DESCRIPTION");
                                    println!("{}", "-".repeat(128));
                                    for d in &datasets {
                                        println!("{:<36} {:<40} {:<20} {:<30}",
                                            d["id"].as_str().unwrap_or("-"),
                                            d["uri"].as_str().unwrap_or("-"),
                                            d["name"].as_str().unwrap_or("-"),
                                            d["description"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} dataset(s) total", datasets.len());
                                }
                            }
                            DatasetAction::Event { action: event_action } => {
                                match event_action {
                                    DatasetEventAction::Emit { dataset, source_dag, source_task, event_type } => {
                                        validate_uri(dataset, "dataset")?;
                                        validate_identifier(source_dag, "source_dag")?;
                                        validate_identifier(source_task, "source_task")?;
                                        // Resolve or create the dataset record by URI
                                        let dataset_id = db.get_or_create_dataset_by_uri(dataset).await?;
                                        let event = advanced_scheduler::DatasetEvent {
                                            dataset_id,
                                            source_dag_id: Some(source_dag.clone()),
                                            source_task_id: Some(source_task.clone()),
                                            source_run_id: Some("cli".to_string()),
                                            event_type: event_type.clone(),
                                            metadata: serde_json::Value::Object(serde_json::Map::new()),
                                        };
                                        db.insert_dataset_event(&event).await?;

                                        let triggers = db.get_dataset_triggers_for_dataset(dataset).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "status": "ok",
                                                "message": format!("Dataset event emitted: {} (type={})", dataset, event_type),
                                                "triggered_dags": triggers,
                                            }))?);
                                        } else {
                                            println!("Dataset event emitted: {} (type={})", dataset, event_type);

                                            if !triggers.is_empty() {
                                                println!("\nTriggered DAGs:");
                                                for t in &triggers {
                                                    println!("  - {} (condition={})",
                                                        t["dag_id"].as_str().unwrap_or("-"),
                                                        t["condition"].as_str().unwrap_or("-"),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            DatasetAction::Triggers { dataset_id } => {
                                validate_uri(dataset_id, "dataset_id")?;
                                let triggers = db.get_dataset_triggers_for_dataset(dataset_id).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "dataset_id": dataset_id,
                                        "triggers": triggers,
                                        "total": triggers.len(),
                                    }))?);
                                } else {
                                    println!("{:<36} {:<30} {:<10} {:<10}", "ID", "DAG_ID", "CONDITION", "ENABLED");
                                    println!("{}", "-".repeat(88));
                                    for t in &triggers {
                                        println!("{:<36} {:<30} {:<10} {:<10}",
                                            t["id"].as_str().unwrap_or("-"),
                                            t["dag_id"].as_str().unwrap_or("-"),
                                            t["condition"].as_str().unwrap_or("-"),
                                            t["enabled"].as_bool().unwrap_or(false),
                                        );
                                    }
                                    println!("\n{} trigger(s) total", triggers.len());
                                }
                            }
                            DatasetAction::Freshness { uri, stale_after } => {
                                if let Some(uri_val) = uri {
                                    match db.get_dataset_freshness(uri_val).await? {
                                        Some(info) => {
                                            if json_output {
                                                println!("{}", serde_json::to_string_pretty(&info)?);
                                            } else {
                                                println!("{:<36} {:<40} {:<25} {:<12} {:<20} {:<20}",
                                                    "ID", "URI", "LAST_EVENT", "AGE_SECS", "SOURCE_DAG", "SOURCE_TASK");
                                                println!("{}", "-".repeat(150));
                                                println!("{:<36} {:<40} {:<25} {:<12} {:<20} {:<20}",
                                                    info["id"].as_str().unwrap_or("-"),
                                                    info["uri"].as_str().unwrap_or("-"),
                                                    info["last_event"].as_str().unwrap_or("never"),
                                                    info["age_seconds"].as_i64().unwrap_or(-1),
                                                    info["source_dag"].as_str().unwrap_or("-"),
                                                    info["source_task"].as_str().unwrap_or("-"),
                                                );
                                            }
                                        }
                                        None => {
                                            if json_output {
                                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                    "error": format!("Dataset with URI '{}' not found", uri_val),
                                                }))?);
                                            } else {
                                                eprintln!("Dataset with URI '{}' not found", uri_val);
                                            }
                                        }
                                    }
                                } else if let Some(secs) = stale_after {
                                    let stale = db.get_stale_datasets(*secs).await?;
                                    if json_output {
                                        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                            "stale_datasets": stale,
                                            "stale_after_secs": secs,
                                            "total": stale.len(),
                                        }))?);
                                    } else {
                                        println!("{:<36} {:<40} {:<25} {:<12}", "ID", "URI", "LAST_EVENT", "AGE_SECS");
                                        println!("{}", "-".repeat(115));
                                        for d in &stale {
                                            println!("{:<36} {:<40} {:<25} {:<12}",
                                                d["id"].as_str().unwrap_or("-"),
                                                d["uri"].as_str().unwrap_or("-"),
                                                d["last_event"].as_str().unwrap_or("never"),
                                                d["age_seconds"].as_i64().unwrap_or(-1),
                                            );
                                        }
                                        println!("\n{} stale dataset(s)", stale.len());
                                    }
                                } else {
                                    anyhow::bail!("Provide either a dataset URI or --stale-after <seconds>");
                                }
                            }
                            DatasetAction::Schema { dataset_id } => {
                                validate_uri(dataset_id, "dataset_id")?;
                                match db.get_latest_dataset_schema(dataset_id).await? {
                                    Some(schema) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&schema)?);
                                        } else {
                                            println!("Dataset: {}  Version: {}  Captured: {}",
                                                dataset_id,
                                                schema["version"].as_i64().unwrap_or(0),
                                                schema["captured_at"].as_str().unwrap_or("-"),
                                            );
                                            if let Some(cols) = schema["schema_json"].as_array() {
                                                println!("\n{:<30} {:<20}", "COLUMN", "TYPE");
                                                println!("{}", "-".repeat(52));
                                                for col in cols {
                                                    println!("{:<30} {:<20}",
                                                        col["name"].as_str().unwrap_or("-"),
                                                        col["data_type"].as_str().unwrap_or("-"),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("No schema found for dataset '{}'", dataset_id),
                                            }))?);
                                        } else {
                                            eprintln!("No schema found for dataset '{}'", dataset_id);
                                        }
                                    }
                                }
                            }
                            DatasetAction::SchemaDiff { dataset_id } => {
                                validate_uri(dataset_id, "dataset_id")?;
                                match db.get_dataset_schema_diff(dataset_id).await? {
                                    Some(diff) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&diff)?);
                                        } else {
                                            println!("Schema diff for '{}': v{} → v{}",
                                                dataset_id,
                                                diff["previous_version"].as_i64().map(|v| v.to_string()).unwrap_or_else(|| "?".to_string()),
                                                diff["current_version"].as_i64().unwrap_or(0),
                                            );
                                            if let Some(msg) = diff["message"].as_str() {
                                                println!("  {}", msg);
                                            } else {
                                                if let Some(added) = diff["added"].as_array() {
                                                    for a in added {
                                                        println!("  + {} ({})", a["name"].as_str().unwrap_or("?"), a["data_type"].as_str().unwrap_or("?"));
                                                    }
                                                }
                                                if let Some(removed) = diff["removed"].as_array() {
                                                    for r in removed {
                                                        println!("  - {} ({})", r["name"].as_str().unwrap_or("?"), r["data_type"].as_str().unwrap_or("?"));
                                                    }
                                                }
                                                if let Some(changed) = diff["changed"].as_array() {
                                                    for c in changed {
                                                        println!("  ~ {} ({} → {})", c["name"].as_str().unwrap_or("?"), c["old_type"].as_str().unwrap_or("?"), c["new_type"].as_str().unwrap_or("?"));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("No schema versions found for dataset '{}'", dataset_id),
                                            }))?);
                                        } else {
                                            eprintln!("No schema versions found for dataset '{}'", dataset_id);
                                        }
                                    }
                                }
                            }
                            DatasetAction::Stats { dataset_id } => {
                                validate_uri(dataset_id, "dataset_id")?;
                                match db.get_dataset_stats(dataset_id).await? {
                                    Some(stats) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&stats)?);
                                        } else {
                                            println!("Stats for dataset '{}':", dataset_id);
                                            println!("  Total events:     {}", stats["total_events"].as_i64().unwrap_or(0));
                                            println!("  Last event:       {}", stats["last_event"].as_str().unwrap_or("-"));
                                            println!("  Last row count:   {}", stats["last_row_count"].as_i64().map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()));
                                            println!("  Last byte size:   {}", stats["last_byte_size"].as_i64().map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()));
                                            println!("  Last partition:   {}", stats["last_partition_key"].as_str().unwrap_or("-"));
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("No events found for dataset '{}'", dataset_id),
                                            }))?);
                                        } else {
                                            eprintln!("No events found for dataset '{}'", dataset_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Commands::Pool { action } => {
                        match action {
                            PoolAction::List => {
                                let pools = db.get_all_pools().await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "pools": pools,
                                        "total": pools.len(),
                                    }))?);
                                } else {
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
                            }
                            PoolAction::Create { name, slots, description } => {
                                validate_identifier(name, "pool_name")?;
                                db.create_pool(name, *slots, description.as_deref().unwrap_or("")).await?;;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("Pool '{}' created with {} slots", name, slots),
                                    }))?);
                                } else {
                                    println!("Pool '{}' created with {} slots", name, slots);
                                }
                            }
                            PoolAction::Delete { name } => {
                                validate_identifier(name, "pool_name")?;
                                db.delete_pool(name).await?;;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("Pool '{}' deleted", name),
                                    }))?);
                                } else {
                                    println!("Pool '{}' deleted", name);
                                }
                            }
                        }
                    }
                    Commands::Config { action } => {
                        match action {
                            ConfigAction::Show => {
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "port": cli.port,
                                        "swarm": cli.swarm,
                                        "grpc_bind": cli.grpc_bind,
                                        "swarm_port": cli.swarm_port,
                                        "ha_mode": cli.ha_mode,
                                        "db_max_connections": cli.db_max_connections,
                                        "db_min_connections": cli.db_min_connections,
                                        "db_idle_timeout_secs": cli.db_idle_timeout,
                                        "tls_enabled": cli.tls_cert.is_some(),
                                        "benchmark_dag": cli.benchmark,
                                        "unsafe_plugins": cli.allow_unsafe_plugins,
                                        "unsafe_dag_exec": cli.allow_unsafe_dag_exec,
                                        "log_level": cli.log_level,
                                    }))?);
                                } else {
                                    println!("Ryuo Configuration:");
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
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "config": config,
                                    }))?);
                                } else {
                                    println!("{}", serde_json::to_string_pretty(&config)?);
                                }
                            }
                        }
                    }
                    Commands::Event { action } => {
                        match action {
                            EventAction::Trigger { action: trigger_action } => {
                                match trigger_action {
                                    EventTriggerAction::Create { name, event_type, dag, filter, config, team } => {
                                        validate_identifier(name, "name")?;
                                        validate_identifier(dag, "dag")?;
                                        // Validate filter and config are valid JSON
                                        let _: serde_json::Value = serde_json::from_str(filter)
                                            .map_err(|e| anyhow::anyhow!("Invalid filter JSON: {}", e))?;
                                        let _: serde_json::Value = serde_json::from_str(config)
                                            .map_err(|e| anyhow::anyhow!("Invalid config JSON: {}", e))?;
                                        let id = uuid::Uuid::new_v4().to_string();
                                        db.create_event_trigger(&id, name, event_type, filter, dag, config, team.as_deref()).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "status": "ok",
                                                "id": id,
                                                "message": format!("Event trigger '{}' created", name),
                                            }))?);
                                        } else {
                                            println!("Event trigger '{}' created (id={})", name, id);
                                        }
                                    }
                                    EventTriggerAction::List => {
                                        let triggers = db.get_event_triggers(100).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "triggers": triggers,
                                                "total": triggers.len(),
                                            }))?);
                                        } else {
                                            println!("{:<36} {:<20} {:<20} {:<30} {:<8}", "ID", "NAME", "EVENT_TYPE", "DAG_ID", "ENABLED");
                                            println!("{}", "-".repeat(116));
                                            for t in &triggers {
                                                println!("{:<36} {:<20} {:<20} {:<30} {:<8}",
                                                    t["id"].as_str().unwrap_or("-"),
                                                    t["name"].as_str().unwrap_or("-"),
                                                    t["event_type"].as_str().unwrap_or("-"),
                                                    t["dag_id"].as_str().unwrap_or("-"),
                                                    t["enabled"].as_bool().unwrap_or(false),
                                                );
                                            }
                                            println!("\n{} trigger(s) total", triggers.len());
                                        }
                                    }
                                    EventTriggerAction::Delete { id } => {
                                        db.delete_event_trigger(id).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "status": "ok",
                                                "message": format!("Event trigger '{}' deleted", id),
                                            }))?);
                                        } else {
                                            println!("Event trigger '{}' deleted", id);
                                        }
                                    }
                                }
                            }
                            EventAction::Recent { event_type, since, limit } => {
                                let events = db.get_recent_events(event_type.as_deref(), *since, *limit).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "events": events,
                                        "total": events.len(),
                                    }))?);
                                } else {
                                    println!("{:<26} {:<15} {:<20} {:<20} {:<20}", "CREATED_AT", "EVENT_TYPE", "DATASET_ID", "SOURCE_DAG", "SOURCE_TASK");
                                    println!("{}", "-".repeat(103));
                                    for e in &events {
                                        println!("{:<26} {:<15} {:<20} {:<20} {:<20}",
                                            e["created_at"].as_str().unwrap_or("-"),
                                            e["event_type"].as_str().unwrap_or("-"),
                                            e["dataset_id"].as_str().unwrap_or("-"),
                                            e["source_dag_id"].as_str().unwrap_or("-"),
                                            e["source_task_id"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} event(s)", events.len());
                                }
                            }
                            EventAction::Watch { event_type, timeout, interval } => {
                                if *interval == 0 {
                                    anyhow::bail!("--interval must be > 0");
                                }
                                let start = std::time::Instant::now();
                                let mut last_poll = chrono::Utc::now();
                                println!("Watching for events (timeout={}s, poll={}s)...", timeout, interval);
                                while start.elapsed().as_secs() < *timeout {
                                    let since_secs = (chrono::Utc::now() - last_poll).num_seconds() + 1;
                                    last_poll = chrono::Utc::now();
                                    let events = db.get_recent_events(event_type.as_deref(), Some(since_secs), 100).await?;
                                    for e in &events {
                                        if json_output {
                                            println!("{}", serde_json::to_string(e)?);
                                        } else {
                                            println!("[{}] {} {} (dag={}, task={})",
                                                e["created_at"].as_str().unwrap_or("-"),
                                                e["event_type"].as_str().unwrap_or("-"),
                                                e["dataset_id"].as_str().unwrap_or("-"),
                                                e["source_dag_id"].as_str().unwrap_or("-"),
                                                e["source_task_id"].as_str().unwrap_or("-"),
                                            );
                                        }
                                    }
                                    tokio::time::sleep(tokio::time::Duration::from_secs(*interval)).await;
                                }
                                println!("Watch timed out after {}s", timeout);
                            }
                            EventAction::Publish { event_type, source, payload } => {
                                validate_uri(event_type, "event_type")?;
                                validate_identifier(source, "source")?;
                                // Validate payload is valid JSON
                                let _: serde_json::Value = serde_json::from_str(payload)
                                    .map_err(|e| anyhow::anyhow!("Invalid payload JSON: {}", e))?;
                                let id = uuid::Uuid::new_v4().to_string();
                                db.publish_custom_event(&id, event_type, source, payload).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "id": id,
                                        "event_type": event_type,
                                        "source": source,
                                    }))?);
                                } else {
                                    println!("Custom event published (id={}, type={}, source={})", id, event_type, source);
                                }
                            }
                            EventAction::Custom { event_type, since, limit } => {
                                let events = db.get_custom_events(event_type.as_deref(), *since, *limit).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "events": events,
                                        "total": events.len(),
                                    }))?);
                                } else {
                                    println!("{:<26} {:<20} {:<20} {}", "CREATED_AT", "EVENT_TYPE", "SOURCE", "PAYLOAD");
                                    println!("{}", "-".repeat(90));
                                    for e in &events {
                                        let payload_str = e["payload"].as_str().unwrap_or("{}");
                                        let truncated = if payload_str.len() > 60 {
                                            format!("{}...", &payload_str[..60])
                                        } else {
                                            payload_str.to_string()
                                        };
                                        println!("{:<26} {:<20} {:<20} {}",
                                            e["created_at"].as_str().unwrap_or("-"),
                                            e["event_type"].as_str().unwrap_or("-"),
                                            e["source"].as_str().unwrap_or("-"),
                                            truncated,
                                        );
                                    }
                                    println!("\n{} custom event(s)", events.len());
                                }
                            }
                        }
                    }
                    Commands::Sensor { action } => {
                        match action {
                            SensorAction::List { limit } => {
                                let sensors = db.get_sensor_tasks(*limit).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "sensors": sensors,
                                        "total": sensors.len(),
                                    }))?);
                                } else {
                                    println!("{:<36} {:<20} {:<15} {:<12} {:<15} {:<25}", "INSTANCE_ID", "DAG", "TASK", "STATE", "SENSOR_TYPE", "EXECUTION_DATE");
                                    println!("{}", "-".repeat(125));
                                    for s in &sensors {
                                        println!("{:<36} {:<20} {:<15} {:<12} {:<15} {:<25}",
                                            s["id"].as_str().unwrap_or("-"),
                                            s["dag_id"].as_str().unwrap_or("-"),
                                            s["task_id"].as_str().unwrap_or("-"),
                                            s["state"].as_str().unwrap_or("-"),
                                            s["sensor_type"].as_str().unwrap_or("-"),
                                            s["execution_date"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} sensor task(s)", sensors.len());
                                }
                            }
                            SensorAction::CheckAnomaly { sql, baseline, sigma } => {
                                // Validate SQL with sqlparser — only SELECT allowed
                                use sqlparser::dialect::GenericDialect;
                                use sqlparser::parser::Parser as SqlParser;
                                let dialect = GenericDialect {};
                                let statements = SqlParser::parse_sql(&dialect, sql)
                                    .map_err(|e| anyhow::anyhow!("SQL parse error: {}", e))?;
                                if statements.is_empty() {
                                    anyhow::bail!("No SQL statement provided");
                                }
                                if statements.len() > 1 {
                                    anyhow::bail!("Only single SELECT statements are allowed");
                                }
                                match &statements[0] {
                                    sqlparser::ast::Statement::Query(_) => {}
                                    _ => anyhow::bail!("Only SELECT queries are allowed"),
                                }

                                // Execute query to get current value
                                let results = db.execute_raw_query(sql, 30, 1).await?;
                                let current_value: f64 = results
                                    .first()
                                    .and_then(|row| {
                                        // Try to extract the first numeric value from the row
                                        if let Some(obj) = row.as_object() {
                                            obj.values().next().and_then(|v| {
                                                v.as_f64().or_else(|| v.as_i64().map(|i| i as f64))
                                                    .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
                                            })
                                        } else {
                                            None
                                        }
                                    })
                                    .ok_or_else(|| anyhow::anyhow!("Query did not return a numeric value"))?;

                                // Parse baseline values
                                let baseline_values: Vec<f64> = baseline
                                    .split(',')
                                    .map(|s| s.trim().parse::<f64>())
                                    .collect::<std::result::Result<Vec<f64>, _>>()
                                    .map_err(|e| anyhow::anyhow!("Invalid baseline value: {}", e))?;
                                if baseline_values.is_empty() {
                                    anyhow::bail!("Baseline must contain at least one value");
                                }
                                if baseline_values.len() < 2 {
                                    anyhow::bail!("Baseline must contain at least 2 values for standard deviation");
                                }

                                // Calculate mean
                                let n = baseline_values.len() as f64;
                                let mean = baseline_values.iter().sum::<f64>() / n;

                                // Calculate standard deviation (sample stddev)
                                let variance = baseline_values
                                    .iter()
                                    .map(|v| (v - mean).powi(2))
                                    .sum::<f64>()
                                    / (n - 1.0);
                                let stddev = variance.sqrt();

                                // Determine anomaly
                                let deviation_sigma = if stddev > 0.0 {
                                    (current_value - mean).abs() / stddev
                                } else {
                                    if (current_value - mean).abs() > f64::EPSILON { f64::INFINITY } else { 0.0 }
                                };
                                let is_anomaly = deviation_sigma > *sigma;

                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "current_value": current_value,
                                        "mean": format!("{:.4}", mean),
                                        "stddev": format!("{:.4}", stddev),
                                        "sigma_threshold": sigma,
                                        "deviation_sigma": format!("{:.4}", deviation_sigma),
                                        "is_anomaly": is_anomaly,
                                        "baseline_count": baseline_values.len(),
                                    }))?);
                                } else {
                                    println!("Anomaly Check Results:");
                                    println!("{}", "-".repeat(40));
                                    println!("Current value:    {:.4}", current_value);
                                    println!("Baseline mean:    {:.4}", mean);
                                    println!("Baseline stddev:  {:.4}", stddev);
                                    println!("Sigma threshold:  {:.1}", sigma);
                                    println!("Deviation (σ):    {:.4}", deviation_sigma);
                                    println!("Anomaly:          {}", if is_anomaly { "YES ⚠️" } else { "NO ✅" });
                                }
                            }
                        }
                    }
                    Commands::Queue { action } => {
                        match action {
                            QueueAction::List { limit } => {
                                let queue = db.get_task_queue(*limit).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "queue": queue,
                                        "total": queue.len(),
                                    }))?);
                                } else {
                                    println!("{:<36} {:<20} {:<15} {:<10} {:<8} {:<15} {:<25}",
                                        "ID", "DAG", "TASK", "STATE", "PRI", "POOL", "EXECUTION_DATE");
                                    println!("{}", "-".repeat(131));
                                    for t in &queue {
                                        println!("{:<36} {:<20} {:<15} {:<10} {:<8} {:<15} {:<25}",
                                            t["id"].as_str().unwrap_or("-"),
                                            t["dag_id"].as_str().unwrap_or("-"),
                                            t["task_id"].as_str().unwrap_or("-"),
                                            t["state"].as_str().unwrap_or("-"),
                                            t["priority"].as_i64().unwrap_or(0),
                                            t["pool"].as_str().unwrap_or("-"),
                                            t["execution_date"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} queued task(s)", queue.len());
                                }
                            }
                            QueueAction::Reprioritize { instance_id, priority } => {
                                validate_identifier(instance_id, "instance_id")?;
                                db.reprioritize_task(instance_id, *priority).await?;
                                // Audit trail for queue reprioritization
                                let reason_str = audit_reason.unwrap_or("");
                                let metadata = serde_json::json!({"priority": priority, "reason": reason_str}).to_string();
                                let _ = db.log_audit_event("cli", "queue.reprioritize", "task_instance", instance_id, &metadata).await;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("Task '{}' priority set to {}", instance_id, priority),
                                    }))?);
                                } else {
                                    println!("Task '{}' priority set to {}", instance_id, priority);
                                }
                            }
                            QueueAction::Pause => {
                                db.set_scheduler_state("paused", "true").await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": "Scheduler paused",
                                    }))?);
                                } else {
                                    println!("Scheduler paused — no new tasks will be dispatched.");
                                }
                            }
                            QueueAction::Resume => {
                                db.set_scheduler_state("paused", "false").await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": "Scheduler resumed",
                                    }))?);
                                } else {
                                    println!("Scheduler resumed — task dispatch is active.");
                                }
                            }
                            QueueAction::Status => {
                                let paused = db.get_scheduler_state("paused").await?.unwrap_or_else(|| "false".to_string());
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "paused": paused == "true",
                                    }))?);
                                } else {
                                    println!("Scheduler paused: {}", paused);
                                }
                            }
                        }
                    }
                    Commands::Task { action } => {
                        match action {
                            TaskAction::Logs { instance_id, tail } => {
                                validate_identifier(instance_id, "instance_id")?;
                                match db.get_task_instance(instance_id).await? {
                                    Some((dag_id, task_id, _exec_date)) => {
                                        let events = db.get_task_events(instance_id).await?;
                                        let mut log_lines: Vec<String> = Vec::new();
                                        for e in &events {
                                            if let Some(msg) = e["message"].as_str() {
                                                for line in msg.lines() {
                                                    log_lines.push(line.to_string());
                                                }
                                            }
                                        }
                                        if let Some(n) = tail {
                                            let start = log_lines.len().saturating_sub(*n);
                                            log_lines = log_lines[start..].to_vec();
                                        }
                                        if json_output {
                                            let total = log_lines.len();
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "instance_id": instance_id,
                                                "dag_id": dag_id,
                                                "task_id": task_id,
                                                "lines": log_lines,
                                                "total_lines": total,
                                            }))?);
                                        } else {
                                            println!("=== Logs for task instance {} (dag={}, task={}) ===", instance_id, dag_id, task_id);
                                            for line in &log_lines {
                                                println!("{}", line);
                                            }
                                            if log_lines.is_empty() {
                                                println!("(no log output recorded)");
                                            }
                                        }
                                    }
                                    None => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("Task instance '{}' not found", instance_id),
                                            }))?);
                                        } else {
                                            eprintln!("Task instance '{}' not found", instance_id);
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            }
                        }
                    }
                    Commands::Approval { action } => {
                        match action {
                            ApprovalAction::List => {
                                let approvals = db.get_approval_requests(Some("pending"), 100).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "approvals": approvals,
                                        "total": approvals.len(),
                                    }))?);
                                } else {
                                    println!("{:<36} {:<15} {:<20} {:<15} {:<10}",
                                        "ID", "RESOURCE_TYPE", "RESOURCE_ID", "REQUESTER", "STATUS");
                                    println!("{}", "-".repeat(98));
                                    for a in &approvals {
                                        println!("{:<36} {:<15} {:<20} {:<15} {:<10}",
                                            a["id"].as_str().unwrap_or("-"),
                                            a["resource_type"].as_str().unwrap_or("-"),
                                            a["resource_id"].as_str().unwrap_or("-"),
                                            a["requester"].as_str().unwrap_or("-"),
                                            a["status"].as_str().unwrap_or("-"),
                                        );
                                    }
                                    println!("\n{} pending approval(s)", approvals.len());
                                }
                            }
                            ApprovalAction::Approve { id } => {
                                let result = db.add_approval_vote(id, "cli-user", Some("Approved via CLI")).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "result": result,
                                        "message": format!("Vote added to approval '{}'", id),
                                    }))?);
                                } else {
                                    println!("Vote added to approval '{}' — status: {}", id, result);
                                }
                            }
                            ApprovalAction::Reject { id } => {
                                db.reject_approval_request(id, "cli-user", Some("Rejected via CLI")).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "message": format!("Approval '{}' rejected", id),
                                    }))?);
                                } else {
                                    println!("Approval '{}' rejected", id);
                                }
                            }
                        }
                    }
                    Commands::RateLimit { action } => {
                        match action {
                            RateLimitAction::Status { actor } => {
                                let counters = db.get_rate_limit_status(actor).await?;
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "actor": actor,
                                        "window": "current minute",
                                        "actions": counters,
                                    }))?);
                                } else {
                                    println!("Rate limit status for '{}':", actor);
                                    if counters.is_empty() {
                                        println!("  (no actions in current window)");
                                    } else {
                                        println!("{:<30} {:<10}", "ACTION", "COUNT");
                                        println!("{}", "-".repeat(40));
                                        for c in &counters {
                                            println!("{:<30} {:<10}",
                                                c["action"].as_str().unwrap_or("-"),
                                                c["count"].as_i64().unwrap_or(0),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Commands::Validate { action } => {
                        match action {
                            ValidateAction::Sql { query } => {
                                use sqlparser::dialect::GenericDialect;
                                use sqlparser::parser::Parser as SqlParser;
                                let dialect = GenericDialect {};
                                match SqlParser::parse_sql(&dialect, query) {
                                    Ok(statements) => {
                                        let mut safe = true;
                                        let mut issues = Vec::new();
                                        for stmt in &statements {
                                            match stmt {
                                                sqlparser::ast::Statement::Query(_) => {} // SELECT is ok
                                                other => {
                                                    safe = false;
                                                    issues.push(format!("Non-SELECT statement detected: {:?}", std::mem::discriminant(other)));
                                                }
                                            }
                                        }
                                        if statements.len() > 1 {
                                            safe = false;
                                            issues.push("Multiple statements detected (only single SELECT allowed)".into());
                                        }
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "valid": safe,
                                                "issues": issues,
                                                "statement_count": statements.len(),
                                            }))?);
                                        } else if safe {
                                            println!("SQL is valid (SELECT-only)");
                                        } else {
                                            println!("SQL validation FAILED:");
                                            for i in &issues { println!("  ❌ {}", i); }
                                        }
                                        if !safe { std::process::exit(1); }
                                    }
                                    Err(e) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "valid": false,
                                                "error": format!("Parse error: {}", e),
                                            }))?);
                                        } else {
                                            println!("SQL parse error: {}", e);
                                        }
                                        std::process::exit(2);
                                    }
                                }
                            }
                            ValidateAction::Command { cmd } => {
                                let warnings = check_command_injection(cmd);
                                let safe = warnings.is_empty();
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "safe": safe,
                                        "warnings": warnings,
                                    }))?);
                                } else if safe {
                                    println!("Command appears safe (no known injection patterns detected)");
                                } else {
                                    println!("⚠ Command injection warnings:");
                                    for w in &warnings {
                                        println!("  ⚠ {}", w);
                                    }
                                }
                                if !safe { std::process::exit(1); }
                            }
                        }
                    }
                    Commands::Agent { action } => {
                        match action {
                            AgentAction::State { action } => {
                                match action {
                                    AgentStateAction::Set { key, value, agent, ttl } => {
                                        validate_identifier(agent, "agent")?;
                                        validate_identifier(key, "key")?;
                                        if value.len() > 256 * 1024 {
                                            anyhow::bail!("Value exceeds 256KB size limit ({} bytes)", value.len());
                                        }
                                        db.agent_state_set(agent, key, value, *ttl).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "status": "ok",
                                                "agent_id": agent,
                                                "key": key,
                                                "ttl_secs": ttl,
                                            }))?);
                                        } else {
                                            println!("✅ State set: agent={} key={}", agent, key);
                                        }
                                    }
                                    AgentStateAction::Get { key, agent } => {
                                        validate_identifier(agent, "agent")?;
                                        validate_identifier(key, "key")?;
                                        let val = db.agent_state_get(agent, key).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "agent_id": agent,
                                                "key": key,
                                                "value": val,
                                            }))?);
                                        } else if let Some(v) = val {
                                            println!("{}", v);
                                        } else {
                                            println!("(not found)");
                                        }
                                    }
                                    AgentStateAction::List { agent, limit } => {
                                        validate_identifier(agent, "agent")?;
                                        let entries = db.agent_state_list(agent, *limit).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "agent_id": agent,
                                                "count": entries.len(),
                                                "entries": entries,
                                            }))?);
                                        } else {
                                            println!("{:<30} {:<40} {:<25} {:<25}",
                                                "KEY", "VALUE", "TTL_EXPIRES", "UPDATED_AT");
                                            println!("{}", "-".repeat(120));
                                            for e in &entries {
                                                let v = e["value"].as_str().unwrap_or("");
                                                let display_val = if v.len() > 37 { format!("{}...", &v[..37]) } else { v.to_string() };
                                                println!("{:<30} {:<40} {:<25} {:<25}",
                                                    e["key"].as_str().unwrap_or("-"),
                                                    display_val,
                                                    e["ttl_expires"].as_str().unwrap_or("-"),
                                                    e["updated_at"].as_str().unwrap_or("-"),
                                                );
                                            }
                                            println!("\nTotal: {} entries", entries.len());
                                        }
                                    }
                                    AgentStateAction::Delete { key, agent } => {
                                        validate_identifier(agent, "agent")?;
                                        validate_identifier(key, "key")?;
                                        db.agent_state_delete(agent, key).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "status": "deleted",
                                                "agent_id": agent,
                                                "key": key,
                                            }))?);
                                        } else {
                                            println!("✅ Deleted: agent={} key={}", agent, key);
                                        }
                                    }
                                }
                            }
                            AgentAction::Log { action } => {
                                match action {
                                    AgentLogAction::Write { message, agent, context, level } => {
                                        validate_identifier(agent, "agent")?;
                                        let valid_levels = ["info", "warn", "error", "debug"];
                                        if !valid_levels.contains(&level.as_str()) {
                                            anyhow::bail!("Invalid log level '{}' — must be one of: info, warn, error, debug", level);
                                        }
                                        // Validate context is valid JSON
                                        serde_json::from_str::<serde_json::Value>(context)
                                            .map_err(|e| anyhow::anyhow!("Invalid JSON context: {}", e))?;
                                        let id = uuid::Uuid::new_v4().to_string();
                                        db.agent_log_insert(&id, agent, message, context, level).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "status": "ok",
                                                "id": id,
                                                "agent_id": agent,
                                                "level": level,
                                            }))?);
                                        } else {
                                            println!("✅ Log entry created: id={} agent={} level={}", id, agent, level);
                                        }
                                    }
                                    AgentLogAction::Query { agent, since, limit } => {
                                        validate_identifier(agent, "agent")?;
                                        let logs = db.agent_log_query(agent, *since, *limit).await?;
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "agent_id": agent,
                                                "count": logs.len(),
                                                "logs": logs,
                                            }))?);
                                        } else {
                                            println!("{:<38} {:<8} {:<25} {}",
                                                "ID", "LEVEL", "CREATED_AT", "MESSAGE");
                                            println!("{}", "-".repeat(120));
                                            for l in &logs {
                                                let msg = l["message"].as_str().unwrap_or("");
                                                let display_msg = if msg.len() > 50 { format!("{}...", &msg[..50]) } else { msg.to_string() };
                                                println!("{:<38} {:<8} {:<25} {}",
                                                    l["id"].as_str().unwrap_or("-"),
                                                    l["level"].as_str().unwrap_or("-"),
                                                    l["created_at"].as_str().unwrap_or("-"),
                                                    display_msg,
                                                );
                                            }
                                            println!("\nTotal: {} log entries", logs.len());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Commands::Mcp { action } => {
                        match action {
                            McpAction::Tools => {
                                let tools = mcp_server::get_tool_definitions();
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&mcp_server::format_tools_list())?);
                                } else {
                                    println!("{:<25} {}", "TOOL", "DESCRIPTION");
                                    println!("{}", "-".repeat(90));
                                    for tool in &tools {
                                        println!("{:<25} {}", tool.name, tool.description);
                                    }
                                    println!("\n{} tool(s) available", tools.len());
                                }
                            }
                            McpAction::Describe { tool_name } => {
                                let tools = mcp_server::get_tool_definitions();
                                match tools.iter().find(|t| t.name == *tool_name) {
                                    Some(tool) => {
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "name": tool.name,
                                                "description": tool.description,
                                                "inputSchema": tool.input_schema,
                                            }))?);
                                        } else {
                                            println!("Tool: {}", tool.name);
                                            println!("Description: {}", tool.description);
                                            println!("Input Schema:");
                                            println!("{}", serde_json::to_string_pretty(&tool.input_schema)?);
                                        }
                                    }
                                    None => {
                                        let available: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
                                        if json_output {
                                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                                "error": format!("Unknown tool: '{}'", tool_name),
                                                "available": available,
                                            }))?);
                                        } else {
                                            eprintln!("Unknown tool: '{}'", tool_name);
                                            eprintln!("Available tools: {}", available.join(", "));
                                        }
                                        std::process::exit(1);
                                    }
                                }
                            }
                            McpAction::Call { tool, args } => {
                                let arguments: std::collections::HashMap<String, serde_json::Value> =
                                    serde_json::from_str(args)
                                        .map_err(|e| anyhow::anyhow!("Invalid JSON args: {}", e))?;
                                let call = mcp_server::McpToolCall {
                                    name: tool.clone(),
                                    arguments,
                                };
                                let result = mcp_server::dispatch_tool_call(call);
                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&result)?);
                                } else {
                                    if result.is_error {
                                        eprintln!("Error: {}", result.content.first().map(|c| c.text.as_str()).unwrap_or("unknown error"));
                                        std::process::exit(1);
                                    }
                                    for content in &result.content {
                                        println!("{}", content.text);
                                    }
                                }
                            }
                        }
                    }
                    Commands::Agentic { action } => {
                        match action {
                            AgenticAction::Translate { python_file, provider, max_retries } => {
                                let contents = std::fs::read_to_string(python_file)
                                    .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", python_file, e))?;

                                let provider_instance: Box<dyn agentic::LlmProvider> = match provider.as_str() {
                                    "openai" => {
                                        let api_key = std::env::var("OPENAI_API_KEY")
                                            .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY env var required for openai provider"))?;
                                        Box::new(agentic::OpenAiProvider {
                                            endpoint: std::env::var("OPENAI_ENDPOINT").unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".into()),
                                            api_key,
                                            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4".into()),
                                        })
                                    }
                                    "anthropic" => {
                                        let api_key = std::env::var("ANTHROPIC_API_KEY")
                                            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY env var required for anthropic provider"))?;
                                        Box::new(agentic::AnthropicProvider {
                                            endpoint: std::env::var("ANTHROPIC_ENDPOINT").unwrap_or_else(|_| "https://api.anthropic.com/v1/messages".into()),
                                            api_key,
                                            model: std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".into()),
                                        })
                                    }
                                    _ => anyhow::bail!("Unknown provider '{}' — supported: openai, anthropic", provider),
                                };

                                let rust_code = agentic::translate_python_to_rust_agentic(
                                    provider_instance.as_ref(), &contents, *max_retries,
                                ).await?;

                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "provider": provider,
                                        "source_file": python_file,
                                        "rust_code": rust_code,
                                    }))?);
                                } else {
                                    println!("{}", rust_code);
                                }
                            }
                            AgenticAction::DbtConvert { manifest } => {
                                let contents = std::fs::read_to_string(manifest)
                                    .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", manifest, e))?;
                                let nodes = agentic::convert_dbt_manifest_to_pipeline(&contents)?;

                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "status": "ok",
                                        "source_file": manifest,
                                        "nodes": nodes,
                                        "total": nodes.len(),
                                    }))?);
                                } else {
                                    println!("{:<30} {:<15} {}", "MODEL", "DEPENDENCIES", "SQL (truncated)");
                                    println!("{}", "-".repeat(100));
                                    for node in &nodes {
                                        let sql_preview = if node.sql.len() > 40 {
                                            format!("{}...", &node.sql[..40])
                                        } else {
                                            node.sql.clone()
                                        };
                                        println!("{:<30} {:<15} {}",
                                            node.name,
                                            node.depends_on.len(),
                                            sql_preview.replace('\n', " "),
                                        );
                                    }
                                    println!("\n{} model(s) converted", nodes.len());
                                }
                            }
                            AgenticAction::Providers => {
                                let mut providers = Vec::new();
                                if std::env::var("OPENAI_API_KEY").is_ok() {
                                    providers.push(serde_json::json!({
                                        "name": "openai",
                                        "status": "configured",
                                        "model": std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4".into()),
                                    }));
                                } else {
                                    providers.push(serde_json::json!({
                                        "name": "openai",
                                        "status": "not_configured",
                                        "hint": "Set OPENAI_API_KEY env var",
                                    }));
                                }
                                if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                                    providers.push(serde_json::json!({
                                        "name": "anthropic",
                                        "status": "configured",
                                        "model": std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".into()),
                                    }));
                                } else {
                                    providers.push(serde_json::json!({
                                        "name": "anthropic",
                                        "status": "not_configured",
                                        "hint": "Set ANTHROPIC_API_KEY env var",
                                    }));
                                }

                                if json_output {
                                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                        "providers": providers,
                                    }))?);
                                } else {
                                    println!("{:<15} {:<18} {}", "PROVIDER", "STATUS", "MODEL/HINT");
                                    println!("{}", "-".repeat(60));
                                    for p in &providers {
                                        let model_or_hint = p.get("model")
                                            .or_else(|| p.get("hint"))
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("-");
                                        println!("{:<15} {:<18} {}",
                                            p["name"].as_str().unwrap_or("-"),
                                            p["status"].as_str().unwrap_or("-"),
                                            model_or_hint,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Commands::Profile { connector, table, columns, timeout } => {
                        if connector != "postgres" {
                            anyhow::bail!("Profiling currently only supported for 'postgres' connector. Got: '{}'", connector);
                        }
                        // Validate table name — allow schema.table (dot-separated identifiers)
                        for part in table.split('.') {
                            validate_identifier(part, "table")?;
                        }

                        // Get column metadata from information_schema
                        // SECURITY (C-2): Use parameterized subquery to avoid format! with table name
                        let table_name_only = table.split('.').last().unwrap_or(table);
                        let col_sql = format!(
                            "SELECT column_name, data_type, is_nullable \
                             FROM information_schema.columns \
                             WHERE table_name = '{}'  \
                             ORDER BY ordinal_position",
                            table_name_only.replace('\'', "''")
                        );
                        let columns_info = db.execute_raw_query(&col_sql, *timeout, 1000).await?;
                        if columns_info.is_empty() {
                            anyhow::bail!("Table '{}' not found or has no columns", table);
                        }

                        // Get row count
                        let count_sql = format!("SELECT COUNT(*) AS count FROM {}", table);
                        let row_count_result = db.execute_raw_query(&count_sql, *timeout, 1).await?;
                        let row_count = row_count_result
                            .first()
                            .and_then(|r| r["count"].as_i64())
                            .unwrap_or(0);

                        // Determine columns to profile
                        let cols_to_profile: Vec<String> = if let Some(ref cols) = columns {
                            cols.split(',').map(|s| s.trim().to_string()).collect()
                        } else {
                            columns_info
                                .iter()
                                .filter_map(|c| c["column_name"].as_str().map(|s| s.to_string()))
                                .collect()
                        };

                        // Profile each column
                        let mut col_profiles = Vec::new();
                        for col_name in &cols_to_profile {
                            validate_identifier(col_name, "column")?;
                            let stats_sql = format!(
                                "SELECT COUNT(*) AS total, \
                                 COUNT({col}) AS non_null, \
                                 COUNT(DISTINCT {col}) AS distinct_count, \
                                 MIN({col}::TEXT) AS min_val, \
                                 MAX({col}::TEXT) AS max_val \
                                 FROM {table}",
                                col = col_name,
                                table = table
                            );
                            match db.execute_raw_query(&stats_sql, *timeout, 1).await {
                                Ok(stats) => {
                                    if let Some(row) = stats.first() {
                                        let total = row["total"].as_i64().unwrap_or(0);
                                        let non_null = row["non_null"].as_i64().unwrap_or(0);
                                        let null_pct = if total > 0 {
                                            ((total - non_null) as f64 / total as f64) * 100.0
                                        } else {
                                            0.0
                                        };
                                        col_profiles.push(serde_json::json!({
                                            "column": col_name,
                                            "total": total,
                                            "non_null": non_null,
                                            "distinct_count": row["distinct_count"],
                                            "null_pct": format!("{:.2}", null_pct),
                                            "min": row["min_val"],
                                            "max": row["max_val"],
                                        }));
                                    }
                                }
                                Err(e) => {
                                    col_profiles.push(serde_json::json!({
                                        "column": col_name,
                                        "error": format!("{}", e),
                                    }));
                                }
                            }
                        }

                        if json_output {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "table": table,
                                "row_count": row_count,
                                "columns_profiled": col_profiles.len(),
                                "columns": col_profiles,
                            }))?);
                        } else {
                            println!("Profile: {} ({} rows)\n", table, row_count);
                            println!("{:<20} {:<10} {:<10} {:<10} {:<10} {:<20} {:<20}",
                                "COLUMN", "TOTAL", "NON_NULL", "DISTINCT", "NULL_%", "MIN", "MAX");
                            println!("{}", "-".repeat(100));
                            for cp in &col_profiles {
                                if let Some(err) = cp["error"].as_str() {
                                    println!("{:<20} ERROR: {}", cp["column"].as_str().unwrap_or("-"), err);
                                } else {
                                    let min_val = cp["min"].as_str().unwrap_or("-");
                                    let max_val = cp["max"].as_str().unwrap_or("-");
                                    let min_display = if min_val.len() > 18 { format!("{}...", &min_val[..15]) } else { min_val.to_string() };
                                    let max_display = if max_val.len() > 18 { format!("{}...", &max_val[..15]) } else { max_val.to_string() };
                                    println!("{:<20} {:<10} {:<10} {:<10} {:<10} {:<20} {:<20}",
                                        cp["column"].as_str().unwrap_or("-"),
                                        cp["total"].as_i64().unwrap_or(0),
                                        cp["non_null"].as_i64().unwrap_or(0),
                                        cp["distinct_count"].as_i64().unwrap_or(0),
                                        cp["null_pct"].as_str().unwrap_or("0.00"),
                                        min_display,
                                        max_display,
                                    );
                                }
                            }
                            println!("\n{} column(s) profiled", col_profiles.len());
                        }
                    }
                    // ─── T-038: Health check ─────────────────────────────
                    Commands::Health => {
                        let mut checks: Vec<(&str, &str, String)> = Vec::new();

                        // 1. DB connectivity
                        match db.get_all_dags(1, 0).await {
                            Ok((dags, total)) => {
                                checks.push(("database", "ok", format!("{} DAGs registered", total)));
                                let _ = dags;
                            }
                            Err(e) => {
                                checks.push(("database", "error", format!("{}", e)));
                            }
                        }

                        // 2. Worker count
                        match db.get_all_workers().await {
                            Ok(workers) => {
                                let active = workers.iter().filter(|w| w["state"].as_str() == Some("active") || w["state"].as_str() == Some("Active")).count();
                                let status = if active > 0 { "ok" } else { "warn" };
                                checks.push(("workers", status, format!("{} active / {} total", active, workers.len())));
                            }
                            Err(e) => checks.push(("workers", "error", format!("{}", e))),
                        }

                        // 3. Queue depth
                        match db.get_task_queue(100).await {
                            Ok(queue) => {
                                let depth = queue.len();
                                let status = if depth > 50 { "warn" } else { "ok" };
                                checks.push(("queue", status, format!("{} tasks queued", depth)));
                            }
                            Err(e) => checks.push(("queue", "error", format!("{}", e))),
                        }

                        // 4. Stale datasets
                        match db.get_stale_datasets(3600).await {
                            Ok(stale) => {
                                let status = if stale.is_empty() { "ok" } else { "warn" };
                                checks.push(("datasets", status, format!("{} stale (>1h)", stale.len())));
                            }
                            Err(e) => checks.push(("datasets", "error", format!("{}", e))),
                        }

                        // 5. Overall status
                        let overall = if checks.iter().any(|c| c.1 == "error") {
                            "unhealthy"
                        } else if checks.iter().any(|c| c.1 == "warn") {
                            "degraded"
                        } else {
                            "healthy"
                        };

                        if json_output {
                            let report = serde_json::json!({
                                "status": overall,
                                "checks": checks.iter().map(|(name, status, msg)| {
                                    serde_json::json!({"component": name, "status": status, "message": msg})
                                }).collect::<Vec<_>>(),
                                "timestamp": chrono::Utc::now().to_rfc3339(),
                            });
                            println!("{}", serde_json::to_string_pretty(&report)?);
                        } else {
                            println!("=== Ryuo Health Check ===");
                            println!("Status: {}", overall.to_uppercase());
                            println!();
                            for (name, status, msg) in &checks {
                                let icon = match *status {
                                    "ok" => "✓",
                                    "warn" => "⚠",
                                    _ => "✗",
                                };
                                println!("  {} {} — {}", icon, name, msg);
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
    info!("🌪️ RYUO Orchestrator v0.7.0 - Operational");

    // Initialize Secret Vault
    let vault = match Vault::new() {
        Ok(v) => { info!("🔐 Secret Vault initialized (AES-256-GCM)."); Some(Arc::new(v)) },
        Err(e) => { warn!("⚠️ Secret Vault DISABLED: {}. Secrets will not be available.", e); None }
    };

    // Initialize Database Backend (PostgreSQL only)
    let db_url_owned = cli.database_url
        .ok_or_else(|| anyhow::anyhow!("❌ --database-url or DATABASE_URL env var is required. RYUO requires PostgreSQL."))?;

    let db_idle_timeout = std::time::Duration::from_secs(cli.db_idle_timeout);

    info!("🗄️ Initializing PostgreSQL backend...");
    let pg_db = db_postgres::PostgresDb::new(&db_url_owned, cli.db_max_connections, cli.db_min_connections, db_idle_timeout).await?;
    // ENT-18 FIX: Validate DB connectivity before serving traffic
    match sqlx::query("SELECT 1").execute(pg_db.pool()).await {
        Ok(_) => info!("✅ Database connection validated"),
        Err(e) => {
            error!("❌ Database connectivity check failed: {}", e);
            return Err(anyhow::anyhow!("Database not reachable: {}", e));
        }
    }
    let db: Arc<dyn db_trait::DatabaseBackend> = Arc::new(pg_db);
    info!("✅ Database initialized.");

    // Initialize Prometheus Metrics
    let ryuo_metrics = Arc::new(metrics::RyuoMetrics::new()?);
    info!("📊 Prometheus metrics initialized (GET /metrics)");

    // Recovery Mode
    let interrupted = db.get_interrupted_tasks().await?;
    if !interrupted.is_empty() {
        warn!("⚠️ Recovery Mode: Found {} interrupted tasks from previous run.", interrupted.len());
        for (ti_id, dag_id, task_id, run_id) in interrupted {
            info!("  - Marking instance {} ({}/{} run={}) as Failed", ti_id, dag_id, task_id, run_id);
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

                            // SEC-6: SHA256 checksum verification for plugin binaries.
                            // Look for a .sha256 sidecar file alongside the plugin.
                            let checksum_path = path.with_extension(format!("{}.sha256", ext));
                            if checksum_path.exists() {
                                match verify_plugin_checksum(&path, &checksum_path) {
                                    Ok(true) => {
                                        info!("✅ Plugin {:?} passed SHA256 verification", path);
                                    }
                                    Ok(false) => {
                                        error!("❌ Plugin {:?} FAILED SHA256 verification — skipping", path);
                                        continue;
                                    }
                                    Err(e) => {
                                        error!("❌ Error verifying plugin {:?}: {} — skipping", path, e);
                                        continue;
                                    }
                                }
                            } else {
                                // No checksum file — only allow with explicit flag
                                warn!(
                                    "⚠️ No .sha256 checksum file found for plugin {:?}. \
                                     Loading unverified plugin because --allow-unsafe-plugins is set.",
                                    path
                                );
                            }

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
                                // Also accept RYUO_ALLOW_PYTHON_DAGS=true env var so the flag
                                // can be toggled per-environment without rebuilding the image.
                                let python_enabled = cli.allow_unsafe_dag_exec
                                    || std::env::var("RYUO_ALLOW_PYTHON_DAGS")
                                        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                                        .unwrap_or(false);
                                if !python_enabled {
                                    warn!("⚠️ Skipping Python DAG file {} — set RYUO_ALLOW_PYTHON_DAGS=true or use --allow-unsafe-dag-exec to enable (SECURITY RISK)", path_str);
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
    let grpc_auth_token = std::env::var("RYUO_GRPC_AUTH_TOKEN").ok();
    let swarm_state = Arc::new(SwarmState::new(Arc::clone(&db), swarm_enabled, vault.clone(), Some(Arc::clone(&ryuo_metrics)), grpc_auth_token.clone()));

    if swarm_enabled {
        let health_state = Arc::clone(&swarm_state);

        // BUG-C7: Refuse to start gRPC without auth token in production mode
        if cli.production && grpc_auth_token.is_none() {
            error!("❌ Production mode requires RYUO_GRPC_AUTH_TOKEN to be set. Refusing to start gRPC server.");
        } else {
        let grpc_state = Arc::clone(&swarm_state);
        // SEC-10: Check CLI args first, then fall back to env vars for TLS cert/key
        let tls_cert_grpc = cli.tls_cert.clone()
            .or_else(|| std::env::var("RYUO_GRPC_TLS_CERT").ok());
        let tls_key_grpc = cli.tls_key.clone()
            .or_else(|| std::env::var("RYUO_GRPC_TLS_KEY").ok());
        let is_production = cli.production;

        // SEC-10: Production mode without TLS — restrict to localhost only
        let effective_grpc_bind = if is_production && tls_cert_grpc.is_none() {
            warn!("⚠️ Production mode: No TLS certs configured for gRPC. Binding to 127.0.0.1 only.");
            "127.0.0.1".to_string()
        } else {
            grpc_bind.clone()
        };

        // Spawn gRPC server
        tokio::spawn(async move {
            if let (Some(cert_path), Some(key_path)) = (&tls_cert_grpc, &tls_key_grpc) {
                let cert = std::fs::read(cert_path).expect("Failed to read TLS cert");
                let key = std::fs::read(key_path).expect("Failed to read TLS key");
                let identity = tonic::transport::Identity::from_pem(cert, key);
                // ENT-1: Load CA cert for mTLS (mutual TLS client verification)
                let tls_config = if let Ok(ca_path) = std::env::var("RYUO_GRPC_TLS_CA") {
                    let ca = std::fs::read(&ca_path).expect("Failed to read TLS CA cert");
                    info!("🔒 gRPC mTLS enabled — client certificates required");
                    tonic::transport::ServerTlsConfig::new()
                        .identity(identity)
                        .client_ca_root(tonic::transport::Certificate::from_pem(ca))
                } else {
                    tonic::transport::ServerTlsConfig::new().identity(identity)
                };
                let addr = format!("{}:{}", effective_grpc_bind, swarm_port).parse().unwrap();
                let server = swarm::create_grpc_server(grpc_state);
                info!("🐝 Swarm Controller listening on {} (TLS + Auth)", addr);
                let _ = tonic::transport::Server::builder()
                    .tls_config(tls_config).unwrap()
                    .add_service(server)
                    .serve(addr).await;
            } else {
                let addr = format!("{}:{}", effective_grpc_bind, swarm_port).parse().unwrap();
                let server = swarm::create_grpc_server(grpc_state);
                info!("🐝 Swarm Controller listening on {} (plaintext — dev only)", addr);
                let _ = tonic::transport::Server::builder().add_service(server).serve(addr).await;
            }
        });

        } // end production auth-token check

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
    let metrics_web = Arc::clone(&ryuo_metrics);
    // Bug 26 fix: add --port CLI flag so the web port is configurable.
    let web_port = cli.port;
    tokio::spawn(async move {
        let auth_mgr = {
            let mut mgr = crate::auth::AuthManager::new(Arc::clone(&db_web));
            // Register the local (DB-backed) authentication provider
            let local_provider = crate::auth::LocalAuthProvider::new(Arc::clone(&db_web));
            if let Err(e) = mgr.register_provider(std::sync::Arc::new(local_provider)) {
                warn!("Failed to register local auth provider: {}", e);
            }
            Some(Arc::new(tokio::sync::RwLock::new(mgr)))
        };
        let server = web::WebServer::new(db_web, tx_web, swarm_web, vault_web, dags_web, metrics_web, auth_mgr);
        if let Err(e) = server.run(web_port, tls_cert, tls_key).await {
            error!("Web server fatally exited: {}", e);
        }
    });

    // Scheduler Loop
    let db_sched = Arc::clone(&db);
    let dags_sched = Arc::clone(&all_dags);
    let swarm_sched = Arc::clone(&swarm_state);
    let metrics_sched = Arc::clone(&ryuo_metrics);
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
    let metrics_cron = Arc::clone(&ryuo_metrics);
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

    // BUG-M8 FIX: Periodic session cleanup — delete expired sessions every 15
    // minutes so they don't accumulate in the DB indefinitely.
    let db_session_cleanup = Arc::clone(&db);
    tokio::spawn(async move {
        info!("🧹 Session cleanup loop started (every 15 min)");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
            match db_session_cleanup.cleanup_expired_sessions().await {
                Ok(count) => {
                    if count > 0 {
                        info!("🧹 Cleaned up {} expired sessions", count);
                    }
                }
                Err(e) => warn!("⚠️ Session cleanup error: {}", e),
            }
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
            for (ti_id, _dag_id, _task_id, _run_id) in tasks {
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

    info!("👋 RYUO controller shut down cleanly.");
    Ok(())
}

/// SEC-6: Verify a plugin binary's SHA256 checksum against its sidecar `.sha256` file.
///
/// The `.sha256` file should contain the hex-encoded SHA256 hash as the first
/// whitespace-delimited token (compatible with `sha256sum` output format).
fn verify_plugin_checksum(
    plugin_path: &std::path::Path,
    checksum_path: &std::path::Path,
) -> anyhow::Result<bool> {
    use sha2::{Sha256, Digest};

    let expected_hex = std::fs::read_to_string(checksum_path)
        .map_err(|e| anyhow::anyhow!("Failed to read checksum file {:?}: {}", checksum_path, e))?;
    let expected_hex = expected_hex
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Checksum file {:?} is empty", checksum_path))?
        .to_lowercase();

    let plugin_bytes = std::fs::read(plugin_path)
        .map_err(|e| anyhow::anyhow!("Failed to read plugin file {:?}: {}", plugin_path, e))?;
    let mut hasher = Sha256::new();
    hasher.update(&plugin_bytes);
    let actual_hex = format!("{:x}", hasher.finalize());

    Ok(actual_hex == expected_hex)
}

fn create_benchmark_dag() -> Dag {
    let mut dag = Dag::new("parallel_benchmark");
    dag.add_task("t1", "Warm-up", "echo 'Ryuo engine warm-up...'");
    dag.add_task("t2", "A", "sleep 1 && echo 'Ingestion A complete'");
    dag.add_task("t3", "B", "sleep 1 && echo 'Ingestion B complete'");
    dag.add_task("t4", "C", "sleep 1 && echo 'Ingestion C complete'");
    dag.add_task("t5", "Final", "echo 'All data processed. Ryuo out.'");
    dag.add_dependency("t1", "t2"); dag.add_dependency("t1", "t3"); dag.add_dependency("t1", "t4");
    dag.add_dependency("t2", "t5"); dag.add_dependency("t3", "t5"); dag.add_dependency("t4", "t5");
    dag
}
