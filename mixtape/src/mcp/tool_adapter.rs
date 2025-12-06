use super::client::{McpClient, ToolDefinition};
use crate::tool::{Tool, ToolError, ToolResult};
use std::sync::Arc;

/// Adapter that wraps an MCP tool as a mixtape Tool
pub struct McpToolAdapter {
    client: Arc<McpClient>,
    definition: ToolDefinition,
    /// The original tool name (before namespacing)
    original_name: String,
}

impl McpToolAdapter {
    /// Create a new adapter for an MCP tool
    pub fn new(client: Arc<McpClient>, definition: ToolDefinition) -> Self {
        let original_name = definition.name.clone();
        Self {
            client,
            definition,
            original_name,
        }
    }

    /// Create a new adapter with a namespace prefix
    pub fn new_with_namespace(
        client: Arc<McpClient>,
        definition: ToolDefinition,
        namespace: &str,
    ) -> Self {
        let original_name = definition.name.clone();
        let namespaced_name = format!("{}{}", namespace, definition.name);

        // Update the definition with the namespaced name
        let mut namespaced_def = definition;
        namespaced_def.name = namespaced_name;

        Self {
            client,
            definition: namespaced_def,
            original_name,
        }
    }

    /// Get the tool definition
    #[cfg(test)]
    pub fn definition(&self) -> &ToolDefinition {
        &self.definition
    }
}

impl Tool for McpToolAdapter {
    // MCP tools accept dynamic JSON input
    type Input = serde_json::Value;

    fn name(&self) -> &str {
        &self.definition.name
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Call the MCP server with the original (un-namespaced) tool name
        let result = self
            .client
            .call_tool(self.original_name.clone(), input)
            .await
            .map_err(|e| ToolError::Custom(format!("MCP tool error: {}", e)))?;

        // Return the result as JSON
        Ok(ToolResult::Json(result))
    }

    fn input_schema(&self) -> serde_json::Value {
        self.definition.input_schema.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpServerConfig, McpTransport};
    use std::collections::HashMap;

    #[test]
    fn test_adapter_metadata_forwarding() {
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = Arc::new(McpClient::new(config).unwrap());

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path"]
        });

        let definition = ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file".to_string(),
            input_schema: schema.clone(),
        };

        let adapter = McpToolAdapter::new(client, definition.clone());

        // Verify metadata is correctly forwarded
        assert_eq!(adapter.name(), "write_file");
        assert_eq!(adapter.description(), "Write content to a file");
        assert_eq!(adapter.input_schema(), schema);
        assert_eq!(adapter.definition().name, "write_file");
    }

    #[test]
    fn test_adapter_schema_preservation() {
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = Arc::new(McpClient::new(config).unwrap());

        let complex_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "User name"
                },
                "age": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 150
                },
                "email": {
                    "type": "string",
                    "format": "email"
                }
            },
            "required": ["name", "email"]
        });

        let definition = ToolDefinition {
            name: "create_user".to_string(),
            description: "Create a new user".to_string(),
            input_schema: complex_schema.clone(),
        };

        let adapter = McpToolAdapter::new(client, definition);

        // Verify complex schema is preserved exactly
        assert_eq!(adapter.input_schema(), complex_schema);
    }

    #[test]
    fn test_adapter_with_empty_description() {
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = Arc::new(McpClient::new(config).unwrap());

        let definition = ToolDefinition {
            name: "test_tool".to_string(),
            description: String::new(),
            input_schema: serde_json::json!({}),
        };

        let adapter = McpToolAdapter::new(client, definition);

        assert_eq!(adapter.name(), "test_tool");
        assert_eq!(adapter.description(), "");
    }

    #[test]
    fn test_multiple_adapters_same_client() {
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = Arc::new(McpClient::new(config).unwrap());

        let def1 = ToolDefinition {
            name: "tool1".to_string(),
            description: "First tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };

        let def2 = ToolDefinition {
            name: "tool2".to_string(),
            description: "Second tool".to_string(),
            input_schema: serde_json::json!({"type": "string"}),
        };

        let adapter1 = McpToolAdapter::new(Arc::clone(&client), def1);
        let adapter2 = McpToolAdapter::new(Arc::clone(&client), def2);

        // Both adapters should work independently
        assert_eq!(adapter1.name(), "tool1");
        assert_eq!(adapter2.name(), "tool2");
        assert_ne!(adapter1.input_schema(), adapter2.input_schema());
    }
}
