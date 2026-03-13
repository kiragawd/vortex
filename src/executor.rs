use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use std::time::Instant;
use tempfile::NamedTempFile;
use std::sync::Arc;
use anyhow::{anyhow, Result};
use libloading::Library;

use std::sync::RwLock;

//── Plugin Registry ──────────────────────────────────────────────────────────
//
// Bug 35 note: PLUGIN_REGISTRY uses std::sync::RwLock, which is thread-safe for
// concurrent reads, but its safety guarantee is convention-only in the following
// sense: `load_plugin` is `unsafe` and must ONLY be called at server startup,
// before the first `get_plugin` call. Calling it after the server is serving
// requests introduces a race between readers and the exclusive write.
// Future work: replace with an AppState-scoped registry to enforce init ordering.
pub static PLUGIN_REGISTRY: RwLock<Option<PluginRegistry>> = RwLock::new(None);

/// Initialise the global plugin registry. Call exactly once at server startup
/// before accepting any requests.
pub fn init_global_registry(registry: PluginRegistry) {
    if let Ok(mut lock) = PLUGIN_REGISTRY.write() {
        *lock = Some(registry);
    }
}

/// Look up a plugin by name. Thread-safe for concurrent reads.
pub fn get_plugin(name: &str) -> Option<Arc<dyn VortexOperator>> {
    if let Ok(lock) = PLUGIN_REGISTRY.read() {
        if let Some(reg) = lock.as_ref() {
            return reg.get(name);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct TaskContext {
    pub task_id: String,
    pub command: String,
    pub config: serde_json::Value,
    pub env_vars: HashMap<String, String>,
}

#[async_trait::async_trait]
pub trait VortexOperator: Send + Sync {
    async fn execute(&self, context: &TaskContext) -> Result<ExecutionResult>;
}

pub struct PluginRegistry {
    plugins: HashMap<String, Arc<dyn VortexOperator>>,
    _libraries: Vec<Library>, // Keeps loaded shared libraries in memory
}

impl PluginRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            plugins: HashMap::new(),
            _libraries: Vec::new(),
        };
        registry.register("http", HttpOperator);
        registry
    }

    pub fn register<S: Into<String>, O: VortexOperator + 'static>(&mut self, name: S, operator: O) {
        self.plugins.insert(name.into(), Arc::new(operator));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn VortexOperator>> {
        self.plugins.get(name).cloned()
    }

    /// Dynamically loads a plugin from a shared library.
    /// The library must export a `_vortex_plugin_create` C-ABI function.
    ///
    /// # Safety
    /// Bug 20 note: this function executes arbitrary native code from the loaded
    /// `.so`/`.dylib`. Only load plugins from trusted, verified sources.
    /// There is no sandboxing — a malicious plugin has full process access.
    /// Future work: run plugins in a separate process with restricted capabilities.
    pub unsafe fn load_plugin<S: Into<String>>(&mut self, path: &str, name: S) -> Result<()> {
        let lib = unsafe { Library::new(path)? };
        let creator: libloading::Symbol<unsafe extern "C" fn() -> *mut dyn VortexOperator> = unsafe { lib.get(b"_vortex_plugin_create\0")? };
        
        let ptr = unsafe { creator() };
        if ptr.is_null() {
            return Err(anyhow!("Plugin returned a null pointer during initialization"));
        }
        
        let boxed_plugin = unsafe { Box::from_raw(ptr) };
        self.plugins.insert(name.into(), Arc::from(boxed_plugin));
        self._libraries.push(lib);
        
        Ok(())
    }
}

/// Maximum bytes of stdout/stderr to retain per task execution (1 MB).
/// BUG-12 FIX: Prevents OOM from tasks that produce unbounded console output.
const MAX_LOG_BYTES: usize = 1_048_576;

/// Truncate a string to MAX_LOG_BYTES, appending a marker if truncated.
fn truncate_log(s: String) -> String {
    if s.len() > MAX_LOG_BYTES {
        let mut truncated = s[..MAX_LOG_BYTES].to_string();
        truncated.push_str("\n... [TRUNCATED — output exceeded 1 MB] ...");
        truncated
    } else {
        s
    }
}

/// A macro for plugins to declare their export function easily.
#[macro_export]
macro_rules! declare_plugin {
    ($plugin_type:ty, $constructor:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _vortex_plugin_create() -> *mut dyn $crate::executor::VortexOperator {
            let constructor: fn() -> $plugin_type = $constructor;
            let object = constructor();
            let boxed: Box<dyn $crate::executor::VortexOperator> = Box::new(object);
            Box::into_raw(boxed)
        }
    };
}

