use super::{McpError, McpServerConfig, McpTransport};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use rmcp::transport::TokioChildProcess;
use rmcp::{model::CallToolRequestParam, RoleClient, ServiceExt};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

/// MCP client wrapper that provides lazy connection and tool access
pub struct McpClient {
    name: String,
    config: McpServerConfig,
    service: Arc<RwLock<Option<RunningService<RoleClient, ()>>>>,
}

impl McpClient {
    /// Create a new MCP client from configuration
    ///
    /// The client is not connected until `connect()` is called or a method that requires
    /// connection is invoked (lazy connection pattern).
    pub fn new(config: McpServerConfig) -> Result<Self, McpError> {
        Ok(Self {
            name: config.name.clone(),
            config,
            service: Arc::new(RwLock::new(None)),
        })
    }

    /// Get the server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Connect to the MCP server
    ///
    /// This method is idempotent - calling it multiple times is safe and will not
    /// create multiple connections.
    pub async fn connect(&self) -> Result<(), McpError> {
        let mut service_guard = self.service.write().await;

        // Already connected
        if service_guard.is_some() {
            return Ok(());
        }

        // Create the service based on transport type
        let service: RunningService<RoleClient, ()> = match &self.config.transport {
            McpTransport::Stdio { command, args, env } => {
                let mut cmd = Command::new(command);

                // Add arguments
                for arg in args {
                    cmd.arg(arg);
                }

                // Add environment variables
                for (key, value) in env {
                    cmd.env(key, value);
                }

                // Create transport and serve
                let transport = TokioChildProcess::new(cmd).map_err(|e| {
                    McpError::Transport(format!("Failed to create child process: {}", e))
                })?;

                ().serve(transport).await.map_err(|e| {
                    McpError::Connection(format!("Failed to connect to server: {}", e))
                })?
            }
            McpTransport::Http { url, headers } => {
                // Create HTTP transport config
                let config = StreamableHttpClientTransportConfig::with_uri(url.clone());

                // Build reqwest client with custom headers
                let mut header_map = HeaderMap::new();
                for (key, value) in headers {
                    let header_name = HeaderName::try_from(key.as_str()).map_err(|e| {
                        McpError::Config(format!("Invalid header name '{}': {}", key, e))
                    })?;
                    let header_value = HeaderValue::try_from(value.as_str()).map_err(|e| {
                        McpError::Config(format!("Invalid header value for '{}': {}", key, e))
                    })?;
                    header_map.insert(header_name, header_value);
                }

                let http_client = reqwest::Client::builder()
                    .default_headers(header_map)
                    .build()
                    .map_err(|e| {
                        McpError::Transport(format!("Failed to create HTTP client: {}", e))
                    })?;

                // Create transport with custom client
                let transport = StreamableHttpClientTransport::with_client(http_client, config);

                ().serve(transport).await.map_err(|e| {
                    McpError::Connection(format!("Failed to connect to HTTP server: {}", e))
                })?
            }
        };

        *service_guard = Some(service);
        Ok(())
    }

    /// Ensure the client is connected (lazy connect)
    async fn ensure_connected(&self) -> Result<(), McpError> {
        self.connect().await
    }

    /// List available tools from the MCP server
    ///
    /// Returns a list of tool definitions including name, description, and input schema.
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>, McpError> {
        self.ensure_connected().await?;

        let service_guard = self.service.read().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| McpError::Connection("Not connected".to_string()))?;

        let result = service
            .list_tools(Default::default())
            .await
            .map_err(|e| McpError::Protocol(format!("Failed to list tools: {}", e)))?;

        Ok(result
            .tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
            })
            .collect())
    }

    /// Call a tool on the MCP server
    ///
    /// # Arguments
    /// * `name` - The name of the tool to call
    /// * `arguments` - JSON object containing the tool arguments
    pub async fn call_tool(
        &self,
        name: String,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        self.ensure_connected().await?;

        let service_guard = self.service.read().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| McpError::Connection("Not connected".to_string()))?;

        let params = CallToolRequestParam {
            name: name.into(),
            arguments: arguments.as_object().cloned(),
        };

        let result = service
            .call_tool(params)
            .await
            .map_err(|e| McpError::ToolExecution(format!("Tool execution failed: {}", e)))?;

        // Convert the result to JSON
        // The result contains a Vec<Content>, we'll serialize it
        serde_json::to_value(result).map_err(McpError::Json)
    }

    /// Disconnect from the MCP server
    ///
    /// After disconnection, the client can be reconnected by calling `connect()` again.
    pub async fn disconnect(&self) -> Result<(), McpError> {
        let mut service_guard = self.service.write().await;

        if let Some(service) = service_guard.take() {
            service
                .cancel()
                .await
                .map_err(|e| McpError::Connection(format!("Failed to disconnect: {}", e)))?;
        }

        Ok(())
    }
}

