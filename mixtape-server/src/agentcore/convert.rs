//! Conversion from mixtape AgentEvent to AgentCore streaming events.

use mixtape_core::events::AgentEvent;

use super::events::AgentCoreEvent;

/// Context for tracking state across event conversions.
///
/// Maintains whether we're currently inside a text message,
/// allowing proper boundary management.
pub struct ConversionContext {
    /// Whether we're currently streaming text content.
    in_text_stream: bool,
}

impl ConversionContext {
    /// Create a new conversion context.
    pub fn new() -> Self {
        Self {
            in_text_stream: false,
        }
    }

    /// Whether text is currently being streamed.
    pub fn in_text_stream(&self) -> bool {
        self.in_text_stream
    }
}

/// Convert an AgentEvent to AgentCore streaming events.
///
/// Some AgentEvents map to multiple AgentCore events, so this returns a Vec.
/// The context is mutated to track state across events.
pub fn convert_event(event: &AgentEvent, ctx: &mut ConversionContext) -> Vec<AgentCoreEvent> {
    match event {
        // ===== Lifecycle Events =====
        AgentEvent::RunStarted { .. } => {
            vec![AgentCoreEvent::RunStarted]
        }

        AgentEvent::RunCompleted { output, .. } => {
            ctx.in_text_stream = false;
            vec![AgentCoreEvent::RunFinished {
                response: output.clone(),
            }]
        }

        AgentEvent::RunFailed { error, .. } => {
            ctx.in_text_stream = false;
            vec![AgentCoreEvent::RunError {
                message: error.clone(),
            }]
        }

        // ===== Model Streaming =====
        AgentEvent::ModelCallStarted { .. } => {
            ctx.in_text_stream = true;
            vec![]
        }

        AgentEvent::ModelCallStreaming { delta, .. } => {
            if ctx.in_text_stream() {
                vec![AgentCoreEvent::ContentDelta {
                    text: delta.clone(),
                }]
            } else {
                vec![]
            }
        }

        AgentEvent::ModelCallCompleted { .. } => {
            // Don't end text stream here - wait for RunCompleted or tool events
            vec![]
        }

        // ===== Tool Events =====
        AgentEvent::ToolRequested {
            tool_use_id,
            name,
            input,
        } => {
            ctx.in_text_stream = false;

            vec![
                AgentCoreEvent::ToolCallStart {
                    tool_call_id: tool_use_id.clone(),
                    name: name.clone(),
                },
                AgentCoreEvent::ToolCallInput {
                    tool_call_id: tool_use_id.clone(),
                    input: input.clone(),
                },
                AgentCoreEvent::ToolCallEnd {
                    tool_call_id: tool_use_id.clone(),
                },
            ]
        }

        AgentEvent::ToolExecuting { .. } => {
            vec![]
        }

        AgentEvent::ToolCompleted {
            tool_use_id,
            output,
            ..
        } => {
            vec![AgentCoreEvent::ToolCallResult {
                tool_call_id: tool_use_id.clone(),
                content: output.as_text(),
            }]
        }

        AgentEvent::ToolFailed {
            tool_use_id, error, ..
        } => {
            vec![AgentCoreEvent::ToolCallError {
                tool_call_id: tool_use_id.clone(),
                error: error.clone(),
            }]
        }

        // ===== Permission Events =====
        // AgentCore agents run without interactive permissions (all tools trusted),
        // so permission events are not forwarded. If a permission event somehow
        // occurs, it's silently ignored.
        AgentEvent::PermissionRequired { .. }
        | AgentEvent::PermissionGranted { .. }
        | AgentEvent::PermissionDenied { .. } => vec![],

        // ===== Session Events & Future Variants =====
        #[allow(unreachable_patterns)]
        _ => vec![],
    }
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
