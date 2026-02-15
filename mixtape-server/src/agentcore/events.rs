//! AgentCore streaming event types.
//!
//! These events are streamed via SSE from the `/invocations` endpoint to
//! AgentCore callers. They provide real-time visibility into agent execution
//! including text streaming, tool calls, and lifecycle events.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events streamed from the agent during execution.
///
/// Serialized as JSON with a `type` field in `snake_case`.
///
/// # Event Flow
///
/// A typical agent execution produces events in this order:
///
/// ```text
/// run_started
///   content_delta { text: "Let me " }
///   content_delta { text: "look that up." }
///   tool_call_start { name: "search", ... }
///   tool_call_input { input: {...} }
///   tool_call_end
///   tool_call_result { content: "..." }
///   content_delta { text: "Based on " }
///   content_delta { text: "the results..." }
/// run_finished { response: "Based on the results..." }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentCoreEvent {
    // ===== Lifecycle Events =====
    /// Agent execution started.
    RunStarted,

    /// Agent execution finished successfully.
    RunFinished {
        /// The complete response text.
        response: String,
    },

    /// Agent execution failed with an error.
    RunError {
        /// Error message describing the failure.
        message: String,
    },

    // ===== Text Streaming Events =====
    /// Incremental text content from the agent.
    ContentDelta {
        /// Text chunk to append.
        text: String,
    },

    // ===== Tool Call Events =====
    /// A tool call has been requested by the model.
    ToolCallStart {
        /// Unique ID for this tool call.
        tool_call_id: String,
        /// Name of the tool being called.
        name: String,
    },

    /// Input arguments for a tool call.
    ToolCallInput {
        /// Tool call ID this belongs to.
        tool_call_id: String,
        /// The complete input arguments as JSON.
        input: Value,
    },

    /// Tool call arguments are complete.
    ToolCallEnd {
        /// Tool call ID that is complete.
        tool_call_id: String,
    },

    /// Successful result from a tool call.
    ToolCallResult {
        /// Tool call ID this result is for.
        tool_call_id: String,
        /// Result content (text or JSON string).
        content: String,
    },

    /// A tool call failed with an error.
    ToolCallError {
        /// Tool call ID that failed.
        tool_call_id: String,
        /// Error message.
        error: String,
    },
}

/// Request body for the `/invocations` endpoint.
///
/// AgentCore forwards the `InvokeAgentRuntime` payload as-is to this endpoint.
/// This type defines the expected JSON structure for mixtape agents.
#[derive(Debug, Deserialize)]
pub struct InvocationRequest {
    /// The user message / prompt to send to the agent.
    pub prompt: String,
}

/// Response body for the `/ping` health check endpoint.
#[derive(Debug, Serialize)]
pub struct PingResponse {
    /// Health status. `"Healthy"` when ready, `"HealthyBusy"` when processing.
    pub status: String,
    /// Unix timestamp of the last health update.
    pub time_of_last_update: i64,
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
