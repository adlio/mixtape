use std::time::{Duration, Instant};

use serde_json::Value;

use crate::permission::Scope;
use crate::tool::ToolResult;
use crate::types::StopReason;

/// Status indicating how a tool execution was approved
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalStatus {
    /// Tool was automatically approved (in registry or AutoApproveAll mode)
    AutoApproved,
    /// Tool was explicitly approved by user
    UserApproved,
    /// Approval system not configured for this agent
    NotRequired,
}

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
    /// Tool execution started
    ToolStarted {
        /// Unique ID for this tool execution
        id: String,
        /// Tool name
        name: String,
        /// Input parameters
        input: Value,
        /// How this tool execution was approved
        approval_status: ToolApprovalStatus,
        /// Timestamp
        timestamp: Instant,
    },

    /// Tool execution completed successfully
    ToolCompleted {
        /// Matching ID from ToolStarted
        id: String,
        /// Tool name
        name: String,
        /// Tool output
        output: ToolResult,
        /// How this tool execution was approved
        approval_status: ToolApprovalStatus,
        /// Execution duration
        duration: Duration,
    },

    /// Tool execution failed
    ToolFailed {
        /// Matching ID from ToolStarted
        id: String,
        /// Tool name
        name: String,
        /// Error message
        error: String,
        /// How long it ran before failing
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

    /// Permission granted
    PermissionGranted {
        /// Matching proposal ID
        proposal_id: String,
        /// The scope of the grant (None if one-time approval)
        scope: Option<Scope>,
    },

    /// Permission denied
    PermissionDenied {
        /// Matching proposal ID
        proposal_id: String,
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
///             AgentEvent::ToolStarted { name, .. } => {
///                 println!("Running tool: {}", name);
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

    #[test]
    fn test_tool_approval_status_variants() {
        // Just verify all variants exist and are distinct
        let auto = ToolApprovalStatus::AutoApproved;
        let user = ToolApprovalStatus::UserApproved;
        let not_required = ToolApprovalStatus::NotRequired;

        assert_ne!(auto, user);
        assert_ne!(auto, not_required);
        assert_ne!(user, not_required);
    }
}
