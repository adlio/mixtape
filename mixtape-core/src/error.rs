//! Top-level error types for mixtape
//!
//! This module provides a simplified, user-facing error type that flattens
//! the internal error hierarchy into actionable categories.

use thiserror::Error;

use crate::agent::AgentError;
use crate::provider::ProviderError;
use crate::tool::ToolError;

#[cfg(feature = "session")]
use crate::session::SessionError;

/// Top-level error type for mixtape operations
///
/// This enum provides a flattened view of errors, categorized by how users
/// typically need to handle them:
///
/// - [`Error::Auth`] - Fix credentials and retry
/// - [`Error::RateLimited`] - Back off and retry
/// - [`Error::Network`] - Check connectivity, retry
/// - [`Error::Unavailable`] - Service is down, wait and retry
/// - [`Error::Model`] - Model-side issues (content filtered, context too long)
/// - [`Error::Tool`] - Tool execution failed
/// - [`Error::Config`] - Fix configuration (bad model ID, missing parameters)
#[derive(Debug, Error)]
pub enum Error {
    /// Authentication failed (invalid or expired credentials)
    #[error("authentication failed: {0}")]
    Auth(String),

    /// Rate limited - slow down requests
    #[error("rate limited: {0}")]
    RateLimited(String),

    /// Network connectivity issue
    #[error("network error: {0}")]
    Network(String),

    /// Service temporarily unavailable
    #[error("service unavailable: {0}")]
    Unavailable(String),

    /// Model error (content filtered, context too long, empty response, etc.)
    #[error("model error: {0}")]
    Model(String),

    /// Tool execution failed
    #[error("tool error: {0}")]
    Tool(String),

    /// Configuration error (bad model ID, missing parameters)
    #[error("configuration error: {0}")]
    Config(String),

    /// Session storage error
    #[cfg(feature = "session")]
    #[error("session error: {0}")]
    Session(String),

    /// MCP server error
    #[cfg(feature = "mcp")]
    #[error("MCP error: {0}")]
    Mcp(String),

    /// Other error
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Returns true if this is an authentication error
    pub fn is_auth(&self) -> bool {
        matches!(self, Self::Auth(_))
    }

    /// Returns true if this is a rate limiting error
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited(_))
    }

    /// Returns true if this is a network error
    pub fn is_network(&self) -> bool {
        matches!(self, Self::Network(_))
    }

    /// Returns true if the service is unavailable
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }

    /// Returns true if this is a model error
    pub fn is_model(&self) -> bool {
        matches!(self, Self::Model(_))
    }

    /// Returns true if this is a tool error
    pub fn is_tool(&self) -> bool {
        matches!(self, Self::Tool(_))
    }

    /// Returns true if this is a configuration error
    pub fn is_config(&self) -> bool {
        matches!(self, Self::Config(_))
    }

    /// Returns true if this error is potentially retryable
    ///
    /// Retryable errors include rate limiting, network issues, and service
    /// unavailability. Authentication and configuration errors are not
    /// retryable without user intervention.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited(_) | Self::Network(_) | Self::Unavailable(_)
        )
    }
}

impl From<ProviderError> for Error {
    fn from(err: ProviderError) -> Self {
        match err {
            ProviderError::Authentication(msg) => Self::Auth(msg),
            ProviderError::RateLimited(msg) => Self::RateLimited(msg),
            ProviderError::Network(msg) => Self::Network(msg),
            ProviderError::ServiceUnavailable(msg) => Self::Unavailable(msg),
            ProviderError::Model(msg) => Self::Model(msg),
            ProviderError::Configuration(msg) => Self::Config(msg),
            ProviderError::Communication(err) => Self::Network(err.to_string()),
            ProviderError::Other(msg) => Self::Other(msg),
        }
    }
}

impl From<ToolError> for Error {
    fn from(err: ToolError) -> Self {
        Self::Tool(err.to_string())
    }
}

#[cfg(feature = "session")]
impl From<SessionError> for Error {
    fn from(err: SessionError) -> Self {
        Self::Session(err.to_string())
    }
}

impl From<AgentError> for Error {
    fn from(err: AgentError) -> Self {
        match err {
            AgentError::Provider(e) => e.into(),
            AgentError::Tool(e) => e.into(),
            #[cfg(feature = "session")]
            AgentError::Session(e) => e.into(),
            AgentError::NoResponse => Self::Model("model returned no response".to_string()),
            AgentError::EmptyResponse => Self::Model("model returned empty response".to_string()),
            AgentError::MaxTokensExceeded => Self::Model(
                "response exceeded maximum token limit - try asking the model to be more concise"
                    .to_string(),
            ),
            AgentError::ContentFiltered => {
                Self::Model("response was filtered by content moderation".to_string())
            }
            AgentError::ToolDenied(msg) => Self::Tool(format!("denied: {}", msg)),
            AgentError::ToolNotFound(name) => Self::Tool(format!("not found: {}", name)),
            AgentError::InvalidToolInput(msg) => Self::Tool(format!("invalid input: {}", msg)),
            AgentError::PermissionFailed(msg) => Self::Tool(format!("permission failed: {}", msg)),
            AgentError::UnexpectedStopReason(reason) => {
                Self::Model(format!("unexpected stop reason: {}", reason))
            }
            AgentError::Context(e) => Self::Model(format!("context error: {}", e)),
        }
    }
}

/// Result type for mixtape operations
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retryable() {
        assert!(Error::RateLimited("slow down".into()).is_retryable());
        assert!(Error::Network("connection refused".into()).is_retryable());
        assert!(Error::Unavailable("503".into()).is_retryable());

        assert!(!Error::Auth("invalid token".into()).is_retryable());
        assert!(!Error::Config("bad model id".into()).is_retryable());
        assert!(!Error::Model("content filtered".into()).is_retryable());
    }

    #[test]
    fn test_from_provider_error() {
        let err: Error = ProviderError::Authentication("expired".into()).into();
        assert!(err.is_auth());

        let err: Error = ProviderError::RateLimited("throttled".into()).into();
        assert!(err.is_rate_limited());

        let err: Error = ProviderError::Network("timeout".into()).into();
        assert!(err.is_network());
    }

    #[test]
    fn test_from_agent_error() {
        let err: Error = AgentError::MaxTokensExceeded.into();
        assert!(err.is_model());

        let err: Error = AgentError::ToolNotFound("calculator".into()).into();
        assert!(err.is_tool());

        let err: Error = AgentError::Provider(ProviderError::RateLimited("slow".into())).into();
        assert!(err.is_rate_limited());
    }

    #[test]
    fn test_convenience_methods() {
        assert!(Error::Auth("x".into()).is_auth());
        assert!(Error::RateLimited("x".into()).is_rate_limited());
        assert!(Error::Network("x".into()).is_network());
        assert!(Error::Unavailable("x".into()).is_unavailable());
        assert!(Error::Model("x".into()).is_model());
        assert!(Error::Tool("x".into()).is_tool());
        assert!(Error::Config("x".into()).is_config());
    }
}
