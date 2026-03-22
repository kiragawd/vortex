use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use vortex::agentic::{
    CompletionRequest, CompletionResponse, LlmProvider, convert_dbt_manifest_to_pipeline,
    translate_python_to_rust_agentic,
};

struct FakeProvider;

#[async_trait]
impl LlmProvider for FakeProvider {
    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            content: "fn translated() -> anyhow::Result<()> { Ok(()) }".to_string(),
            model: "fake".to_string(),
            usage: HashMap::new(),
        })
    }
}

#[tokio::test]
async fn test_python_to_rust_agentic_loop() {
    let provider = FakeProvider;
    let rust = translate_python_to_rust_agentic(&provider, "def f():\n  return 1", 2)
        .await
        .expect("agentic translation should succeed");
    assert!(rust.contains("fn "));
}

#[test]
fn test_dbt_manifest_conversion() {
    let manifest = r#"{
      "nodes": {
        "model.pkg.orders": {
          "name": "orders",
          "raw_sql": "select * from raw_orders",
          "depends_on": {"nodes": ["source.pkg.raw_orders"]}
        }
      }
    }"#;

    let nodes = convert_dbt_manifest_to_pipeline(manifest).expect("manifest should parse");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name, "orders");
}
