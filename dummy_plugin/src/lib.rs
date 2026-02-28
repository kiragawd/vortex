use vortex::executor::{VortexOperator, TaskContext, ExecutionResult};
use vortex::declare_plugin;
use anyhow::Result;

pub struct DummyOperator;

#[async_trait::async_trait]
impl VortexOperator for DummyOperator {
    async fn execute(&self, _ctx: &TaskContext) -> Result<ExecutionResult> {
        Ok(ExecutionResult {
            task_id: _ctx.task_id.clone(),
            success: true,
            exit_code: 0,
            stdout: "Dummy Success".into(),
            stderr: "".into(),
            duration_ms: 10,
        })
    }
}

impl DummyOperator {
    pub fn new() -> Self {
        DummyOperator
    }
}

declare_plugin!(DummyOperator, DummyOperator::new);

