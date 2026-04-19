use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub system: String,
    pub prompt: String,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub model: String,
    pub usage: HashMap<String, u64>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
}

pub struct OpenAiProvider {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let client = reqwest::Client::new();
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": request.system},
                {"role": "user", "content": request.prompt}
            ],
            "temperature": request.temperature,
        });

        let res = client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let v: serde_json::Value = res.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(anyhow!("OpenAI request failed: {}", status));
        }

        let content = v.get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        Ok(CompletionResponse {
            content,
            model: self.model.clone(),
            usage: HashMap::new(),
        })
    }
}

pub struct AnthropicProvider {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let client = reqwest::Client::new();
        let body = json!({
            "model": self.model,
            "max_tokens": 2048,
            "messages": [{"role": "user", "content": format!("{}\n\n{}", request.system, request.prompt)}]
        });

        let res = client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;
        let status = res.status();
        let v: serde_json::Value = res.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(anyhow!("Anthropic request failed: {}", status));
        }
        let content = v.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        Ok(CompletionResponse {
            content,
            model: self.model.clone(),
            usage: HashMap::new(),
        })
    }
}

pub async fn translate_python_to_rust_agentic<P: LlmProvider + ?Sized>(
    provider: &P,
    python_fn: &str,
    max_retries: u32,
) -> Result<String> {
    let mut feedback = String::new();
    for _ in 0..max_retries {
        let prompt = format!(
            "Translate this Python function to idiomatic Rust with explicit Result handling. {}\n\n{}",
            feedback, python_fn
        );
        let response = provider
            .complete(CompletionRequest {
                system: "You are a Rust migration engine".to_string(),
                prompt,
                temperature: 0.1,
            })
            .await?;

        if response.content.contains("fn ") && response.content.contains("Result") {
            return Ok(response.content);
        }
        feedback = "Previous output failed validation; include a Rust fn returning Result.".to_string();
    }
    Err(anyhow!("Failed to translate Python after retry budget"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbtModelNode {
    pub name: String,
    pub sql: String,
    pub depends_on: Vec<String>,
}

pub fn convert_dbt_manifest_to_pipeline(manifest_json: &str) -> Result<Vec<DbtModelNode>> {
    let v: serde_json::Value = serde_json::from_str(manifest_json)?;
    let mut out = Vec::new();
    let nodes = v
        .get("nodes")
        .and_then(|x| x.as_object())
        .ok_or_else(|| anyhow!("manifest missing nodes"))?;

    for n in nodes.values() {
        let name = n
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string();
        let sql = n
            .get("raw_sql")
            .or_else(|| n.get("compiled_sql"))
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let depends_on = n
            .get("depends_on")
            .and_then(|x| x.get("nodes"))
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| i.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push(DbtModelNode {
            name,
            sql,
            depends_on,
        });
    }
    Ok(out)
}
