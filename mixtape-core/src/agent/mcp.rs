//! MCP (Model Context Protocol) configuration for agents
//!
//! This module contains all MCP-related configuration methods:
//! - Builder methods for configuring MCP servers at construction time
//! - Post-construction methods for dynamically adding MCP servers

use std::sync::Arc;

use super::builder::AgentBuilder;
use super::Agent;
use crate::mcp::tool_adapter::McpToolAdapter;
use crate::mcp::{load_config_file, McpClient, McpError, McpServerConfig};

// ============================================================================
// AgentBuilder MCP configuration methods
// ============================================================================

impl AgentBuilder {
    /// Add an MCP server to the agent
    ///
    /// The server will be connected when `.build().await` is called.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::mcp::{McpServerConfig, McpTransport};
    ///
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .with_mcp_server(McpServerConfig::new(
    ///         "filesystem",
    ///         McpTransport::stdio("npx")
    ///             .args(["-y", "@modelcontextprotocol/server-filesystem"])
    ///     ))
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_mcp_server(mut self, config: McpServerConfig) -> Self {
        self.mcp_servers.push(config);
        self
    }

    /// Add tools from MCP servers defined in a configuration file
    ///
    /// The file will be loaded and servers connected when `.build().await` is called.
    /// Supports Claude Desktop/Code config file format with environment variable expansion.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .with_mcp_config_file("~/.claude.json")
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_mcp_config_file(mut self, path: impl AsRef<std::path::Path>) -> Self {
        self.mcp_config_files.push(path.as_ref().to_path_buf());
        self
    }
}

// ============================================================================
// Helper function for connecting MCP servers during build
// ============================================================================

/// Connect to MCP servers and add their tools to the agent
///
/// This is called from AgentBuilder::build() to set up MCP connections.
pub(super) async fn connect_mcp_servers(
    agent: &mut Agent,
    servers: Vec<McpServerConfig>,
    config_files: Vec<std::path::PathBuf>,
) -> Result<(), crate::error::Error> {
    // Connect to individually specified servers
    for config in servers {
        let client = Arc::new(
            McpClient::new(config.clone()).map_err(|e| crate::error::Error::Mcp(e.to_string()))?,
        );
        let tools = client
            .list_tools()
            .await
            .map_err(|e| crate::error::Error::Mcp(e.to_string()))?;

        for tool_def in tools {
            if config.should_include_tool(&tool_def.name) {
                let adapter = if let Some(namespace) = config.namespace() {
                    McpToolAdapter::new_with_namespace(Arc::clone(&client), tool_def, namespace)
                } else {
                    McpToolAdapter::new(Arc::clone(&client), tool_def)
                };
                agent.add_tool(adapter);
            }
        }
        agent.mcp_clients.push(client);
    }

    // Connect to servers from config files
    for path in config_files {
        let server_configs = load_config_file(&path)
            .await
            .map_err(|e| crate::error::Error::Mcp(e.to_string()))?;

        for config in server_configs {
            let client = Arc::new(
                McpClient::new(config.clone())
                    .map_err(|e| crate::error::Error::Mcp(e.to_string()))?,
            );
            let tools = client
                .list_tools()
                .await
                .map_err(|e| crate::error::Error::Mcp(e.to_string()))?;

            for tool_def in tools {
                if config.should_include_tool(&tool_def.name) {
                    let adapter = McpToolAdapter::new(Arc::clone(&client), tool_def);
                    agent.add_tool(adapter);
                }
            }
            agent.mcp_clients.push(client);
        }
    }

    Ok(())
}

// ============================================================================
// Agent post-construction MCP methods
// ============================================================================

impl Agent {
    /// Add tools from an MCP server after construction
    ///
    /// Use this to dynamically add MCP servers at runtime. For initial
    /// configuration, prefer using the builder:
    /// `Agent::builder().with_mcp_server(config).build().await`
    ///
    /// # Example
    /// ```ignore
    /// use mixtape_core::mcp::{McpServerConfig, McpTransport};
    ///
    /// let config = McpServerConfig::new(
    ///     "filesystem",
    ///     McpTransport::stdio("npx")
    ///         .args(["-y", "@modelcontextprotocol/server-filesystem"])
    /// );
    ///
    /// agent.add_mcp_server(config).await?;
    /// ```
    pub async fn add_mcp_server(&mut self, config: McpServerConfig) -> Result<(), McpError> {
        let client = Arc::new(McpClient::new(config.clone())?);
        let tools = client.list_tools().await?;

        // Filter tools based on the config's tool filter
        for tool_def in tools {
            if config.should_include_tool(&tool_def.name) {
                let adapter = if let Some(namespace) = config.namespace() {
                    McpToolAdapter::new_with_namespace(Arc::clone(&client), tool_def, namespace)
                } else {
                    McpToolAdapter::new(Arc::clone(&client), tool_def)
                };
                self.add_tool(adapter);
            }
        }

        // Store client for shutdown cleanup
        self.mcp_clients.push(client);

        Ok(())
    }

    /// Add tools from MCP servers defined in a configuration file
    ///
    /// Use this to dynamically load MCP config at runtime. For initial
    /// configuration, prefer using the builder:
    /// `Agent::builder().with_mcp_config_file(path).build().await`
    ///
    /// # Example
    /// ```ignore
    /// agent.add_mcp_config_file("~/.claude.json").await?;
    /// ```
    pub async fn add_mcp_config_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), McpError> {
        let server_configs = load_config_file(path).await?;

        for config in server_configs {
            let client = Arc::new(McpClient::new(config.clone())?);
            let tools = client.list_tools().await?;

            // Filter tools based on the config's tool filter
            for tool_def in tools {
                if config.should_include_tool(&tool_def.name) {
                    let adapter = McpToolAdapter::new(Arc::clone(&client), tool_def);
                    self.add_tool(adapter);
                }
            }

            // Store client for shutdown cleanup
            self.mcp_clients.push(client);
        }

        Ok(())
    }
}
