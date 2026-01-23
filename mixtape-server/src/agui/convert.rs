//! Conversion from mixtape AgentEvent to AG-UI events.

use mixtape_core::events::AgentEvent;

use super::events::{AguiEvent, InterruptData, InterruptType, MessageRole};

/// Context for converting AgentEvent to AG-UI events.
///
/// Maintains state across events to properly track message boundaries
/// and generate consistent IDs.
pub struct ConversionContext {
    /// Thread ID for conversation continuity.
    pub thread_id: String,
    /// Run ID for this execution.
    pub run_id: String,
    /// Current message ID being built (for streaming).
    current_message_id: Option<String>,
}

impl ConversionContext {
    /// Create a new conversion context.
    pub fn new(thread_id: String, run_id: String) -> Self {
        Self {
            thread_id,
            run_id,
            current_message_id: None,
        }
    }

    /// Get the current message ID, if any.
    pub fn current_message_id(&self) -> Option<&str> {
        self.current_message_id.as_deref()
    }

    /// Set the current message ID.
    pub fn set_current_message_id(&mut self, id: String) {
        self.current_message_id = Some(id);
    }

    /// Clear and return the current message ID.
    pub fn take_current_message_id(&mut self) -> Option<String> {
        self.current_message_id.take()
    }
}

/// Convert an AgentEvent to AG-UI events.
///
/// Some AgentEvents map to multiple AG-UI events, so this returns a Vec.
/// The context is mutated to track state across events.
pub fn convert_event(event: &AgentEvent, ctx: &mut ConversionContext) -> Vec<AguiEvent> {
    match event {
        // ===== Lifecycle Events =====
        AgentEvent::RunStarted { .. } => {
            vec![AguiEvent::RunStarted {
                thread_id: ctx.thread_id.clone(),
                run_id: ctx.run_id.clone(),
            }]
        }

        AgentEvent::RunCompleted { .. } => {
            let mut events = Vec::new();

            // End any current message
            if let Some(msg_id) = ctx.take_current_message_id() {
                events.push(AguiEvent::TextMessageEnd { message_id: msg_id });
            }

            events.push(AguiEvent::RunFinished {
                thread_id: ctx.thread_id.clone(),
                run_id: ctx.run_id.clone(),
            });

            events
        }

        AgentEvent::RunFailed { error, .. } => {
            vec![AguiEvent::RunError {
                message: error.clone(),
                code: None,
            }]
        }

        // ===== Model Streaming (Text Messages) =====
        AgentEvent::ModelCallStarted { .. } => {
            // Start a new assistant message
            let message_id = uuid::Uuid::new_v4().to_string();
            ctx.set_current_message_id(message_id.clone());

            vec![AguiEvent::TextMessageStart {
                message_id,
                role: MessageRole::Assistant,
            }]
        }

        AgentEvent::ModelCallStreaming { delta, .. } => {
            if let Some(message_id) = ctx.current_message_id() {
                vec![AguiEvent::TextMessageContent {
                    message_id: message_id.to_string(),
                    delta: delta.clone(),
                }]
            } else {
                vec![]
            }
        }

        AgentEvent::ModelCallCompleted { .. } => {
            // Don't end the message here - wait for RunCompleted or next ModelCallStarted
            // This handles the case where the model continues after tool use
            vec![]
        }

        // ===== Tool Events =====
        AgentEvent::ToolRequested {
            tool_use_id,
            name,
            input,
        } => {
            // End current message before tool call
            let mut events = Vec::new();
            if let Some(msg_id) = ctx.take_current_message_id() {
                events.push(AguiEvent::TextMessageEnd { message_id: msg_id });
            }

            // Tool call events
            events.push(AguiEvent::ToolCallStart {
                tool_call_id: tool_use_id.clone(),
                tool_call_name: name.clone(),
                parent_message_id: None,
            });
            events.push(AguiEvent::ToolCallArgs {
                tool_call_id: tool_use_id.clone(),
                delta: serde_json::to_string(input).unwrap_or_default(),
            });
            events.push(AguiEvent::ToolCallEnd {
                tool_call_id: tool_use_id.clone(),
            });

            events
        }

        AgentEvent::ToolExecuting { .. } => {
            // No AG-UI equivalent - tool execution status is implicit
            vec![]
        }

        AgentEvent::ToolCompleted {
            tool_use_id,
            output,
            ..
        } => {
            vec![AguiEvent::ToolCallResult {
                message_id: uuid::Uuid::new_v4().to_string(),
                tool_call_id: tool_use_id.clone(),
                content: output.as_text(),
                role: Some(MessageRole::Tool),
            }]
        }

        AgentEvent::ToolFailed {
            tool_use_id, error, ..
        } => {
            vec![AguiEvent::ToolCallResult {
                message_id: uuid::Uuid::new_v4().to_string(),
                tool_call_id: tool_use_id.clone(),
                content: format!("Error: {}", error),
                role: Some(MessageRole::Tool),
            }]
        }

        // ===== Permission Events =====
        AgentEvent::PermissionRequired {
            proposal_id,
            tool_name,
            params,
            params_hash,
        } => {
            vec![AguiEvent::Interrupt {
                interrupt_id: proposal_id.clone(),
                interrupt_type: InterruptType::ToolApproval,
                data: InterruptData {
                    tool_use_id: proposal_id.clone(),
                    tool_name: tool_name.clone(),
                    params: params.clone(),
                    params_hash: params_hash.clone(),
                },
            }]
        }

        AgentEvent::PermissionGranted { .. } => {
            // Silent - the tool will execute and emit ToolCompleted
            vec![]
        }

        AgentEvent::PermissionDenied { .. } => {
            // Silent - covered by subsequent ToolFailed event
            vec![]
        }

        // ===== Session Events =====
        // These are feature-gated in mixtape-core, but we handle them here
        // regardless since the enum variant exists when session is enabled
        #[allow(unreachable_patterns)]
        _ => vec![],
    }
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
