//! Mock MCP server binary for integration tests
//!
//! This implements a minimal MCP server that responds to:
//! - initialize: Returns server capabilities
//! - tools/list: Returns mock tools (echo, add, fail)
//! - tools/call: Executes mock tools

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

fn handle_initialize(id: Option<Value>) -> Option<JsonRpcResponse> {
    Some(JsonRpcResponse::success(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {"name": "mock-server", "version": "1.0.0"},
            "capabilities": {"tools": {}}
        }),
    ))
}

fn handle_list_tools(id: Option<Value>) -> Option<JsonRpcResponse> {
    Some(JsonRpcResponse::success(
        id,
        json!({
            "tools": [
                {
                    "name": "echo",
                    "description": "Echo back the input",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "message": {"type": "string", "description": "Message to echo"}
                        },
                        "required": ["message"]
                    }
                },
                {
                    "name": "add",
                    "description": "Add two numbers",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "a": {"type": "number"},
                            "b": {"type": "number"}
                        },
                        "required": ["a", "b"]
                    }
                },
                {
                    "name": "fail",
                    "description": "A tool that always fails",
                    "inputSchema": {"type": "object", "properties": {}}
                }
            ]
        }),
    ))
}

fn handle_call_tool(id: Option<Value>, params: &Value) -> Option<JsonRpcResponse> {
    let tool_name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];

    match tool_name {
        "echo" => {
            let message = args["message"].as_str().unwrap_or("");
            Some(JsonRpcResponse::success(
                id,
                json!({
                    "content": [{"type": "text", "text": message}],
                    "isError": false
                }),
            ))
        }
        "add" => {
            let a = args["a"].as_f64().unwrap_or(0.0);
            let b = args["b"].as_f64().unwrap_or(0.0);
            let result = a + b;
            let text = if result.fract() == 0.0 {
                format!("{}", result as i64)
            } else {
                format!("{}", result)
            };
            Some(JsonRpcResponse::success(
                id,
                json!({
                    "content": [{"type": "text", "text": text}],
                    "isError": false
                }),
            ))
        }
        "fail" => Some(JsonRpcResponse::success(
            id,
            json!({
                "content": [{"type": "text", "text": "This tool always fails"}],
                "isError": true
            }),
        )),
        _ => Some(JsonRpcResponse::error(
            id,
            -32601,
            format!("Unknown tool: {}", tool_name),
        )),
    }
}

fn handle_request(req: JsonRpcRequest) -> Option<JsonRpcResponse> {
    match req.method.as_str() {
        "initialize" => handle_initialize(req.id),
        "notifications/initialized" => None,
        "tools/list" => handle_list_tools(req.id),
        "tools/call" => handle_call_tool(req.id, &req.params),
        _ => Some(JsonRpcResponse::error(
            req.id,
            -32601,
            format!("Method not found: {}", req.method),
        )),
    }
}

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => handle_request(req),
            Err(e) => Some(JsonRpcResponse::error(
                None,
                -32700,
                format!("Parse error: {}", e),
            )),
        };

        if let Some(resp) = response {
            let json = serde_json::to_string(&resp).expect("Failed to serialize response");
            writeln!(stdout, "{}", json).expect("Failed to write response");
            stdout.flush().expect("Failed to flush stdout");
        }
    }
}
