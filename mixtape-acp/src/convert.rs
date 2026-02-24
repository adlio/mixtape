use agent_client_protocol::{
    ContentBlock as AcpContentBlock, ContentChunk, SessionUpdate, ToolCall, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use mixtape_core::AgentEvent;
use serde_json::Value;

/// Convert a mixtape `AgentEvent` into an ACP `SessionUpdate`, if applicable.
///
/// Returns `None` for lifecycle events (RunStarted, RunCompleted, etc.) that
/// don't map to ACP session updates — those are handled by the `PromptResponse`
/// return value instead.
pub(crate) fn agent_event_to_session_update(event: &AgentEvent) -> Option<SessionUpdate> {
    match event {
        AgentEvent::ModelCallStreaming { delta, .. } => {
            let content = AcpContentBlock::from(delta.as_str());
            Some(SessionUpdate::AgentMessageChunk(ContentChunk::new(content)))
        }

        AgentEvent::ToolRequested {
            tool_use_id,
            name,
            input,
        } => {
            let tool_call = ToolCall::new(ToolCallId::from(tool_use_id.clone()), name.clone())
                .status(ToolCallStatus::Pending)
                .raw_input(input.clone());
            Some(SessionUpdate::ToolCall(tool_call))
        }

        AgentEvent::ToolExecuting { tool_use_id, .. } => {
            let fields = ToolCallUpdateFields::new().status(ToolCallStatus::InProgress);
            Some(tool_call_update(tool_use_id, fields))
        }

        AgentEvent::ToolCompleted {
            tool_use_id,
            output,
            ..
        } => {
            let fields = ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .raw_output(Value::String(output.as_text()));
            Some(tool_call_update(tool_use_id, fields))
        }

        AgentEvent::ToolFailed {
            tool_use_id, error, ..
        } => {
            let fields = ToolCallUpdateFields::new()
                .status(ToolCallStatus::Failed)
                .raw_output(Value::String(error.clone()));
            Some(tool_call_update(tool_use_id, fields))
        }

        // Lifecycle events don't map to session updates
        AgentEvent::RunStarted { .. }
        | AgentEvent::RunCompleted { .. }
        | AgentEvent::RunFailed { .. }
        | AgentEvent::ModelCallStarted { .. }
        | AgentEvent::ModelCallCompleted { .. }
        | AgentEvent::PermissionRequired { .. }
        | AgentEvent::PermissionGranted { .. }
        | AgentEvent::PermissionDenied { .. } => None,

        // Catch any future variants
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

/// Wrap fields into a `SessionUpdate::ToolCallUpdate`.
fn tool_call_update(tool_use_id: &str, fields: ToolCallUpdateFields) -> SessionUpdate {
    let update = ToolCallUpdate::new(ToolCallId::from(tool_use_id.to_string()), fields);
    SessionUpdate::ToolCallUpdate(update)
}

/// Map an `AgentError` to an ACP `StopReason`.
///
/// Returns `Ok(stop_reason)` for errors that map to a known stop reason,
/// or `Err(acp_error)` for errors that should be reported as protocol errors.
pub(crate) fn agent_error_to_stop_reason(
    error: &mixtape_core::AgentError,
) -> Result<agent_client_protocol::StopReason, agent_client_protocol::Error> {
    match error {
        mixtape_core::AgentError::MaxTokensExceeded => {
            Ok(agent_client_protocol::StopReason::MaxTokens)
        }
        mixtape_core::AgentError::ContentFiltered => Ok(agent_client_protocol::StopReason::Refusal),
        other => Err(agent_client_protocol::Error::internal_error().data(other.to_string())),
    }
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
