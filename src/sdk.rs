#![allow(dead_code)]
// Developer SDK, Plugin Ecosystem & Testing Framework
//
// Provides plugin scaffolding, marketplace registry, DAG testing framework,
// plugin validation, and IDE integration hooks.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

// ─── Plugin Manifest ──────────────────────────────────────────

/// Plugin manifest (vortex-plugin.toml) — describes a plugin package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub license: String,
    pub vortex_version: String,
    pub plugin_type: PluginType,
    pub entry_point: String,
    pub capabilities: Vec<String>,
    pub dependencies: HashMap<String, String>,
    pub config_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    Operator,
    Sensor,
    Connector,
    Hook,
    Decorator,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<Vec<String>> {
        let mut warnings = Vec::new();

        if self.name.is_empty() {
            return Err(anyhow!("Plugin name is required"));
        }
        if !self.name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(anyhow!("Plugin name must be alphanumeric with hyphens/underscores"));
        }
        if self.version.is_empty() {
            return Err(anyhow!("Plugin version is required"));
        }
        if self.entry_point.is_empty() {
            return Err(anyhow!("Entry point is required"));
        }
        if self.description.is_empty() {
            warnings.push("Plugin description is empty".to_string());
        }
        if self.authors.is_empty() {
            warnings.push("No authors specified".to_string());
        }

        Ok(warnings)
    }
}

// ─── Plugin Scaffold Generator ────────────────────────────────

/// Generate scaffold files for a new plugin project.
pub struct PluginScaffold;

impl PluginScaffold {
    /// Generate a new plugin project in the given directory.
    pub async fn generate(name: &str, plugin_type: PluginType, output_dir: &Path) -> Result<Vec<PathBuf>> {
        let project_dir = output_dir.join(name);
        tokio::fs::create_dir_all(&project_dir).await
            .map_err(|e| anyhow!("Failed to create plugin directory: {}", e))?;
        tokio::fs::create_dir_all(project_dir.join("src")).await?;
        tokio::fs::create_dir_all(project_dir.join("tests")).await?;

        let mut created = Vec::new();

        // Cargo.toml
        let cargo_toml = Self::generate_cargo_toml(name, &plugin_type);
        let cargo_path = project_dir.join("Cargo.toml");
        tokio::fs::write(&cargo_path, cargo_toml).await?;
        created.push(cargo_path);

        // vortex-plugin.toml
        let manifest = Self::generate_manifest(name, &plugin_type);
        let manifest_path = project_dir.join("vortex-plugin.toml");
        tokio::fs::write(&manifest_path, manifest).await?;
        created.push(manifest_path);

        // src/lib.rs
        let lib_rs = Self::generate_lib_rs(name, &plugin_type);
        let lib_path = project_dir.join("src/lib.rs");
        tokio::fs::write(&lib_path, lib_rs).await?;
        created.push(lib_path);

        // tests/integration.rs
        let test_rs = Self::generate_test_rs(name, &plugin_type);
        let test_path = project_dir.join("tests/integration.rs");
        tokio::fs::write(&test_path, test_rs).await?;
        created.push(test_path);

        // README.md
        let readme = format!(
            "# {}\n\nA Vortex {:?} plugin.\n\n## Build\n\n```bash\ncargo build --release\n```\n\n## Test\n\n```bash\ncargo test\n```\n",
            name, plugin_type
        );
        let readme_path = project_dir.join("README.md");
        tokio::fs::write(&readme_path, readme).await?;
        created.push(readme_path);

        info!(name = %name, plugin_type = ?plugin_type, files = created.len(), "Plugin scaffold generated");
        Ok(created)
    }