pub struct HttpOperator;

#[async_trait::async_trait]
impl VortexOperator for HttpOperator {
    async fn execute(&self, context: &TaskContext) -> Result<ExecutionResult> {
        let start = Instant::now();
        let client = reqwest::Client::new();
        
        let url = context.config.get("endpoint").and_then(|v| v.as_str()).unwrap_or(&context.command);
        if url.is_empty() {
             return Err(anyhow!("HTTP operator: endpoint/command is empty"));
        }
        let method = context.config.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
        
        let mut req = match method.to_uppercase().as_str() {
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            _ => client.get(url),
        };
        
        if let Some(headers) = context.config.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(vs) = v.as_str() {
                    req = req.header(k, vs);
                }
            }
        }
        
        if let Some(body) = context.config.get("data") {
            req = req.json(body);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let success = status.is_success();
                let text = resp.text().await.unwrap_or_default();
                let exit_code = if success { 0 } else { status.as_u16() as i32 };
                
                Ok(ExecutionResult {
                    task_id: context.task_id.clone(),
                    success,
                    exit_code,
                    stdout: text,
                    stderr: if success { String::new() } else { format!("HTTP Error: {}", status) },
                    duration_ms: start.elapsed().as_millis() as u64,
                })
            }
            Err(e) => {
                Ok(ExecutionResult {
                    task_id: context.task_id.clone(),
                    success: false,
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Request failed: {}", e),
                    duration_ms: start.elapsed().as_millis() as u64,
                })
            }
        }
    }
}

#[derive(Debug, Clone)]

