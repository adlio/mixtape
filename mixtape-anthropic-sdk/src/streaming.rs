//! Streaming support for the Anthropic API
//!
//! Handles Server-Sent Events (SSE) parsing for streaming responses.
//!
//! # Example: Collecting text from a stream
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!     .user("Hello!")
//!     .build();
//!
//! let stream = client.messages().stream(params).await?;
//! let text = stream.collect_text().await?;
//! println!("{}", text);
//! # Ok(())
//! # }
//! ```

use crate::error::{AnthropicError, ApiError};
use crate::messages::{ContentBlock, Message, MessageCreateParams, StopReason, Usage};
use futures::stream::Stream;
use futures::StreamExt;
use reqwest::header::HeaderMap;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use serde::Deserialize;
use std::pin::Pin;
use std::task::{Context, Poll};

// ============================================================================
// Streaming Event Types
// ============================================================================

/// Server-sent event from the streaming API
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageStreamEvent {
    /// Start of the message
    MessageStart { message: Message },

    /// Start of a content block
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },

    /// Delta update to a content block
    ContentBlockDelta {
        index: usize,
        delta: ContentBlockDelta,
    },

    /// End of a content block
    ContentBlockStop { index: usize },

    /// Delta update to the message (e.g., stop_reason)
    MessageDelta {
        delta: MessageDeltaData,
        usage: Option<DeltaUsage>,
    },

    /// End of the message
    MessageStop,

    /// Ping event (keepalive)
    Ping,

    /// Error event
    Error { error: ApiError },
}

/// Delta update for a content block
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockDelta {
    /// Text delta
    TextDelta { text: String },

    /// Partial JSON for tool input
    InputJsonDelta { partial_json: String },

    /// Thinking delta
    ThinkingDelta { thinking: String },

    /// Signature delta (for thinking blocks)
    SignatureDelta { signature: String },
}

/// Delta update for the message
#[derive(Debug, Clone, Deserialize)]
pub struct MessageDeltaData {
    /// Stop reason (set when generation completes)
    pub stop_reason: Option<StopReason>,

    /// Stop sequence that triggered completion
    pub stop_sequence: Option<String>,
}

/// Usage info in delta events
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaUsage {
    pub output_tokens: u32,
}

// ============================================================================
// MessageStream Implementation
// ============================================================================

/// A stream of message events from the Anthropic API
pub struct MessageStream {
    inner: EventSource,
}

impl MessageStream {
    /// Create a new message stream
    pub(crate) async fn new(
        client: &reqwest::Client,
        url: &str,
        headers: HeaderMap,
        params: MessageCreateParams,
    ) -> Result<Self, AnthropicError> {
        let request = client.post(url).headers(headers).json(&params);

        let event_source = request
            .eventsource()
            .map_err(|e| AnthropicError::Stream(format!("Failed to create event source: {}", e)))?;

        Ok(Self {
            inner: event_source,
        })
    }

    /// Collect all text content from the stream into a single String
    ///
    /// This is a convenience method that consumes the stream and concatenates
    /// all text deltas into a single string. It ignores non-text content blocks
    /// like tool use, thinking, etc.
    ///
    /// # Example
    ///
    /// ```no_run
    /// // Requires ANTHROPIC_API_KEY environment variable
    /// # use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Anthropic::from_env()?;
    /// # let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    /// #     .user("Hello!")
    /// #     .build();
    /// let stream = client.messages().stream(params).await?;
    /// let text = stream.collect_text().await?;
    /// println!("Response: {}", text);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn collect_text(mut self) -> Result<String, AnthropicError> {
        let mut text = String::new();

        while let Some(event) = self.next().await {
            match event? {
                MessageStreamEvent::ContentBlockDelta {
                    delta: ContentBlockDelta::TextDelta { text: chunk },
                    ..
                } => {
                    text.push_str(&chunk);
                }
                MessageStreamEvent::MessageStop => break,
                MessageStreamEvent::Error { error } => {
                    return Err(AnthropicError::Stream(format!(
                        "Stream error: {}",
                        error.message
                    )));
                }
                _ => {}
            }
        }