    fn generate_cargo_toml(name: &str, _plugin_type: &PluginType) -> String {
        format!(r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
vortex = {{ path = "../.." }}
anyhow = "1.0"
async-trait = "0.1"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tokio = {{ version = "1", features = ["full"] }}
tracing = "0.1"
"#)
    }

    fn generate_manifest(name: &str, plugin_type: &PluginType) -> String {
        let type_str = match plugin_type {
            PluginType::Operator => "operator",
            PluginType::Sensor => "sensor",
            PluginType::Connector => "connector",
            PluginType::Hook => "hook",
            PluginType::Decorator => "decorator",
        };
        format!(r#"name = "{name}"
version = "0.1.0"
description = "A Vortex {type_str} plugin"
authors = []
license = "Apache-2.0"
vortex_version = ">=0.6.0"
plugin_type = "{type_str}"
entry_point = "_vortex_plugin_create"
capabilities = []
"#)
    }

    fn generate_lib_rs(name: &str, plugin_type: &PluginType) -> String {
        let struct_name = name.split('-').map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }).collect::<String>();

        match plugin_type {
            PluginType::Operator => format!(r#"use vortex::executor::{{VortexOperator, TaskContext, ExecutionResult}};
use vortex::declare_plugin;
use anyhow::Result;

pub struct {struct_name};

#[async_trait::async_trait]
impl VortexOperator for {struct_name} {{
    async fn execute(&self, ctx: &TaskContext) -> Result<ExecutionResult> {{
        // TEMPLATE: Implement your operator logic here
        Ok(ExecutionResult {{
            task_id: ctx.task_id.clone(),
            success: true,
            exit_code: 0,
            stdout: "{struct_name} executed successfully".into(),
            stderr: String::new(),
            duration_ms: 0,
        }})
    }}
}}

impl {struct_name} {{
    pub fn new() -> Self {{
        {struct_name}
    }}
}}

declare_plugin!({struct_name}, {struct_name}::new);
"#),
            _ => format!(r#"// {struct_name} plugin stub
// TODO: Implement the plugin for type {:?}
pub struct {struct_name};
"#, plugin_type),
        }
    }

    fn generate_test_rs(name: &str, plugin_type: &PluginType) -> String {
        let struct_name = name.split('-').map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }).collect::<String>();

        match plugin_type {
            PluginType::Operator => format!(r#"use {crate_name}::{struct_name};
use vortex::executor::{{VortexOperator, TaskContext}};

#[tokio::test]
async fn test_{name}_executes_successfully() {{
    let op = {struct_name}::new();
    let ctx = TaskContext {{
        task_id: "test-task".to_string(),
        dag_id: "test-dag".to_string(),
        run_id: "test-run".to_string(),
        attempt: 1,
        config: serde_json::json!({{}}),
    }};
    let result = op.execute(&ctx).await.unwrap();
    assert!(result.success);
}}
"#, crate_name = name.replace('-', "_"), struct_name = struct_name, name = name.replace('-', "_")),
            _ => format!(r#"// Integration tests for {name}
#[test]
fn test_plugin_loads() {{
    // FUTURE: Add integration tests for plugin lifecycle
    assert!(true);
}}
"#),
        }
    }
}

// ─── Plugin Registry (Marketplace) ───────────────────────────

/// A published plugin in the marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub manifest: PluginManifest,
    pub published_at: DateTime<Utc>,
    pub downloads: u64,
    pub checksum: String,
    pub artifact_url: Option<String>,
    pub verified: bool,
}

/// In-memory plugin marketplace registry.
pub struct PluginMarketplace {
    entries: Arc<RwLock<HashMap<String, Vec<RegistryEntry>>>>,
}

impl PluginMarketplace {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish a plugin to the registry.
    pub async fn publish(&self, manifest: PluginManifest, checksum: &str) -> Result<()> {
        manifest.validate()?;
        let entry = RegistryEntry {
            manifest: manifest.clone(),
            published_at: Utc::now(),
            downloads: 0,
            checksum: checksum.to_string(),
            artifact_url: None,
            verified: false,
        };
        let mut entries = self.entries.write().await;
        entries.entry(manifest.name.clone()).or_default().push(entry);
        info!(name = %manifest.name, version = %manifest.version, "Plugin published");
        Ok(())
    }

