//! Streaming model responses

use futures::StreamExt;

use crate::events::{AgentEvent, TokenUsage};
use crate::model::ModelResponse;
use crate::provider::{ProviderError, StreamEvent};
use crate::types::{ContentBlock, Message, Role, ToolDefinition, ToolUseBlock};

use super::types::AgentError;
use super::Agent;

impl Agent {
    /// Call the model with streaming, emitting events for each text delta.
    pub(super) async fn generate_with_streaming(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<ModelResponse, AgentError> {
        let mut stream = self
            .provider
            .generate_stream(messages, tools, system_prompt)
            .await?;

        let mut text_content = String::new();
        let mut tool_uses: Vec<ToolUseBlock> = Vec::new();
        let mut replay_content = Vec::new();
        let mut stop_reason = None;
        let mut usage: Option<TokenUsage> = None;

        while let Some(event_result) = stream.next().await {
            match event_result? {
                StreamEvent::TextDelta(delta) => {
                    text_content.push_str(&delta);
                    self.emit_event(AgentEvent::ModelCallStreaming {
                        delta,
                        accumulated_length: text_content.len(),
                    });
                }
                StreamEvent::ToolUse(tool_use) => tool_uses.push(tool_use),
                StreamEvent::ThinkingDelta(_) => {
                    // Display deltas cannot carry the complete reasoning signature.
                    // Only complete blocks are used for conversation replay.
                }
                StreamEvent::ContentBlock(block) => replay_content.push(block),
                StreamEvent::Stop {
                    stop_reason: reason,
                    usage: reported_usage,
                } => {
                    stop_reason = Some(reason);
                    usage = reported_usage;
                }
            }
        }

        let stop_reason = stop_reason.ok_or_else(|| {
            ProviderError::Model("Model stream ended without a stop event".into())
        })?;

        // Compatibility with providers that only emit the original delta events.
        // Complete blocks, when supplied, are authoritative: never duplicate text
        // or rearrange signed reasoning relative to its tool calls.
        if replay_content.is_empty() {
            if !text_content.is_empty() {
                replay_content.push(ContentBlock::Text(text_content));
            }
            replay_content.extend(tool_uses.into_iter().map(ContentBlock::ToolUse));
        }
        if replay_content.is_empty() {
            return Err(AgentError::EmptyResponse);
        }

        Ok(ModelResponse {
            message: Message {
                role: Role::Assistant,
                content: replay_content,
            },
            stop_reason,
            usage,
        })
    }
}
