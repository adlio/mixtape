//! Agent-related types

use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

use crate::provider::ProviderError;
use crate::tool::ToolError;

use super::context::ContextError;

#[cfg(feature = "session")]
use crate::session::SessionError;

/// Errors that can occur during agent execution
#[derive(Debug, Error)]
pub enum AgentError {
    /// Model provider errors (API calls, authentication, rate limits)
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Tool execution errors
    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    /// Session storage errors
    #[cfg(feature = "session")]
    #[error("Session error: {0}")]
    Session(#[from] SessionError),

    /// Model returned no text response
    #[error("Model returned no text response")]
    NoResponse,

    /// Model returned empty response with no content
    #[error("Model returned empty response with no text or tool use")]
    EmptyResponse,

    /// Response exceeded maximum token limit
    #[error("Response exceeded maximum token limit. Try asking the model to be more concise or break the task into smaller steps.")]
    MaxTokensExceeded,

    /// Response was filtered by content moderation
    #[error("Response was filtered by content moderation")]
    ContentFiltered,

    /// Tool execution was denied by user or policy
    #[error("Tool execution denied: {0}")]
    ToolDenied(String),

    /// Tool not found
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Invalid tool input from model
    #[error("Invalid tool input: {0}")]
    InvalidToolInput(String),

    /// Permission request failed
    #[error("Permission request failed: {0}")]
    PermissionFailed(String),

    /// Unexpected stop reason from model
    #[error("Unexpected stop reason: {0}")]
    UnexpectedStopReason(String),

    /// Context file loading error
    #[error("Context error: {0}")]
    Context(#[from] ContextError),
}

/// Errors that can occur during permission operations
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    /// Permission request not found (expired or invalid ID)
    #[error("Permission request not found: {0}")]
    RequestNotFound(String),

    /// Failed to send response on channel (receiver dropped)
    #[error("Failed to send permission response: channel closed")]
    ChannelClosed,

    /// Failed to save grant to store
    #[error("Failed to save grant: {0}")]
    StoreSave(#[from] crate::permission::GrantStoreError),
}

/// Information about a tool for display purposes
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
}

/// Information about the current session
#[cfg(feature = "session")]
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session ID
    pub id: String,
    /// Directory where session is active
    pub directory: String,
    /// Number of messages in session
    pub message_count: usize,
    /// When session was created
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update time
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Default permission timeout (5 minutes)
pub const DEFAULT_PERMISSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Default maximum concurrent tool executions
pub const DEFAULT_MAX_CONCURRENT_TOOLS: usize = 12;

/// Response from Agent.run() containing the result and execution statistics
#[derive(Debug, Clone)]
pub struct AgentResponse {
    /// The final text response from the agent
    pub text: String,
    /// All tool calls made during this run
    pub tool_calls: Vec<ToolCallInfo>,
    /// Total token usage across all model calls (if available)
    pub token_usage: Option<TokenUsageStats>,
    /// Total execution time
    pub duration: Duration,
    /// Number of model calls made (includes retries after tool use)
    pub model_calls: usize,
}

impl AgentResponse {
    /// Get just the text response
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for AgentResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text)
    }
}

impl From<AgentResponse> for String {
    fn from(response: AgentResponse) -> Self {
        response.text
    }
}

impl PartialEq<&str> for AgentResponse {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

/// Information about a tool call made during agent execution
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    /// Tool name
    pub name: String,
    /// Input parameters (as JSON)
    pub input: Value,
    /// Output from the tool
    pub output: String,
    /// Whether the tool succeeded
    pub success: bool,
    /// Execution duration
    pub duration: Duration,
}

/// Cumulative token usage statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsageStats {
    /// Total input tokens across all model calls
    pub input_tokens: usize,
    /// Total output tokens across all model calls
    pub output_tokens: usize,
}

impl TokenUsageStats {
    /// Total tokens (input + output)
    pub fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_stats() {
        let stats = TokenUsageStats {
            input_tokens: 100,
            output_tokens: 50,
        };
        assert_eq!(stats.total(), 150);
    }

    #[test]
    fn test_agent_response() {
        let response = AgentResponse {
            text: "Hello".to_string(),
            tool_calls: vec![],
            token_usage: None,
            duration: Duration::from_secs(1),
            model_calls: 1,
        };
        assert_eq!(response.text(), "Hello");
        assert_eq!(format!("{}", response), "Hello");
        assert!(response == "Hello");
    }
}