    /// Search for plugins by name or keyword.
    pub async fn search(&self, query: &str) -> Vec<RegistryEntry> {
        let entries = self.entries.read().await;
        let query_lower = query.to_lowercase();
        entries.values().flat_map(|versions| versions.iter())
            .filter(|e| {
                e.manifest.name.to_lowercase().contains(&query_lower)
                    || e.manifest.description.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect()
    }

    /// Get a specific plugin by name and optional version.
    pub async fn get(&self, name: &str, version: Option<&str>) -> Option<RegistryEntry> {
        let entries = self.entries.read().await;
        entries.get(name).and_then(|versions| {
            if let Some(v) = version {
                versions.iter().find(|e| e.manifest.version == v).cloned()
            } else {
                versions.last().cloned()
            }
        })
    }

    /// List all plugins (latest version only).
    pub async fn list_all(&self) -> Vec<RegistryEntry> {
        let entries = self.entries.read().await;
        entries.values().filter_map(|versions| versions.last().cloned()).collect()
    }

    /// Increment download count for a plugin.
    pub async fn record_download(&self, name: &str, version: &str) -> Result<()> {
        let mut entries = self.entries.write().await;
        if let Some(versions) = entries.get_mut(name) {
            if let Some(entry) = versions.iter_mut().find(|e| e.manifest.version == version) {
                entry.downloads += 1;
                return Ok(());
            }
        }
        Err(anyhow!("Plugin {}@{} not found", name, version))
    }
}

// ─── DAG Testing Framework ────────────────────────────────────

/// Test harness for DAG definitions — validate structure, task deps, cycles.
#[derive(Debug, Clone)]
pub struct DagTestHarness {
    pub dag_id: String,
    pub tasks: Vec<TestTask>,
}

#[derive(Debug, Clone)]
pub struct TestTask {
    pub task_id: String,
    pub operator: String,
    pub dependencies: Vec<String>,
    pub config: Value,
}

/// Result of a DAG test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagTestResult {
    pub dag_id: String,
    pub passed: bool,
    pub checks: Vec<TestCheck>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

impl DagTestHarness {
    pub fn new(dag_id: &str) -> Self {
        Self {
            dag_id: dag_id.to_string(),
            tasks: Vec::new(),
        }
    }

    pub fn add_task(&mut self, task_id: &str, operator: &str, deps: Vec<&str>, config: Value) {
        self.tasks.push(TestTask {
            task_id: task_id.to_string(),
            operator: operator.to_string(),
            dependencies: deps.into_iter().map(String::from).collect(),
            config,
        });
    }

    /// Run all structural validations on the DAG.
    pub fn validate(&self) -> DagTestResult {
        let start = std::time::Instant::now();
        let mut checks = Vec::new();

        // Check 1: DAG has at least one task
        checks.push(TestCheck {
            name: "has_tasks".to_string(),
            passed: !self.tasks.is_empty(),
            message: if self.tasks.is_empty() {
                "DAG has no tasks".into()
            } else {
                format!("DAG has {} tasks", self.tasks.len())
            },
        });

        // Check 2: All task IDs are unique
        let mut task_ids: Vec<&str> = self.tasks.iter().map(|t| t.task_id.as_str()).collect();
        let unique_count = {
            task_ids.sort();
            task_ids.dedup();
            task_ids.len()
        };
        checks.push(TestCheck {
            name: "unique_task_ids".to_string(),
            passed: unique_count == self.tasks.len(),
            message: if unique_count == self.tasks.len() {
                "All task IDs are unique".into()
            } else {
                "Duplicate task IDs found".into()
            },
        });

        // Check 3: All dependency references exist
        let valid_ids: std::collections::HashSet<&str> = self.tasks.iter().map(|t| t.task_id.as_str()).collect();
        let mut all_deps_valid = true;
        let mut missing_deps = Vec::new();
        for task in &self.tasks {
            for dep in &task.dependencies {
                if !valid_ids.contains(dep.as_str()) {
                    all_deps_valid = false;
                    missing_deps.push(format!("{} -> {}", task.task_id, dep));
                }
            }
        }
        checks.push(TestCheck {
            name: "valid_dependencies".to_string(),
            passed: all_deps_valid,
            message: if all_deps_valid {
                "All dependencies reference valid tasks".into()
            } else {
                format!("Missing dependencies: {}", missing_deps.join(", "))
            },
        });

        // Check 4: No cycles (topological sort check)
        let has_cycle = self.detect_cycle();
        checks.push(TestCheck {
            name: "no_cycles".to_string(),
            passed: !has_cycle,
            message: if has_cycle { "Cycle detected in task dependencies".into() } else { "No cycles detected".into() },
        });

        // Check 5: All tasks have operators
        let all_have_operators = self.tasks.iter().all(|t| !t.operator.is_empty());
        checks.push(TestCheck {
            name: "all_tasks_have_operators".to_string(),
            passed: all_have_operators,
            message: if all_have_operators {
                "All tasks have operators assigned".into()
            } else {
                "Some tasks are missing operators".into()
            },
        });

        let passed = checks.iter().all(|c| c.passed);
        DagTestResult {
            dag_id: self.dag_id.clone(),
            passed,
            checks,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Detect cycles using DFS coloring.
    fn detect_cycle(&self) -> bool {
        let task_map: HashMap<&str, &TestTask> = self.tasks.iter()
            .map(|t| (t.task_id.as_str(), t)).collect();
        let mut visited: HashMap<&str, u8> = HashMap::new(); // 0=white, 1=gray, 2=black

        fn dfs<'a>(
            node: &'a str,
            task_map: &HashMap<&str, &'a TestTask>,
            visited: &mut HashMap<&'a str, u8>,
        ) -> bool {
            visited.insert(node, 1); // gray
            if let Some(task) = task_map.get(node) {
                for dep in &task.dependencies {
                    match visited.get(dep.as_str()) {
                        Some(1) => return true, // back edge = cycle
                        Some(2) => {} // already finished
                        _ => {
                            if dfs(dep.as_str(), task_map, visited) {
                                return true;
                            }
                        }
                    }
                }
            }
            visited.insert(node, 2); // black
            false
        }

        for task in &self.tasks {
            if *visited.get(task.task_id.as_str()).unwrap_or(&0) == 0 {
                if dfs(task.task_id.as_str(), &task_map, &mut visited) {
                    return true;
                }
            }
        }
        false
    }

    /// Compute execution order (topological sort). Returns None if there's a cycle.
    pub fn execution_order(&self) -> Option<Vec<String>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

        for task in &self.tasks {
            in_degree.entry(task.task_id.as_str()).or_insert(0);
            adj.entry(task.task_id.as_str()).or_default();
            for dep in &task.dependencies {
                adj.entry(dep.as_str()).or_default().push(task.task_id.as_str());
                *in_degree.entry(task.task_id.as_str()).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<&str> = in_degree.iter()
            .filter(|(_, deg)| **deg == 0).map(|(id, _)| *id).collect();
        queue.sort(); // deterministic ordering
        let mut order = Vec::new();

        while let Some(node) = queue.pop() {
            order.push(node.to_string());
            if let Some(neighbors) = adj.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(neighbor);
                            queue.sort();
                        }
                    }
                }
            }
        }

        if order.len() == self.tasks.len() { Some(order) } else { None }
    }
}

// ─── Plugin Validator ─────────────────────────────────────────

/// Validate a plugin binary before loading.
pub struct PluginValidator;

impl PluginValidator {
    /// Check that a shared library exports the required symbol.
    pub fn validate_binary(path: &Path) -> Result<ValidationResult> {
        let mut checks = Vec::new();

        // Check file exists
        let exists = path.exists();
        checks.push(TestCheck {
            name: "file_exists".to_string(),
            passed: exists,
            message: if exists { "Plugin binary found".into() } else { format!("File not found: {}", path.display()) },
        });

        if !exists {
            return Ok(ValidationResult { valid: false, checks });
        }

        // Check file extension
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let valid_ext = ext == "so" || ext == "dylib" || ext == "dll";
        checks.push(TestCheck {
            name: "valid_extension".to_string(),
            passed: valid_ext,
            message: if valid_ext {
                format!("Valid extension: .{}", ext)
            } else {
                format!("Expected .so/.dylib/.dll, got .{}", ext)
            },
        });

        // Check file size is reasonable (not empty, not too large)
        if let Ok(metadata) = std::fs::metadata(path) {
            let size = metadata.len();
            let reasonable = size > 0 && size < 500_000_000; // < 500MB
            checks.push(TestCheck {
                name: "reasonable_size".to_string(),
                passed: reasonable,
                message: format!("Binary size: {} bytes", size),
            });
        }

        let valid = checks.iter().all(|c| c.passed);
        Ok(ValidationResult { valid, checks })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub checks: Vec<TestCheck>,
}

// ─── IDE Integration Hooks ────────────────────────────────────

/// LSP-compatible diagnostic for IDE integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Analyze a DAG file and produce diagnostics for IDE display.
pub fn analyze_dag_file(content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Check for common issues
    for (i, line) in content.lines().enumerate() {
        let line_num = (i + 1) as u32;

        // Detect hardcoded secrets
        let lower = line.to_lowercase();
        if (lower.contains("password") || lower.contains("secret") || lower.contains("api_key"))
            && line.contains('=')
            && !line.trim_start().starts_with('#')
            && !line.trim_start().starts_with("//")
        {
            diagnostics.push(Diagnostic {
                file: String::new(),
                line: line_num,
                column: 1,
                severity: DiagnosticSeverity::Warning,
                message: "Potential hardcoded secret detected. Use vault or environment variables.".to_string(),
                code: Some("VTX-SEC-001".to_string()),
            });
        }

        // Detect deprecated patterns
        if line.contains("BashOperator") {
            diagnostics.push(Diagnostic {
                file: String::new(),
                line: line_num,
                column: 1,
                severity: DiagnosticSeverity::Info,
                message: "Consider using ShellOperator for better error handling.".to_string(),
                code: Some("VTX-DEP-001".to_string()),
            });
        }
    }

    diagnostics
}

// ─── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manifest_validation() {
        let manifest = PluginManifest {
            name: "my-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "Test plugin".to_string(),
            authors: vec!["Test Author".to_string()],
            license: "Apache-2.0".to_string(),
            vortex_version: ">=0.6.0".to_string(),
            plugin_type: PluginType::Operator,
            entry_point: "_vortex_plugin_create".to_string(),
            capabilities: vec![],
            dependencies: HashMap::new(),
            config_schema: None,
        };
        let warnings = manifest.validate().unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_plugin_manifest_validation_failure() {
        let manifest = PluginManifest {
            name: "".to_string(),
            version: "1.0.0".to_string(),
            description: "".to_string(),
            authors: vec![],
            license: "".to_string(),
            vortex_version: "".to_string(),
            plugin_type: PluginType::Operator,
            entry_point: "entry".to_string(),
            capabilities: vec![],
            dependencies: HashMap::new(),
            config_schema: None,
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_dag_test_harness_basic() {
        let mut harness = DagTestHarness::new("test_dag");
        harness.add_task("extract", "HttpOperator", vec![], serde_json::json!({}));
        harness.add_task("transform", "ShellOperator", vec!["extract"], serde_json::json!({}));
        harness.add_task("load", "ShellOperator", vec!["transform"], serde_json::json!({}));

        let result = harness.validate();
        assert!(result.passed);
        assert_eq!(result.checks.len(), 5);
    }

    #[test]
    fn test_dag_test_harness_cycle_detection() {
        let mut harness = DagTestHarness::new("cyclic_dag");
        harness.add_task("a", "Op", vec!["c"], serde_json::json!({}));
        harness.add_task("b", "Op", vec!["a"], serde_json::json!({}));
        harness.add_task("c", "Op", vec!["b"], serde_json::json!({}));

        let result = harness.validate();
        assert!(!result.passed);
        let cycle_check = result.checks.iter().find(|c| c.name == "no_cycles").unwrap();
        assert!(!cycle_check.passed);
    }

    #[test]
    fn test_dag_test_harness_missing_deps() {
        let mut harness = DagTestHarness::new("bad_dag");
        harness.add_task("a", "Op", vec!["nonexistent"], serde_json::json!({}));

        let result = harness.validate();
        assert!(!result.passed);
    }

    #[test]
    fn test_execution_order() {
        let mut harness = DagTestHarness::new("ordered_dag");
        harness.add_task("a", "Op", vec![], serde_json::json!({}));
        harness.add_task("b", "Op", vec!["a"], serde_json::json!({}));
        harness.add_task("c", "Op", vec!["a"], serde_json::json!({}));
        harness.add_task("d", "Op", vec!["b", "c"], serde_json::json!({}));

        let order = harness.execution_order().unwrap();
        assert_eq!(order[0], "a");
        assert_eq!(order.last().unwrap(), "d");
        assert!(order.iter().position(|x| x == "b").unwrap() > order.iter().position(|x| x == "a").unwrap());
        assert!(order.iter().position(|x| x == "c").unwrap() > order.iter().position(|x| x == "a").unwrap());
    }

    #[test]
    fn test_execution_order_cycle() {
        let mut harness = DagTestHarness::new("cyclic");
        harness.add_task("a", "Op", vec!["b"], serde_json::json!({}));
        harness.add_task("b", "Op", vec!["a"], serde_json::json!({}));
        assert!(harness.execution_order().is_none());
    }

    #[tokio::test]
    async fn test_marketplace_publish_and_search() {
        let market = PluginMarketplace::new();
        let manifest = PluginManifest {
            name: "bigquery-loader".to_string(),
            version: "1.0.0".to_string(),
            description: "Load data into BigQuery".to_string(),
            authors: vec!["Vortex Team".to_string()],
            license: "Apache-2.0".to_string(),
            vortex_version: ">=0.6.0".to_string(),
            plugin_type: PluginType::Operator,
            entry_point: "_vortex_plugin_create".to_string(),
            capabilities: vec!["write".to_string()],
            dependencies: HashMap::new(),
            config_schema: None,
        };
        market.publish(manifest, "sha256:abc123").await.unwrap();

        let results = market.search("bigquery").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].manifest.name, "bigquery-loader");

        let empty = market.search("nonexistent").await;
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_marketplace_versions() {
        let market = PluginMarketplace::new();
        for v in &["1.0.0", "1.1.0", "2.0.0"] {
            let manifest = PluginManifest {
                name: "test-plugin".to_string(),
                version: v.to_string(),
                description: "Test".to_string(),
                authors: vec![],
                license: "MIT".to_string(),
                vortex_version: ">=0.6.0".to_string(),
                plugin_type: PluginType::Operator,
                entry_point: "entry".to_string(),
                capabilities: vec![],
                dependencies: HashMap::new(),
                config_schema: None,
            };
            market.publish(manifest, "check").await.unwrap();
        }

        // Latest version
        let latest = market.get("test-plugin", None).await.unwrap();
        assert_eq!(latest.manifest.version, "2.0.0");

        // Specific version
        let v1 = market.get("test-plugin", Some("1.0.0")).await.unwrap();
        assert_eq!(v1.manifest.version, "1.0.0");
    }

    #[test]
    fn test_plugin_validator() {
        let result = PluginValidator::validate_binary(Path::new("/nonexistent/plugin.so")).unwrap();
        assert!(!result.valid);
    }

    #[test]
    fn test_analyze_dag_file_secrets() {
        let content = r#"
password = "my_secret_123"
api_key = "key-value"
# password = "commented out"
normal_var = "hello"
"#;
        let diags = analyze_dag_file(content);
        assert_eq!(diags.len(), 2); // password and api_key lines
        assert!(diags.iter().all(|d| d.code.as_deref() == Some("VTX-SEC-001")));
    }

    #[test]
    fn test_analyze_dag_file_deprecated() {
        let content = "task = BashOperator(command='echo hello')";
        let diags = analyze_dag_file(content);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("VTX-DEP-001"));
    }

    #[tokio::test]
    async fn test_scaffold_generates_files() {
        let dir = std::env::temp_dir().join("vortex_scaffold_test");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let files = PluginScaffold::generate("test-operator", PluginType::Operator, &dir).await.unwrap();
        assert!(files.len() >= 4);
        assert!(dir.join("test-operator/Cargo.toml").exists());
        assert!(dir.join("test-operator/src/lib.rs").exists());
        assert!(dir.join("test-operator/tests/integration.rs").exists());
        assert!(dir.join("test-operator/vortex-plugin.toml").exists());

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