pub struct ExecutionResult {
    pub task_id: String,
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

/// Injects VORTEX context env vars (for XCom, etc.) into the command's environment.
fn inject_vortex_env(cmd: &mut Command, env_vars: &HashMap<String, String>) {
    cmd.envs(env_vars.iter());
    // Always inject VORTEX_BASE_URL for XCom/pool helpers
    if !env_vars.contains_key("VORTEX_BASE_URL") {
        cmd.env("VORTEX_BASE_URL", std::env::var("VORTEX_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()));
    }
    // NOTE: We intentionally do NOT inject a default admin API key here.
    // Tasks should be given scoped, read-only tokens via their env_vars
    // if they need API access. Injecting the admin key is a security risk.
    if !env_vars.contains_key("VORTEX_API_KEY") {
        if let Ok(key) = std::env::var("VORTEX_TASK_API_KEY") {
            // Use a dedicated task-scoped key if configured
            cmd.env("VORTEX_API_KEY", key);
        }
        // If no task key is configured, tasks won't have API access — by design.
    }
}


pub struct TaskExecutor;

impl TaskExecutor {
    pub async fn execute_bash(
        task_id: &str,
        bash_command: &str,
        env_vars: HashMap<String, String>,
        timeout_secs: Option<u64>,
    ) -> ExecutionResult {
        let start = Instant::now();
        
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(bash_command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);  // BUG-2 FIX: kill child process if the future is dropped
        inject_vortex_env(&mut cmd, &env_vars);

        let timeout_duration = timeout_secs.unwrap_or(300);
        let result = timeout(Duration::from_secs(timeout_duration), cmd.output()).await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let success = output.status.success();
                let exit_code = output.status.code().unwrap_or(if success { 0 } else { 1 });
                ExecutionResult {
                    task_id: task_id.to_string(),
                    success,
                    exit_code,
                    stdout: truncate_log(String::from_utf8_lossy(&output.stdout).to_string()),
                    stderr: truncate_log(String::from_utf8_lossy(&output.stderr).to_string()),
                    duration_ms,
                }
            }
            Ok(Err(e)) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to execute command: {}", e),
                duration_ms,
            },
            Err(_) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -2,
                stdout: String::new(),
                stderr: format!("Task timed out after {} seconds", timeout_duration),
                duration_ms,
            },
        }
    }

    pub async fn execute_python(
        task_id: &str,
        python_code: &str,
        env_vars: HashMap<String, String>,
        timeout_secs: Option<u64>,
    ) -> ExecutionResult {
        let start = Instant::now();

        // Bug 21 fix: the old code detected a bare function name (no spaces/parens)
        // and generated `def fn(): pass; fn()` — an empty no-op stub that never
        // executed the real function. Without knowing which module the function
        // lives in we cannot import it, so we treat this case as an unsupported
        // invocation and return a clear error rather than silently succeeding.
        //
        // If the caller wants to invoke a Python function they should provide
        // either: (a) the full script including the function definition, or
        // (b) an importable call expression like `from my_dag import fn; fn()`.
        let is_bare_function_name = !python_code.contains('\n')
            && !python_code.contains('(')
            && !python_code.contains(' ')
            && !python_code.is_empty();

        if is_bare_function_name {
            return ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!(
                    "Bug 21 fix: bare function name '{}' cannot be executed without its module. \
                     Provide a full script or an import expression like \
                     'from my_module import {}; {}()'",
                    python_code, python_code, python_code
                ),
                duration_ms: 0,
            };
        }

        let full_script = python_code.to_string();

        // Create a temporary file for the Python code
        let temp_file = match NamedTempFile::new() {
            Ok(file) => file,
            Err(e) => {
                return ExecutionResult {
                    task_id: task_id.to_string(),
                    success: false,
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("Failed to create temp file: {}", e),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        let temp_path = temp_file.path().to_path_buf();
        if let Err(e) = std::fs::write(&temp_path, &full_script) {
            return ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to write to temp file: {}", e),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }

        let mut cmd = Command::new("python3");
        cmd.arg(&temp_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);  // BUG-2 FIX: kill child process if the future is dropped
        inject_vortex_env(&mut cmd, &env_vars);

        let timeout_duration = timeout_secs.unwrap_or(300);
        let result = timeout(Duration::from_secs(timeout_duration), cmd.output()).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let success = output.status.success();
                let exit_code = output.status.code().unwrap_or(if success { 0 } else { 1 });
                ExecutionResult {
                    task_id: task_id.to_string(),
                    success,
                    exit_code,
                    stdout: truncate_log(String::from_utf8_lossy(&output.stdout).to_string()),
                    stderr: truncate_log(String::from_utf8_lossy(&output.stderr).to_string()),
                    duration_ms,
                }
            }
            Ok(Err(e)) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Failed to execute python: {}", e),
                duration_ms,
            },
            Err(_) => ExecutionResult {
                task_id: task_id.to_string(),
                success: false,
                exit_code: -2,
                stdout: String::new(),
                stderr: format!("Task timed out after {} seconds", timeout_duration),
                duration_ms,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_bash_success() {
        let env_vars = HashMap::new();
        let res = TaskExecutor::execute_bash("t1", "echo 'hello world'", env_vars, Some(5)).await;
        
        assert!(res.success);
        assert_eq!(res.exit_code, 0);
        assert_eq!(res.stdout.trim(), "hello world");
    }

    #[tokio::test]
    async fn test_execute_bash_failure() {
        let env_vars = HashMap::new();
        let res = TaskExecutor::execute_bash("t2", "ls /nonexistent_directory_here", env_vars, Some(5)).await;
        
        assert!(!res.success);
        assert_ne!(res.exit_code, 0);
        assert!(res.stderr.contains("No such file or directory") || res.stderr.contains("doesn't exist"));
    }

    #[tokio::test]
    async fn test_execute_bash_timeout() {
        let env_vars = HashMap::new();
        let res = TaskExecutor::execute_bash("t3", "sleep 3", env_vars, Some(1)).await;
        
        assert!(!res.success);
        assert_eq!(res.exit_code, -2);
        assert!(res.stderr.contains("timed out"));
    }

    #[tokio::test]
    async fn test_execute_python_success() {
        let env_vars = HashMap::new();
        let code = "print('hello python')\n";
        let res = TaskExecutor::execute_python("t4", code, env_vars, Some(5)).await;
        
        assert!(res.success);
        assert_eq!(res.exit_code, 0);
        assert_eq!(res.stdout.trim(), "hello python");
    }

    #[tokio::test]
    async fn test_execute_python_bug21_bare_function() {
        let env_vars = HashMap::new();
        // A bare function name with no spaces or parens should be rejected
        let code = "my_function";
        let res = TaskExecutor::execute_python("t5", code, env_vars, Some(5)).await;
        
        assert!(!res.success);
        assert_eq!(res.exit_code, -1);
        assert!(res.stderr.contains("bare function name"));
        assert!(res.stderr.contains("my_function"));
    }

    #[tokio::test]
    async fn test_env_injection() {
        let mut env_vars = HashMap::new();
        env_vars.insert("CUSTOM_VAR".to_string(), "custom_value".to_string());
        
        let res = TaskExecutor::execute_bash("t6", "echo $CUSTOM_VAR", env_vars, Some(5)).await;
        
        assert!(res.success);
        assert_eq!(res.stdout.trim(), "custom_value");
    }
}
