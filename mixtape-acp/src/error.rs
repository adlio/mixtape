use thiserror::Error;

/// Errors that can occur in the ACP adapter layer.
#[derive(Debug, Error)]
pub enum AcpError {
    /// An error from the underlying mixtape agent.
    #[error("Agent error: {0}")]
    Agent(#[from] mixtape_core::AgentError),

    /// The requested session was not found.
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// No agent factory was configured on the builder.
    #[error("No agent factory configured")]
    NoAgentFactory,

    /// The agent factory failed to build an agent.
    #[error("Failed to build agent: {0}")]
    BuildFailed(String),

    /// A transport-level error (I/O, JSON-RPC framing, etc).
    #[error("Transport error: {0}")]
    Transport(String),

    /// An internal error (agent task panicked, channel dropped, etc).
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<AcpError> for agent_client_protocol::Error {
    fn from(err: AcpError) -> Self {
        agent_client_protocol::Error::internal_error().data(err.to_string())
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