        Ok(text)
    }

    /// Collect the stream into a complete Message
    ///
    /// This reconstructs the full Message object from stream events,
    /// including all content blocks and usage information.
    ///
    /// # Example
    ///
    /// ```no_run
    /// // Requires ANTHROPIC_API_KEY environment variable
    /// # use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Anthropic::from_env()?;
    /// # let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    /// #     .user("Hello!")
    /// #     .build();
    /// let stream = client.messages().stream(params).await?;
    /// let message = stream.collect_message().await?;
    /// println!("Stop reason: {:?}", message.stop_reason);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn collect_message(mut self) -> Result<Message, AnthropicError> {
        let mut message: Option<Message> = None;
        let mut content_blocks: Vec<ContentBlockBuilder> = Vec::new();
        let mut stop_reason: Option<StopReason> = None;
        let mut stop_sequence: Option<String> = None;
        let mut final_usage: Option<Usage> = None;

        while let Some(event) = self.next().await {
            match event? {
                MessageStreamEvent::MessageStart { message: msg } => {
                    message = Some(msg);
                }
                MessageStreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => {
                    // Ensure we have enough slots
                    while content_blocks.len() <= index {
                        content_blocks.push(ContentBlockBuilder::new());
                    }
                    content_blocks[index].set_initial(content_block);
                }
                MessageStreamEvent::ContentBlockDelta { index, delta } => {
                    if index < content_blocks.len() {
                        content_blocks[index].apply_delta(delta);
                    }
                }
                MessageStreamEvent::ContentBlockStop { .. } => {
                    // Block is complete, nothing to do
                }
                MessageStreamEvent::MessageDelta { delta, usage } => {
                    stop_reason = delta.stop_reason;
                    stop_sequence = delta.stop_sequence;
                    if let Some(u) = usage {
                        if let Some(ref mut msg) = message {
                            msg.usage.output_tokens = u.output_tokens;
                        }
                        if let Some(ref mut usage) = final_usage {
                            usage.output_tokens = u.output_tokens;
                        }
                    }
                }
                MessageStreamEvent::MessageStop => break,
                MessageStreamEvent::Error { error } => {
                    return Err(AnthropicError::Stream(format!(
                        "Stream error: {}",
                        error.message
                    )));
                }
                MessageStreamEvent::Ping => {}
            }
        }

        let mut msg = message
            .ok_or_else(|| AnthropicError::Stream("No message_start received".to_string()))?;

        // Build final content blocks
        msg.content = content_blocks
            .into_iter()
            .filter_map(|b| b.build())
            .collect();
        msg.stop_reason = stop_reason;
        msg.stop_sequence = stop_sequence;

        if let Some(usage) = final_usage {
            msg.usage = usage;
        }

        Ok(msg)
    }

    /// Parse an SSE event into a MessageStreamEvent
    fn parse_event(event: Event) -> Result<Option<MessageStreamEvent>, AnthropicError> {
        match event {
            Event::Open => Ok(None),
            Event::Message(msg) => {
                // Skip empty data
                if msg.data.is_empty() {
                    return Ok(None);
                }

                // Parse the event data as JSON
                let stream_event: MessageStreamEvent =
                    serde_json::from_str(&msg.data).map_err(|e| {
                        AnthropicError::Stream(format!(
                            "Failed to parse stream event: {} (data: {})",
                            e, msg.data
                        ))
                    })?;

                Ok(Some(stream_event))
            }
        }
    }
}