/// Tool definition from an MCP server
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// JSON Schema for the tool's input
    pub input_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_client_creation() {
        let config = McpServerConfig::new(
            "test-server",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                env: HashMap::new(),
            },
        );

        let client = McpClient::new(config.clone()).unwrap();
        assert_eq!(client.name(), "test-server");
    }

    #[test]
    fn test_http_transport_client_creation() {
        // HTTP transport is now supported - client creation should succeed
        let config = McpServerConfig::new(
            "http-server",
            McpTransport::http("https://example.com/mcp")
                .header("Authorization", "Bearer test-token"),
        );

        let client = McpClient::new(config).unwrap();
        assert_eq!(client.name(), "http-server");
    }

    #[tokio::test]
    async fn test_http_transport_connection_error() {
        // Connection to a non-MCP server should fail gracefully
        let config =
            McpServerConfig::new("http-server", McpTransport::http("https://example.com/mcp"));

        let client = McpClient::new(config).unwrap();
        let result = client.connect().await;

        // Should fail because example.com isn't an MCP server
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_definition_creation() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"}
            }
        });

        let def = ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from disk".to_string(),
            input_schema: schema.clone(),
        };

        assert_eq!(def.name, "read_file");
        assert_eq!(def.description, "Read a file from disk");
        assert_eq!(def.input_schema, schema);
    }

    #[tokio::test]
    async fn test_lazy_connection_not_connected_initially() {
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = McpClient::new(config).unwrap();

        // Client should be created but not connected
        let service_guard = client.service.read().await;
        assert!(service_guard.is_none());
    }

    #[test]
    fn test_config_preservation() {
        let config = McpServerConfig::new(
            "filtered-server",
            McpTransport::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".to_string()],
                env: HashMap::new(),
            },
        )
        .only_tools(["tool1", "tool2"]);

        let _client = McpClient::new(config.clone()).unwrap();

        // Verify the config is preserved (including filters)
        assert!(config.should_include_tool("tool1"));
        assert!(config.should_include_tool("tool2"));
        assert!(!config.should_include_tool("tool3"));
    }

    #[tokio::test]
    async fn test_disconnect_when_not_connected() {
        // Disconnecting when not connected should be a no-op (idempotent)
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = McpClient::new(config).unwrap();

        // Should succeed even though we never connected
        let result = client.disconnect().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_tools_without_connection_fails() {
        // Calling list_tools on an invalid command should fail at connection
        let config = McpServerConfig::new(
            "nonexistent",
            McpTransport::Stdio {
                command: "/nonexistent/command/that/does/not/exist".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = McpClient::new(config).unwrap();
        let result = client.list_tools().await;

        // Should fail because the command doesn't exist
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Should be a Transport error (failed to create child process)
        assert!(
            matches!(err, McpError::Transport(_)),
            "Expected Transport error, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_call_tool_without_connection_fails() {
        let config = McpServerConfig::new(
            "nonexistent",
            McpTransport::Stdio {
                command: "/nonexistent/command".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = McpClient::new(config).unwrap();
        let result = client
            .call_tool("some_tool".to_string(), serde_json::json!({}))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_http_invalid_header_name() {
        // Header names with invalid characters should fail
        let mut headers = HashMap::new();
        headers.insert("Invalid Header".to_string(), "value".to_string()); // Space is invalid

        let config = McpServerConfig::new(
            "bad-headers",
            McpTransport::Http {
                url: "https://example.com/mcp".to_string(),
                headers,
            },
        );

        let client = McpClient::new(config).unwrap();
        let result = client.connect().await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, McpError::Config(_)),
            "Expected Config error for invalid header, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_http_invalid_header_value() {
        // Header values with invalid characters (like newlines) should fail
        let mut headers = HashMap::new();
        headers.insert("X-Test".to_string(), "value\nwith\nnewlines".to_string());

        let config = McpServerConfig::new(
            "bad-headers",
            McpTransport::Http {
                url: "https://example.com/mcp".to_string(),
                headers,
            },
        );

        let client = McpClient::new(config).unwrap();
        let result = client.connect().await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, McpError::Config(_)),
            "Expected Config error for invalid header value, got {:?}",
            err
        );
    }

    #[tokio::test]
    async fn test_stdio_with_env_vars() {
        // Test that environment variables are properly configured
        let mut env = HashMap::new();
        env.insert("MY_VAR".to_string(), "my_value".to_string());
        env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

        let config = McpServerConfig::new(
            "env-test",
            McpTransport::Stdio {
                command: "/nonexistent".to_string(),
                args: vec!["arg1".to_string(), "arg2".to_string()],
                env,
            },
        );

        let client = McpClient::new(config).unwrap();
        // Will fail at execution, but verifies the config path is exercised
        let result = client.connect().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multiple_disconnect_calls() {
        // Multiple disconnects should be safe
        let config = McpServerConfig::new(
            "test",
            McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
        );

        let client = McpClient::new(config).unwrap();

        // Call disconnect multiple times - all should succeed
        assert!(client.disconnect().await.is_ok());
        assert!(client.disconnect().await.is_ok());
        assert!(client.disconnect().await.is_ok());
    }
}
