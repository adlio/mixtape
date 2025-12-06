//! Session management for Agent
//!
//! This module provides session persistence and message conversion.
//! Only available when the `session` feature is enabled.

use crate::session::{MessageRole, SessionError, SessionMessage};
use crate::tool::ToolResult;
use crate::types::{ContentBlock, Message, Role, ToolResultBlock, ToolResultStatus, ToolUseBlock};
use serde_json::Value;

use super::types::SessionInfo;
use super::Agent;

// =============================================================================
// Agent methods
// =============================================================================

impl Agent {
    /// Get current session information
    pub async fn get_session_info(&self) -> Result<Option<SessionInfo>, SessionError> {
        if let Some(store) = &self.session_store {
            let session = store.get_or_create_session().await?;

            Ok(Some(SessionInfo {
                id: session.id,
                directory: session.directory,
                message_count: session.messages.len(),
                created_at: session.created_at,
                updated_at: session.updated_at,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get session history (last n messages)
    pub async fn get_session_history(
        &self,
        limit: usize,
    ) -> Result<Vec<SessionMessage>, SessionError> {
        if let Some(store) = &self.session_store {
            let session = store.get_or_create_session().await?;

            let start = session.messages.len().saturating_sub(limit);
            Ok(session.messages[start..].to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    /// Clear the current session (delete stored history for this directory).
    ///
    /// This is idempotent: if no session store is configured, it succeeds silently.
    pub async fn clear_session(&self) -> Result<(), SessionError> {
        if let Some(store) = &self.session_store {
            let session = store.get_or_create_session().await?;
            store.delete_session(&session.id).await?;
        }
        Ok(())
    }
}

// =============================================================================
// Session message conversion
// =============================================================================

/// Convert a session message to one or more mixtape messages.
///
/// This may return multiple messages because the format requires:
/// - Tool use blocks (tool_calls) in assistant messages only
/// - Tool result blocks (tool_results) in user messages only
/// - These cannot be mixed in the same message
///
/// So if a session message has both tool_calls and tool_results,
/// we split it into two messages.
pub(super) fn convert_session_message_to_mixtape(
    msg: &SessionMessage,
) -> Result<Vec<Message>, SessionError> {
    let mut messages = Vec::new();

    let has_tool_calls = !msg.tool_calls.is_empty();
    let has_tool_results = !msg.tool_results.is_empty();
    let is_assistant = matches!(msg.role, MessageRole::Assistant);

    // For assistant messages with tool calls, create an assistant message with tool_use blocks
    if is_assistant && has_tool_calls {
        let mut content_blocks = Vec::new();

        for tool_call in &msg.tool_calls {
            let input: Value = serde_json::from_str(&tool_call.input)?;

            content_blocks.push(ContentBlock::ToolUse(ToolUseBlock {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                input,
            }));
        }

        messages.push(Message {
            role: Role::Assistant,
            content: content_blocks,
        });
    }

    // For tool results, create a user message with tool_result blocks
    if has_tool_results {
        let mut result_blocks = Vec::new();

        for tool_result in &msg.tool_results {
            let status = if tool_result.success {
                ToolResultStatus::Success
            } else {
                ToolResultStatus::Error
            };

            result_blocks.push(ToolResultBlock {
                tool_use_id: tool_result.tool_use_id.clone(),
                content: ToolResult::Text(tool_result.content.clone()),
                status,
            });
        }

        messages.push(Message::tool_results(result_blocks));
    }

    // For assistant messages with final text (no tool calls), or user messages
    if !has_tool_calls && !has_tool_results && !msg.content.is_empty() {
        let role = match msg.role {
            MessageRole::User => Role::User,
            MessageRole::Assistant => Role::Assistant,
            MessageRole::System => Role::User,
        };

        messages.push(Message {
            role,
            content: vec![ContentBlock::Text(msg.content.clone())],
        });
    }

    // Handle case where assistant has both tool calls AND final text response
    // (tool calls message already added above, now add text response)
    if is_assistant && has_tool_calls && !msg.content.is_empty() {
        messages.push(Message::assistant(&msg.content));
    }

    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ToolCall, ToolResult};
    use chrono::Utc;

    #[test]
    fn test_convert_user_message() {
        let msg = SessionMessage {
            role: MessageRole::User,
            content: "Hello".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        };

        let messages = convert_session_message_to_mixtape(&msg).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].role, Role::User));
        assert_eq!(messages[0].text(), "Hello");
    }

    #[test]
    fn test_convert_assistant_with_tool_call() {
        let msg = SessionMessage {
            role: MessageRole::Assistant,
            content: "".to_string(),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "search".to_string(),
                input: r#"{"query": "test"}"#.to_string(),
            }],
            tool_results: vec![ToolResult {
                tool_use_id: "call_1".to_string(),
                success: true,
                content: "Found it".to_string(),
            }],
            timestamp: Utc::now(),
        };

        let messages = convert_session_message_to_mixtape(&msg).unwrap();
        // Should have assistant message with tool use + user message with tool result
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, Role::Assistant));
        assert!(matches!(messages[1].role, Role::User));
    }

    #[test]
    fn test_convert_assistant_text_only() {
        let msg = SessionMessage {
            role: MessageRole::Assistant,
            content: "Here's my response".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        };

        let messages = convert_session_message_to_mixtape(&msg).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0].role, Role::Assistant));
        assert_eq!(messages[0].text(), "Here's my response");
    }
}
