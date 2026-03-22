use clap::{Parser, Subcommand};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(name = "vortex-cli", about = "VORTEX Command Line Interface", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage DAGs
    Dags {
        #[command(subcommand)]
        action: DagsAction,
    },
    /// Manage Secrets
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },
    /// Manage Tasks
    Tasks {
        #[command(subcommand)]
        action: TasksAction,
    },
    /// Manage Users
    Users {
        #[command(subcommand)]
        action: UsersAction,
    },
    /// Migrate Airflow DAGs to generated Rust DAG modules
    Migrate {
        /// Path to a Python DAG file or a directory containing DAG files
        input_path: String,
        /// Output directory for generated Rust modules
        #[arg(long, default_value = "generated_dags")]
        output_dir: String,
        /// Strict mode: fail if placeholders are generated
        #[arg(long)]
        strict: bool,
        /// Enable agentic PythonOperator translation
        #[arg(long)]
        agentic: bool,
        /// LLM provider to use when --agentic is set: openai|anthropic
        #[arg(long, default_value = "openai")]
        llm_provider: String,
        /// Optional model override for agentic translation
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DagsAction {
    /// List all DAGs
    List,
    /// Trigger a DAG run
    Trigger {
        /// ID of the DAG to trigger
        id: String,
    },
    /// Backfill a DAG over a date range
    Backfill {
        /// ID of the DAG
        id: String,
        /// Start date (YYYY-MM-DD or RFC3339)
        #[arg(long)]
        start_date: String,
        /// End date (YYYY-MM-DD or RFC3339)
        #[arg(long)]
        end_date: String,
        /// Run in parallel catchup mode
        #[arg(long)]
        parallel: bool,
        /// Dry run: list intervals without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Pause a DAG
    Pause {
        /// ID of the DAG to pause
        id: String,
    },
    /// Unpause a DAG
    Unpause {
        /// ID of the DAG to unpause
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum TasksAction {
    /// Get logs for a specific task instance
    Logs {
        /// ID of the task instance
        instance_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum SecretsAction {
    /// Set a secret value
    Set {
        key: String,
        value: String,
    },
}

#[derive(Subcommand, Debug)]
enum UsersAction {
    /// Create a new user
    Create {
        user: String,
        #[arg(long, default_value = "Operator")]
        role: String,
        #[arg(long, default_value = "changeme")]
        password: String,
    },
}

struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    fn new() -> Self {
        let base_url = env::var("VORTEX_SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let api_key = env::var("VORTEX_API_KEY").unwrap_or_else(|_| {
            String::new()
        });

        let team_id = env::var("VORTEX_TEAM").unwrap_or_else(|_| String::new());

        let mut headers = HeaderMap::new();
        if !api_key.is_empty() {
            if let Ok(h) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
                headers.insert(AUTHORIZATION, h);
            }
        }
        if !team_id.is_empty() {
            if let Ok(h) = HeaderValue::from_str(&team_id) {
                headers.insert(reqwest::header::HeaderName::from_static("x-vortex-team"), h);
            }
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    async fn get(&self, path: &str) -> reqwest::Result<reqwest::Response> {
        self.client.get(format!("{}{}", self.base_url, path)).send().await
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> reqwest::Result<reqwest::Response> {
        self.client.post(format!("{}{}", self.base_url, path)).json(&body).send().await
    }

    async fn patch(&self, path: &str) -> reqwest::Result<reqwest::Response> {
        self.client.patch(format!("{}{}", self.base_url, path)).send().await
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let api = ApiClient::new();

    match cli.command {
        Commands::Dags { action } => match action {
            DagsAction::List => {
                match api.get("/api/dags").await {
                    Ok(res) => {
                        if res.status().is_success() {
                            let text = res.text().await.unwrap_or_default();
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                // Bug 8 fix: server returns PaginatedResponse { data: [...], total, limit, offset }
                                let dags_array = json["data"].as_array().or_else(|| json.as_array());
                                if let Some(dags) = dags_array {
                                    use cli_table::{Cell, Style, Table};
                                    let mut table = Vec::new();
                                    for dag in dags {
                                        // Bug 8 fix: server returns "id", not "dag_id"
                                        let id = dag["id"].as_str().unwrap_or("?");
                                        let schedule = dag["schedule_interval"].as_str().unwrap_or("None");
                                        let paused = dag["is_paused"].as_bool().unwrap_or(false);
                                        let dynamic = dag["is_dynamic"].as_bool().unwrap_or(false);
                                        table.push(vec![
                                            id.cell(),
                                            schedule.cell(),
                                            if paused { "⏸️  Paused".cell() } else { "▶️  Active".cell() },
                                            if dynamic { "✨ Yes".cell() } else { "No".cell() },
                                        ]);
                                    }
                                    if !table.is_empty() {
                                        let table_display = table.table()
                                            .title(vec![
                                                "DAG ID".cell().bold(true),
                                                "Schedule".cell().bold(true),
                                                "Status".cell().bold(true),
                                                "Dynamic".cell().bold(true),
                                            ])
                                            .bold(true);
                                        if let Ok(display) = table_display.display() {
                                            println!("{}", display);
                                        }
                                    } else {
                                        println!("No DAGs found.");
                                    }
                                } else {
                                    println!("{}", text);
                                }
                            } else {
                                println!("{}", text);
                            }
                        } else {
                            eprintln!("Failed to list dags: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
            DagsAction::Trigger { id } => {
                match api.post(&format!("/api/dags/{}/trigger", id), json!({})).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            println!("Successfully triggered DAG: {}", id);
                        } else {
                            eprintln!("Failed to trigger DAG: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
            DagsAction::Backfill { id, start_date, end_date, parallel, dry_run } => {
                // Formatting naive dates to RFC3339 if needed
                let start = if start_date.len() == 10 { format!("{}T00:00:00Z", start_date) } else { start_date };
                let end = if end_date.len() == 10 { format!("{}T23:59:59Z", end_date) } else { end_date };
                
                let body = json!({
                    "start_date": start,
                    "end_date": end,
                    "parallel": parallel,
                    "dry_run": dry_run
                });

                match api.post(&format!("/api/dags/{}/backfill", id), body).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            let text = res.text().await.unwrap_or_default();
                            if dry_run {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                    println!("{}", json["message"].as_str().unwrap_or("Dry run"));
                                    if let Some(intervals) = json["intervals"].as_array() {
                                        for interval in intervals {
                                            println!("  - {}", interval.as_str().unwrap_or(""));
                                        }
                                    }
                                } else {
                                    println!("{}", text);
                                }
                            } else {
                                println!("Backfill started: {}", text);
                            }
                        } else {
                            eprintln!("Failed to backfill DAG: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
            DagsAction::Pause { id } => {
                // Bug 7 fix: server defines PATCH /api/dags/:id/pause, not POST /api/dags/:id/config
                match api.patch(&format!("/api/dags/{}/pause", id)).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            println!("DAG {} paused.", id);
                        } else {
                            eprintln!("Failed to pause DAG: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
            DagsAction::Unpause { id } => {
                // Bug 7 fix: server defines PATCH /api/dags/:id/unpause, not POST /api/dags/:id/config
                match api.patch(&format!("/api/dags/{}/unpause", id)).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            println!("DAG {} unpaused.", id);
                        } else {
                            eprintln!("Failed to unpause DAG: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
        },
        Commands::Tasks { action } => match action {
            TasksAction::Logs { instance_id } => {
                match api.get(&format!("/api/tasks/{}/logs", instance_id)).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            let text = res.text().await.unwrap_or_default();
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                println!("{}", json["logs"].as_str().unwrap_or("No logs available."));
                            } else {
                                println!("Invalid response format: {}", text);
                            }
                        } else {
                            eprintln!("Failed to get logs: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
        },
        Commands::Secrets { action } => match action {
            SecretsAction::Set { key, value } => {
                match api.post("/api/secrets", json!({"key": key, "value": value})).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            println!("Successfully set secret: {}", key);
                        } else {
                            eprintln!("Failed to set secret: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
        },
        Commands::Users { action } => match action {
            UsersAction::Create { user, role, password } => {
                match api.post("/api/users", json!({"username": user, "password": password, "role": role})).await {
                    Ok(res) => {
                        if res.status().is_success() {
                            let text = res.text().await.unwrap_or_default();
                            println!("{}", text);
                        } else {
                            eprintln!("Failed to create user: {}", res.status());
                            let txt = res.text().await.unwrap_or_default();
                            eprintln!("Response: {}", txt);
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error connecting to server: {}", e);
                        process::exit(1);
                    }
                }
            }
        },
        Commands::Migrate {
            input_path,
            output_dir,
            strict,
            agentic,
            llm_provider,
            model,
        } => {
            use vortex::agentic::{
                AnthropicProvider, OpenAiProvider, translate_python_to_rust_agentic,
            };
            use vortex::airflow_ast_parser::parse_airflow_file;
            use vortex::dag_codegen::{
                assert_graph_equivalence, validate_generated_rust_source,
                write_generated_dag_with_overrides, write_migration_report,
            };

            let input = PathBuf::from(&input_path);
            let mut py_files = Vec::new();
            if input.is_file() {
                py_files.push(input);
            } else if input.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&input) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.extension().and_then(|x| x.to_str()) == Some("py") {
                            py_files.push(p);
                        }
                    }
                }
            } else {
                eprintln!("Input path not found: {}", input_path);
                process::exit(1);
            }

            if py_files.is_empty() {
                eprintln!("No Python DAG files found in {}", input_path);
                process::exit(1);
            }

            let mut summaries = Vec::new();
            for file in py_files {
                match parse_airflow_file(&file) {
                    Ok(dags) => {
                        for dag in dags {
                            let mut overrides: HashMap<String, String> = HashMap::new();
                            if agentic {
                                for t in &dag.tasks {
                                    if t.operator_type.contains("Python") {
                                        let source_text = t
                                            .raw_kwargs
                                            .get("raw_args")
                                            .cloned()
                                            .unwrap_or_else(|| format!("python_callable={:?}", t.python_callable));

                                        let translated = if llm_provider == "anthropic" {
                                            let provider = AnthropicProvider {
                                                endpoint: env::var("ANTHROPIC_ENDPOINT").unwrap_or_else(|_| "https://api.anthropic.com/v1/messages".to_string()),
                                                api_key: env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
                                                model: model.clone().unwrap_or_else(|| "claude-3-5-sonnet-latest".to_string()),
                                            };
                                            translate_python_to_rust_agentic(&provider, &source_text, 3).await
                                        } else {
                                            let provider = OpenAiProvider {
                                                endpoint: env::var("OPENAI_ENDPOINT").unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string()),
                                                api_key: env::var("OPENAI_API_KEY").unwrap_or_default(),
                                                model: model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string()),
                                            };
                                            translate_python_to_rust_agentic(&provider, &source_text, 3).await
                                        };

                                        match translated {
                                            Ok(code) => {
                                                overrides.insert(t.task_id.clone(), code);
                                            }
                                            Err(e) => {
                                                if strict {
                                                    eprintln!(
                                                        "Agentic translation failed for DAG {} task {}: {}",
                                                        dag.dag_id, t.task_id, e
                                                    );
                                                    process::exit(1);
                                                }
                                                eprintln!(
                                                    "Agentic translation skipped for DAG {} task {}: {}",
                                                    dag.dag_id, t.task_id, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            match write_generated_dag_with_overrides(&output_dir, &dag, &overrides) {
                                Ok((summary, generated_source)) => {
                                    if let Err(e) = validate_generated_rust_source(&generated_source) {
                                        eprintln!(
                                            "Generated Rust validation failed for DAG {}: {}",
                                            summary.dag_id, e
                                        );
                                        process::exit(1);
                                    }
                                    if let Err(e) = assert_graph_equivalence(&dag, &generated_source) {
                                        eprintln!(
                                            "Graph equivalence check failed for DAG {}: {}",
                                            summary.dag_id, e
                                        );
                                        process::exit(1);
                                    }
                                    if strict && summary.placeholder_tasks > 0 {
                                        eprintln!(
                                            "Strict mode failed for DAG {}: {} placeholder tasks generated",
                                            summary.dag_id, summary.placeholder_tasks
                                        );
                                        process::exit(1);
                                    }
                                    println!(
                                        "Generated {} (converted={}, placeholders={})",
                                        summary.generated_file,
                                        summary.converted_tasks,
                                        summary.placeholder_tasks
                                    );
                                    summaries.push(summary);
                                }
                                Err(e) => {
                                    eprintln!("Generation failed for {}: {}", file.display(), e);
                                    process::exit(1);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Parse failed for {}: {}", file.display(), e);
                        process::exit(1);
                    }
                }
            }

            if let Err(e) = write_migration_report(&output_dir, &summaries) {
                eprintln!("Failed to write migration report: {}", e);
                process::exit(1);
            }

            println!("Migration completed. Output: {}", output_dir);
        }
    }
}
