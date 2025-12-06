//! MCP (Model Context Protocol) server integration
//!
//! This module provides support for connecting to MCP servers and using their tools
//! within mixtape agents. You can configure MCP servers either programmatically or
//! by loading them from standard MCP JSON configuration files (compatible with
//! Claude Desktop and Claude Code).
//!
//! # Examples
//!
//! ## Programmatic configuration
//!
//! ```rust,no_run
//! use mixtape_core::mcp::{McpServerConfig, McpTransport};
//!
//! // Basic stdio transport (most MCP servers)
//! let config = McpServerConfig::new("filesystem",
//!     McpTransport::stdio("npx")
//!         .args(["-y", "@modelcontextprotocol/server-filesystem"])
//! );
//!
//! // With tool filtering
//! let config = McpServerConfig::new("filesystem",
//!     McpTransport::stdio("npx")
//!         .args(["-y", "@modelcontextprotocol/server-filesystem"])
//! )
//! .only_tools(["read_file", "write_file"]);
//!
//! // HTTP transport with authentication
//! let config = McpServerConfig::new("api",
//!     McpTransport::http("https://api.example.com/mcp")
//!         .header("Authorization", "Bearer token")
//! );
//! ```
//!
//! ## From JSON config file
//!
//! ```ignore
//! use mixtape_core::{Agent, ClaudeSonnet4_5};
//!
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4_5)
//!     .with_mcp_config_file("~/.claude.json")
//!     .build()
//!     .await?;
//! ```

mod client;
mod config;
pub(crate) mod tool_adapter;
mod transport;

pub use client::McpClient;
pub use config::{load_config_file, McpConfigFile, McpServerEntry};
pub use transport::{HttpBuilder, McpServerConfig, McpTransport, StdioBuilder};

use thiserror::Error;

/// Errors that can occur during MCP operations
#[derive(Debug, Error)]
pub enum McpError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("MCP protocol error: {0}")]
    Protocol(String),
}