impl Stream for MessageStream {
    type Item = Result<MessageStreamEvent, AnthropicError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    match Self::parse_event(event) {
                        Ok(Some(stream_event)) => {
                            // Check if this is a message_stop event
                            if matches!(stream_event, MessageStreamEvent::MessageStop) {
                                return Poll::Ready(Some(Ok(stream_event)));
                            }
                            return Poll::Ready(Some(Ok(stream_event)));
                        }
                        Ok(None) => {
                            // Skip this event (e.g., Open event or empty data)
                            continue;
                        }
                        Err(e) => return Poll::Ready(Some(Err(e))),
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    let error = match e {
                        reqwest_eventsource::Error::StreamEnded => {
                            // Stream ended normally
                            return Poll::Ready(None);
                        }
                        reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                            // Try to get error body
                            AnthropicError::Stream(format!(
                                "HTTP {}: {:?}",
                                status.as_u16(),
                                response
                            ))
                        }
                        reqwest_eventsource::Error::InvalidContentType(_, _) => {
                            AnthropicError::Stream("Invalid content type".to_string())
                        }
                        other => AnthropicError::Stream(format!("Stream error: {}", other)),
                    };
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Helper for building content blocks from stream deltas (internal use only)
#[derive(Debug)]
pub(crate) struct ContentBlockBuilder {
    block_type: Option<ContentBlockType>,
    text: String,
    tool_id: String,
    tool_name: String,
    tool_input_json: String,
    thinking: String,
    thinking_signature: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ContentBlockType {
    Text,
    ToolUse,
    Thinking,
    RedactedThinking,
    ServerToolUse,
    WebSearchToolResult,
}

impl ContentBlockBuilder {
    fn new() -> Self {
        Self {
            block_type: None,
            text: String::new(),
            tool_id: String::new(),
            tool_name: String::new(),
            tool_input_json: String::new(),
            thinking: String::new(),
            thinking_signature: String::new(),
        }
    }

    fn set_initial(&mut self, block: ContentBlock) {
        match block {
            ContentBlock::Text { text } => {
                self.block_type = Some(ContentBlockType::Text);
                self.text = text;
            }
            ContentBlock::ToolUse { id, name, input } => {
                self.block_type = Some(ContentBlockType::ToolUse);
                self.tool_id = id;
                self.tool_name = name;
                // Initial input in streaming is typically empty - don't serialize empty objects
                // as deltas will build up the full JSON
                if input.is_object() && input.as_object().is_some_and(|o| !o.is_empty()) {
                    self.tool_input_json = serde_json::to_string(&input).unwrap_or_default();
                }
            }
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                self.block_type = Some(ContentBlockType::Thinking);
                self.thinking = thinking;
                self.thinking_signature = signature;
            }
            ContentBlock::RedactedThinking { data } => {
                self.block_type = Some(ContentBlockType::RedactedThinking);
                self.text = data;
            }
            ContentBlock::ServerToolUse { id, name, input } => {
                self.block_type = Some(ContentBlockType::ServerToolUse);
                self.tool_id = id;
                self.tool_name = name;
                // Initial input in streaming is typically empty - don't serialize empty objects
                if input.is_object() && input.as_object().is_some_and(|o| !o.is_empty()) {
                    self.tool_input_json = serde_json::to_string(&input).unwrap_or_default();
                }
            }
            ContentBlock::WebSearchToolResult {
                tool_use_id,
                content,
            } => {
                self.block_type = Some(ContentBlockType::WebSearchToolResult);
                self.tool_id = tool_use_id;
                self.tool_input_json = serde_json::to_string(&content).unwrap_or_default();
            }
        }
    }

    fn apply_delta(&mut self, delta: ContentBlockDelta) {
        match delta {
            ContentBlockDelta::TextDelta { text } => {
                self.text.push_str(&text);
            }
            ContentBlockDelta::InputJsonDelta { partial_json } => {
                self.tool_input_json.push_str(&partial_json);
            }
            ContentBlockDelta::ThinkingDelta { thinking } => {
                self.thinking.push_str(&thinking);
            }
            ContentBlockDelta::SignatureDelta { signature } => {
                self.thinking_signature.push_str(&signature);
            }
        }
    }

