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

When you start VORTEX, it will automatically scan the `plugins/` directory, load the shared library, and register `my_custom_task` as an official executor. You can now use this task type in your Python DAG definitions.
