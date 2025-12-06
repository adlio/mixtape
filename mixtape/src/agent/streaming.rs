//! Streaming model responses

use futures::StreamExt;

use crate::events::{AgentEvent, TokenUsage};
use crate::model::ModelResponse;
use crate::provider::StreamEvent;
use crate::types::{ContentBlock, Message, Role, StopReason, ToolDefinition, ToolUseBlock};

use super::types::AgentError;
use super::Agent;

impl Agent {
    /// Call the model with streaming, emitting events for each text delta
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
        let mut stop_reason = StopReason::EndTurn;
        let mut usage: Option<TokenUsage> = None;

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => match event {
                    StreamEvent::TextDelta(delta) => {
                        text_content.push_str(&delta);
                        self.emit_event(AgentEvent::ModelCallStreaming {
                            delta,
                            accumulated_length: text_content.len(),
                        });
                    }
                    StreamEvent::ToolUse(tool_use) => {
                        tool_uses.push(tool_use);
                    }
                    StreamEvent::ThinkingDelta(_thinking) => {
                        // Extended thinking delta - we don't expose thinking content to events yet
                        // but it's processed through the stream
                    }
                    StreamEvent::Stop {
                        stop_reason: reason,
                        usage: u,
                    } => {
                        stop_reason = reason;
                        usage = u;
                    }
                },
                Err(e) => {
                    return Err(AgentError::Provider(e));
                }
            }
        }

        // Build the response message
        let mut content = Vec::new();
        if !text_content.is_empty() {
            content.push(ContentBlock::Text(text_content));
        }
        for tool_use in tool_uses {
            content.push(ContentBlock::ToolUse(tool_use));
        }

        // Safety: AWS Bedrock requires at least one content block
        // This should not happen with proper streaming, but guard against it
        if content.is_empty() {
            return Err(AgentError::EmptyResponse);
        }

        Ok(ModelResponse {
            message: Message {
                role: Role::Assistant,
                content,
            },
            stop_reason,
            usage,
        })
    }
}