    fn build(self) -> Option<ContentBlock> {
        match self.block_type? {
            ContentBlockType::Text => Some(ContentBlock::Text { text: self.text }),
            ContentBlockType::ToolUse => {
                let input = serde_json::from_str(&self.tool_input_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                Some(ContentBlock::ToolUse {
                    id: self.tool_id,
                    name: self.tool_name,
                    input,
                })
            }
            ContentBlockType::Thinking => Some(ContentBlock::Thinking {
                thinking: self.thinking,
                signature: self.thinking_signature,
            }),
            ContentBlockType::RedactedThinking => {
                Some(ContentBlock::RedactedThinking { data: self.text })
            }
            ContentBlockType::ServerToolUse => {
                let input = serde_json::from_str(&self.tool_input_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                Some(ContentBlock::ServerToolUse {
                    id: self.tool_id,
                    name: self.tool_name,
                    input,
                })
            }
            ContentBlockType::WebSearchToolResult => {
                let content = serde_json::from_str(&self.tool_input_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                Some(ContentBlock::WebSearchToolResult {
                    tool_use_id: self.tool_id,
                    content,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_delta_json() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let event: MessageStreamEvent = serde_json::from_str(json).unwrap();

        match event {
            MessageStreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    ContentBlockDelta::TextDelta { text } => {
                        assert_eq!(text, "Hello");
                    }
                    _ => panic!("Expected TextDelta"),
                }
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn test_parse_message_stop_json() {
        let json = r#"{"type":"message_stop"}"#;
        let event: MessageStreamEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, MessageStreamEvent::MessageStop));
    }

    #[test]
    fn test_parse_input_json_delta() {
        let json = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"foo\":"}}"#;
        let event: MessageStreamEvent = serde_json::from_str(json).unwrap();

        match event {
            MessageStreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 1);
                match delta {
                    ContentBlockDelta::InputJsonDelta { partial_json } => {
                        assert_eq!(partial_json, r#"{"foo":"#);
                    }
                    _ => panic!("Expected InputJsonDelta"),
                }
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn test_parse_open_event() {
        let event = Event::Open;
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_none());
    }

    // Helper to create an SSE message event
    fn make_message_event(data: &str) -> Event {
        use eventsource_stream::Event as SseEvent;
        Event::Message(SseEvent {
            event: "message".to_string(),
            data: data.to_string(),
            id: String::new(),
            retry: None,
        })
    }

    #[test]
    fn test_parse_empty_message_data() {
        let event = make_message_event("");
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_invalid_json() {
        let event = make_message_event("not valid json");
        let result = MessageStream::parse_event(event);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AnthropicError::Stream(_)));
    }

    #[test]
    fn test_parse_message_start_event() {
        let json = r#"{"type":"message_start","message":{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-20250514","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::MessageStart { message } => {
                assert_eq!(message.id, "msg_123");
            }
            _ => panic!("Expected MessageStart"),
        }
    }

    #[test]
    fn test_parse_content_block_start() {
        let json =
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::ContentBlockStart { index, .. } => {
                assert_eq!(index, 0);
            }
            _ => panic!("Expected ContentBlockStart"),
        }
    }

    #[test]
    fn test_parse_content_block_stop() {
        let json = r#"{"type":"content_block_stop","index":0}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::ContentBlockStop { index } => {
                assert_eq!(index, 0);
            }
            _ => panic!("Expected ContentBlockStop"),
        }
    }

