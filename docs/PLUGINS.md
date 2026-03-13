# VORTEX Plugins Guide

VORTEX supports extensibility through a dynamic plugin system. You can write custom implementations of the `VortexOperator` trait, compile them as shared libraries (`.so` or `.dylib`), and drop them into the `plugins/` directory.

## 1. Implement the `VortexOperator` Trait

A plugin must implement the `VortexOperator` trait, which executes the task logic within a given `TaskContext`.

```rust
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct TaskContext {
    pub dag_id: String,
    pub task_id: String,
    pub config: serde_json::Value,
}

pub trait VortexOperator: Send + Sync {
    fn execute(&self, context: &TaskContext) -> Result<String, String>;
}
```

## 2. Use the `declare_plugin!` Macro

Once you have your custom struct implementing `VortexOperator`, use the `declare_plugin!` macro to export it using Rust's C-ABI compatibility. This allows VORTEX's `PluginRegistry` to safely load it dynamically at engine boot.

```rust
// In your plugin's lib.rs
pub struct MyCustomOperator;

impl VortexOperator for MyCustomOperator {
    fn execute(&self, context: &TaskContext) -> Result<String, String> {
        println!("Executing MyCustomOperator for task {}", context.task_id);
        Ok("Success!".to_string())
    }
}

// Export the plugin under a unique identifier
declare_plugin!("my_custom_task", MyCustomOperator);
```

## 3. Compile as a Dynamic Library

Update your plugin's `Cargo.toml` to compile as a `cdylib`.

```toml
[lib]
crate-type = ["cdylib"]
```

Build the project:
```bash
cargo build --release
```

## 4. Install the Plugin

Copy the resulting shared library (e.g., `libmy_custom_plugin.so` or `libmy_custom_plugin.dylib` on macOS) into your VORTEX root's `plugins/` directory:

```bash
cp target/release/libmy_custom_plugin.dylib /path/to/vortex/plugins/
```

When you start VORTEX **with the `--allow-unsafe-plugins` flag**, it will automatically scan the `plugins/` directory, load the shared library, and register `my_custom_task` as an official executor. You can now use this task type in your Python DAG definitions.

```bash
# Start VORTEX and explicitly opt-in to loading dynamic plugins
./target/release/vortex server --swarm --database-url "postgres://..." --allow-unsafe-plugins
```

## ⚠️ Security Warning

Plugin loading uses `unsafe` Rust to dynamically load shared libraries. There is **no sandboxing** — plugins have full process memory access and system call permissions. Always follow these rules:

- **Only load plugins from trusted, verified sources.** A malicious plugin can exfiltrate secrets, modify data, or crash the process.
- **Load plugins at startup only**, before the server begins serving requests. The `load_plugin` function must not be called concurrently with any `get_plugin` call (convention-enforced, not type-enforced).
- Future work: migrate plugins to subprocess isolation with restricted capabilities.

## Built-in Task Types

Before writing a plugin, check if your use case is covered by built-in task types:

| Type | Description |
|------|-------------|
| `bash` | Execute shell commands via `sh -c` |
| `python` | Execute Python scripts via `python3` |
| `http` | Make HTTP requests with configurable method, URL, headers, and body |

Plugins are useful for custom integrations (database operators, cloud service operators, etc.) that aren't covered by the built-in types.
