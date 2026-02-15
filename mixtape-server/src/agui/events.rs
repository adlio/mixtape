//! AG-UI protocol event types.
//!
//! These types represent the ~17 standard AG-UI event types used for
//! agent-to-frontend communication.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// AG-UI protocol events.
///
/// Events are serialized with a `type` field in SCREAMING_SNAKE_CASE
/// as per the AG-UI specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AguiEvent {
    // ===== Lifecycle Events =====
    /// Agent run started.
    RunStarted {
        /// Thread ID for conversation continuity.
        thread_id: String,
        /// Unique run ID for this execution.
        run_id: String,
    },

    /// Agent run finished successfully.
    RunFinished {
        /// Thread ID for conversation continuity.
        thread_id: String,
        /// Unique run ID for this execution.
        run_id: String,
    },

    /// Agent run failed with an error.
    RunError {
        /// Error message describing the failure.
        message: String,
        /// Optional error code.
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },

    // ===== Text Message Events =====
    /// Start of a new text message.
    TextMessageStart {
        /// Unique message ID.
        message_id: String,
        /// Role of the message author.
        role: MessageRole,
    },

    /// Incremental content for a text message.
    TextMessageContent {
        /// Message ID this content belongs to.
        message_id: String,
        /// Text delta to append.
        delta: String,
    },

    /// End of a text message.
    TextMessageEnd {
        /// Message ID that is complete.
        message_id: String,
    },

    // ===== Tool Call Events =====
    /// Start of a tool call.
    ToolCallStart {
        /// Unique tool call ID.
        tool_call_id: String,
        /// Name of the tool being called.
        tool_call_name: String,
        /// Optional parent message ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    },

    /// Incremental arguments for a tool call.
    ToolCallArgs {
        /// Tool call ID this belongs to.
        tool_call_id: String,
        /// JSON argument delta.
        delta: String,
    },

    /// End of tool call arguments.
    ToolCallEnd {
        /// Tool call ID that is complete.
        tool_call_id: String,
    },

    /// Result from a tool call.
    ToolCallResult {
        /// Unique message ID for this result.
        message_id: String,
        /// Tool call ID this result is for.
        tool_call_id: String,
        /// Result content (text or JSON string).
        content: String,
        /// Role (typically Tool).
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<MessageRole>,
    },

    // ===== State Management Events =====
    /// Complete state snapshot.
    StateSnapshot {
        /// The complete state object.
        snapshot: Value,
    },

    /// Incremental state update (JSON Patch).
    StateDelta {
        /// JSON Patch operations (RFC 6902).
        delta: Vec<JsonPatchOp>,
    },

    // ===== Interrupt Events (Human-in-the-Loop) =====
    /// Interrupt requiring user action.
    ///
    /// Used for permission requests and other human-in-the-loop interactions.
    Interrupt {
        /// Unique interrupt ID.
        interrupt_id: String,
        /// Type of interrupt.
        interrupt_type: InterruptType,
        /// Data associated with the interrupt.
        data: InterruptData,
    },
}

/// Message author role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// User message.
    User,
    /// Assistant message.
    Assistant,
    /// System message.
    System,
    /// Tool result message.
    Tool,
}

/// Type of interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptType {
    /// Tool requires user approval before execution.
    ToolApproval,
}

/// Data associated with an interrupt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptData {
    /// Tool use ID / proposal ID.
    pub tool_use_id: String,
    /// Name of the tool requiring approval.
    pub tool_name: String,
    /// Tool input parameters.
    pub params: Value,
    /// Hash of parameters for exact-match grants.
    pub params_hash: String,
}

/// Response to an interrupt from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InterruptResponse {
    /// Approve this single call without saving a grant.
    ApproveOnce,
    /// Trust this tool entirely and save a grant.
    TrustTool {
        /// Scope for the grant.
        scope: GrantScope,
    },
    /// Trust this exact call (matching parameters) and save a grant.
    TrustExact {
        /// Scope for the grant.
        scope: GrantScope,
    },
    /// Deny the request.
    Deny {
        /// Optional reason for denial.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// Scope for permission grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantScope {
    /// Grant lives in memory only, cleared when process exits.
    Session,
    /// Grant persists to storage.
    Persistent,
}

/// JSON Patch operation (RFC 6902).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonPatchOp {
    /// Operation type (add, remove, replace, move, copy, test).
    pub op: String,
    /// JSON Pointer path.
    pub path: String,
    /// Value for add/replace operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