    #[test]
    fn test_parse_message_delta() {
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":25}}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason, Some(StopReason::EndTurn));
                assert_eq!(usage.unwrap().output_tokens, 25);
            }
            _ => panic!("Expected MessageDelta"),
        }
    }

    #[test]
    fn test_parse_ping_event() {
        let json = r#"{"type":"ping"}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), MessageStreamEvent::Ping));
    }

    #[test]
    fn test_parse_error_event() {
        let json =
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"Too many requests"}}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::Error { error } => {
                assert_eq!(error.error_type, "rate_limit_error");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_parse_thinking_delta() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::ContentBlockDelta { delta, .. } => match delta {
                ContentBlockDelta::ThinkingDelta { thinking } => {
                    assert_eq!(thinking, "Let me think...");
                }
                _ => panic!("Expected ThinkingDelta"),
            },
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn test_parse_signature_delta() {
        let json = r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig_abc"}}"#;
        let event = make_message_event(json);
        let result = MessageStream::parse_event(event).unwrap();
        assert!(result.is_some());
        match result.unwrap() {
            MessageStreamEvent::ContentBlockDelta { delta, .. } => match delta {
                ContentBlockDelta::SignatureDelta { signature } => {
                    assert_eq!(signature, "sig_abc");
                }
                _ => panic!("Expected SignatureDelta"),
            },
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    // ===== ContentBlockBuilder Tests =====

    #[test]
    fn test_content_block_builder_text() {
        let mut builder = ContentBlockBuilder::new();
        builder.set_initial(ContentBlock::Text {
            text: "Hello".to_string(),
        });
        builder.apply_delta(ContentBlockDelta::TextDelta {
            text: " World".to_string(),
        });
        let block = builder.build();
        assert!(block.is_some());
        match block.unwrap() {
            ContentBlock::Text { text } => assert_eq!(text, "Hello World"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_builder_tool_use() {
        let mut builder = ContentBlockBuilder::new();
        builder.set_initial(ContentBlock::ToolUse {
            id: "tool_123".to_string(),
            name: "get_weather".to_string(),
            input: serde_json::json!({}),
        });
        builder.apply_delta(ContentBlockDelta::InputJsonDelta {
            partial_json: r#"{"city":"SF"}"#.to_string(),
        });
        let block = builder.build();
        assert!(block.is_some());
        match block.unwrap() {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tool_123");
                assert_eq!(name, "get_weather");
                assert_eq!(input["city"], "SF");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_content_block_builder_thinking() {
        let mut builder = ContentBlockBuilder::new();
        builder.set_initial(ContentBlock::Thinking {
            thinking: "Let me ".to_string(),
            signature: "".to_string(),
        });
        builder.apply_delta(ContentBlockDelta::ThinkingDelta {
            thinking: "think about this...".to_string(),
        });
        builder.apply_delta(ContentBlockDelta::SignatureDelta {
            signature: "sig_xyz".to_string(),
        });
        let block = builder.build();
        assert!(block.is_some());
        match block.unwrap() {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "Let me think about this...");
                assert_eq!(signature, "sig_xyz");
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    #[test]
    fn test_content_block_builder_empty() {
        let builder = ContentBlockBuilder::new();
        assert!(builder.build().is_none());
    }

    #[test]
    fn test_content_block_builder_redacted_thinking() {
        let mut builder = ContentBlockBuilder::new();
        builder.set_initial(ContentBlock::RedactedThinking {
            data: "encrypted_data".to_string(),
        });
        let block = builder.build();
        assert!(block.is_some());
        match block.unwrap() {
            ContentBlock::RedactedThinking { data } => {
                assert_eq!(data, "encrypted_data");
            }
            _ => panic!("Expected RedactedThinking block"),
        }
    }

    #[test]
    fn test_content_block_builder_multiple_text_deltas() {
        let mut builder = ContentBlockBuilder::new();
        builder.set_initial(ContentBlock::Text {
            text: "".to_string(),
        });
        builder.apply_delta(ContentBlockDelta::TextDelta {
            text: "One ".to_string(),
        });
        builder.apply_delta(ContentBlockDelta::TextDelta {
            text: "Two ".to_string(),
        });
        builder.apply_delta(ContentBlockDelta::TextDelta {
            text: "Three".to_string(),
        });
        let block = builder.build();
        match block.unwrap() {
            ContentBlock::Text { text } => assert_eq!(text, "One Two Three"),
            _ => panic!("Expected Text block"),
        }
    }
}
