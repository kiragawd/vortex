//! MCP (Model Context Protocol) Tool Server for Ryuo
//!
//! Exposes orchestration operations as LLM-callable tools via JSON-RPC.
//! Supports stdio transport for local agent integration.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// MCP tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// MCP tool call request
#[derive(Debug, Deserialize)]
pub struct McpToolCall {
    pub name: String,
    pub arguments: HashMap<String, Value>,
}

/// MCP tool call response
#[derive(Debug, Serialize)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    pub is_error: bool,
}

#[derive(Debug, Serialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Registry of available MCP tools
pub fn get_tool_definitions() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "dag_list".into(),
            description: "List all registered DAGs with their status, schedule, and team".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "description": "Max DAGs to return", "default": 100}
                }
            }),
        },
        McpTool {
            name: "dag_get".into(),
            description: "Get detailed information about a specific DAG".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "dag_id": {"type": "string", "description": "The DAG identifier"}
                },
                "required": ["dag_id"]
            }),
        },
        McpTool {
            name: "dag_trigger".into(),
            description: "Trigger a DAG run, optionally with config overrides".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "dag_id": {"type": "string"},
                    "config": {"type": "object", "description": "Runtime config overrides"}
                },
                "required": ["dag_id"]
            }),
        },
        McpTool {
            name: "dag_runs".into(),
            description: "List run history for a DAG".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "dag_id": {"type": "string"},
                    "limit": {"type": "integer", "default": 20},
                    "state": {"type": "string", "description": "Filter by state"}
                },
                "required": ["dag_id"]
            }),
        },
        McpTool {
            name: "dataset_freshness".into(),
            description: "Check how fresh a dataset is (last update time, age in seconds)".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "uri": {"type": "string", "description": "Dataset URI"},
                    "stale_after": {"type": "integer", "description": "Stale threshold in seconds"}
                }
            }),
        },
        McpTool {
            name: "xcom_pull".into(),
            description: "Pull a value from XCom inter-task data store".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "dag": {"type": "string"},
                    "task": {"type": "string"},
                    "run": {"type": "string"},
                    "key": {"type": "string"}
                },
                "required": ["dag", "task", "run", "key"]
            }),
        },
        McpTool {
            name: "connector_query".into(),
            description: "Execute a read-only SQL query through a registered connector".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "connector": {"type": "string", "description": "Connector name (e.g., postgres)"},
                    "sql": {"type": "string", "description": "SQL SELECT query"}
                },
                "required": ["connector", "sql"]
            }),
        },
        McpTool {
            name: "pool_list".into(),
            description: "List all execution pools with slot usage".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        McpTool {
            name: "queue_list".into(),
            description: "List the current task queue ordered by priority".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "default": 100}
                }
            }),
        },
        McpTool {
            name: "agent_state_get".into(),
            description: "Get a value from the agent state store".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "default": "default"},
                    "key": {"type": "string"}
                },
                "required": ["key"]
            }),
        },
        McpTool {
            name: "agent_state_set".into(),
            description: "Set a value in the agent state store".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "default": "default"},
                    "key": {"type": "string"},
                    "value": {"type": "string"},
                    "ttl": {"type": "integer", "description": "TTL in seconds"}
                },
                "required": ["key", "value"]
            }),
        },
        McpTool {
            name: "audit_recent".into(),
            description: "Query recent audit events".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "default": 50}
                }
            }),
        },
    ]
}

/// Format tool definitions for MCP protocol response
pub fn format_tools_list() -> Value {
    let tools: Vec<Value> = get_tool_definitions()
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();
    serde_json::json!({ "tools": tools })
}

/// Dispatch an MCP tool call and return a structured result.
///
/// Validates that the requested tool exists and constructs a `McpToolResult`.
/// Actual execution requires a live database/runtime context that is not
/// available inside this module, so the handler returns a placeholder result
/// for now. The CLI and REST layers can extend this with real dispatch logic.
pub fn dispatch_tool_call(call: McpToolCall) -> McpToolResult {
    let tools = get_tool_definitions();
    match tools.iter().find(|t| t.name == call.name) {
        Some(tool) => McpToolResult {
            content: vec![McpContent {
                content_type: "text".into(),
                text: serde_json::json!({
                    "tool": tool.name,
                    "arguments": call.arguments,
                    "status": "dispatched",
                    "message": format!("Tool '{}' accepted — execution requires a live runtime context", tool.name),
                }).to_string(),
            }],
            is_error: false,
        },
        None => {
            let available: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            McpToolResult {
                content: vec![McpContent {
                    content_type: "text".into(),
                    text: serde_json::json!({
                        "error": format!("Unknown tool: '{}'", call.name),
                        "available_tools": available,
                    }).to_string(),
                }],
                is_error: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_tool_definitions_not_empty() {
        let tools = get_tool_definitions();
        assert!(!tools.is_empty());
        // Every tool must have a non-empty name and description
        for tool in &tools {
            assert!(!tool.name.is_empty(), "Tool name must not be empty");
            assert!(!tool.description.is_empty(), "Tool description must not be empty");
            assert!(tool.input_schema.is_object(), "input_schema must be an object");
        }
    }

    #[test]
    fn test_format_tools_list_structure() {
        let list = format_tools_list();
        let tools = list["tools"].as_array().expect("tools should be an array");
        assert!(!tools.is_empty());
        for tool in tools {
            assert!(tool["name"].is_string());
            assert!(tool["description"].is_string());
            assert!(tool["inputSchema"].is_object());
        }
    }

    #[test]
    fn test_tool_names_unique() {
        let tools = get_tool_definitions();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "Tool names must be unique");
    }

    #[test]
    fn test_mcp_tool_call_deserialize() {
        let json = r#"{"name":"dag_list","arguments":{"limit":10}}"#;
        let call: McpToolCall = serde_json::from_str(json).unwrap();
        assert_eq!(call.name, "dag_list");
        assert_eq!(call.arguments["limit"], 10);
    }

    #[test]
    fn test_mcp_tool_result_serialize() {
        let result = McpToolResult {
            content: vec![McpContent {
                content_type: "text".into(),
                text: "hello".into(),
            }],
            is_error: false,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["is_error"], false);
        assert_eq!(json["content"][0]["type"], "text");
        assert_eq!(json["content"][0]["text"], "hello");
    }
}
