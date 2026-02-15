use std::time::{Duration, Instant};

use serde_json::Value;

use crate::permission::Scope;
use crate::tool::ToolResult;
use crate::types::StopReason;

/// Events emitted during agent execution
///
/// These events allow observers to track agent lifecycle, model calls,
/// and tool executions in real-time.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    // ===== Agent Lifecycle =====
    /// Agent.run() started
    RunStarted {
        /// User input message
        input: String,
        /// Timestamp
        timestamp: Instant,
    },

    /// Agent.run() completed
    RunCompleted {
        /// Final response to user
        output: String,
        /// Total execution duration
        duration: Duration,
    },

    /// Agent.run() failed with error
    RunFailed {
        /// Error message
        error: String,
        /// How long before failure
        duration: Duration,
    },

    // ===== Model API Lifecycle =====
    /// Model API call started
    ModelCallStarted {
        /// Messages being sent to model
        message_count: usize,
        /// Number of tools available to model
        tool_count: usize,
        /// Timestamp
        timestamp: Instant,
    },

    /// Model streaming a token (only if streaming enabled)
    ModelCallStreaming {
        /// Incremental text delta
        delta: String,
        /// Accumulated length so far
        accumulated_length: usize,
    },

    /// Model API call completed
    ModelCallCompleted {
        /// Response content
        response_content: String,
        /// Token usage statistics
        tokens: Option<TokenUsage>,
        /// API call duration
        duration: Duration,
        /// Stop reason from model
        stop_reason: Option<StopReason>,
    },

    // ===== Tool Lifecycle =====
    /// Model requested a tool (fires exactly once per tool use)
    ToolRequested {
        /// Unique ID for this tool use
        tool_use_id: String,
        /// Tool name
        name: String,
        /// Input parameters
        input: Value,
    },

    /// Tool execution actually starting (after permission granted)
    ToolExecuting {
        /// Unique ID for this tool use
        tool_use_id: String,
        /// Tool name
        name: String,
    },

    /// Tool execution completed successfully
    ToolCompleted {
        /// Matching ID from ToolRequested
        tool_use_id: String,
        /// Tool name
        name: String,
        /// Tool output
        output: ToolResult,
        /// Execution duration
        duration: Duration,
    },

    /// Tool execution failed
    ToolFailed {
        /// Matching ID from ToolRequested
        tool_use_id: String,
        /// Tool name
        name: String,
        /// Error message
        error: String,
        /// How long before failure
        duration: Duration,
    },

    // ===== Permission Events =====
    /// Tool execution requires permission
    PermissionRequired {
        /// Unique ID for this permission request
        proposal_id: String,
        /// Tool name
        tool_name: String,
        /// Tool input parameters
        params: Value,
        /// Hash of parameters (for creating exact-match grants)
        params_hash: String,
    },

    /// Permission granted (auto-approved or user-approved)
    PermissionGranted {
        /// Tool use ID
        tool_use_id: String,
        /// Tool name
        tool_name: String,
        /// The scope of the grant (None if one-time approval)
        scope: Option<Scope>,
    },

    /// Permission denied
    PermissionDenied {
        /// Tool use ID
        tool_use_id: String,
        /// Tool name
        tool_name: String,
        /// Reason for denial
        reason: String,
    },

    // ===== Session Events =====
    #[cfg(feature = "session")]
    /// Session resumed from storage
    SessionResumed {
        /// Session ID
        session_id: String,
        /// Number of prior messages in session
        message_count: usize,
        /// When session was created
        created_at: chrono::DateTime<chrono::Utc>,
    },

    #[cfg(feature = "session")]
    /// Session saved to storage
    SessionSaved {
        /// Session ID
        session_id: String,
        /// Total messages in session now
        message_count: usize,
    },
}

/// Token usage statistics from model
#[derive(Debug, Clone, Copy)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

impl TokenUsage {
    pub fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
}

/// Hook for observing agent events
///
/// Implement this trait to receive notifications about agent execution.
///
/// # Example
/// ```
/// use mixtape_core::events::{AgentEvent, AgentHook};
///
/// struct Logger;
///
/// impl AgentHook for Logger {
///     fn on_event(&self, event: &AgentEvent) {
///         match event {
///             AgentEvent::RunStarted { input, .. } => {
///                 println!("Starting: {}", input);
///             }
///             AgentEvent::ToolRequested { name, .. } => {
///                 println!("Tool requested: {}", name);
///             }
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait AgentHook: Send + Sync {
    /// Called when an event occurs
    fn on_event(&self, event: &AgentEvent);
}

/// Blanket implementation for closures
impl<F> AgentHook for F
where
    F: Fn(&AgentEvent) + Send + Sync,
{
    fn on_event(&self, event: &AgentEvent) {
        self(event)
    }
}

/// Unique identifier for a registered hook.
///
/// Used to remove hooks via [`crate::Agent::remove_hook`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookId(pub(crate) u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_total() {
        let cases = [
            (100, 50, 150),
            (0, 0, 0),
            (1000, 2000, 3000),
            (1, 0, 1),
            (0, 1, 1),
            (usize::MAX / 2, usize::MAX / 2, usize::MAX - 1),
        ];

        for (input, output, expected) in cases {
            let usage = TokenUsage {
                input_tokens: input,
                output_tokens: output,
            };
            assert_eq!(
                usage.total(),
                expected,
                "Failed for input={}, output={}",
                input,
                output
            );
        }
    }
}
