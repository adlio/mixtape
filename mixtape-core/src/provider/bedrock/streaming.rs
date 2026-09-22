//! Lossless assembly of ConverseStream content, separate from network I/O.

use std::collections::BTreeMap;

use aws_sdk_bedrockruntime::types::{
    ContentBlockDelta, ContentBlockStart, ConverseStreamOutput, ReasoningContentBlockDelta,
};
use base64::Engine;

use super::conversion::from_bedrock_stop_reason;
use crate::events::TokenUsage;
use crate::provider::{ProviderError, StreamEvent};
use crate::types::{ContentBlock, StopReason, ToolUseBlock};

#[derive(Default)]
enum PendingBlock {
    #[default]
    Empty,
    Text(String),
    Thinking {
        text: String,
        signature: String,
    },
    Redacted(Vec<u8>),
    Tool {
        id: String,
        name: String,
        input: String,
    },
}

impl PendingBlock {
    fn finish(self) -> Result<ContentBlock, ProviderError> {
        match self {
            Self::Empty => Ok(ContentBlock::Text(String::new())),
            Self::Text(text) => Ok(ContentBlock::Text(text)),
            Self::Thinking { text, signature } => Ok(ContentBlock::Thinking {
                thinking: text,
                signature,
            }),
            Self::Redacted(bytes) => Ok(ContentBlock::RedactedThinking {
                data: base64::engine::general_purpose::STANDARD.encode(bytes),
            }),
            Self::Tool { id, name, input } => {
                let input: serde_json::Value = serde_json::from_str(&input)
                    .map_err(|_| invalid("Tool input is not valid JSON"))?;
                if !input.is_object() {
                    return Err(invalid("Tool input must be a JSON object"));
                }
                Ok(ContentBlock::ToolUse(ToolUseBlock { id, name, input }))
            }
        }
    }
}

fn invalid(message: &str) -> ProviderError {
    // Never include input JSON, reasoning, signatures, or prompts in errors.
    ProviderError::Model(format!("Invalid Bedrock stream: {message}"))
}

#[derive(Default)]
pub(super) struct StreamAssembler {
    pending: BTreeMap<i32, PendingBlock>,
    completed: BTreeMap<i32, ContentBlock>,
    stop_reason: Option<StopReason>,
    usage: Option<TokenUsage>,
    invocation_usage: Option<super::InvocationUsage>,
    raw_stop_reason: Option<String>,
    provider_latency_ms: Option<u64>,
}

impl StreamAssembler {
    pub(super) fn update_invocation(&self, record: &mut super::BedrockInvocation) {
        record.usage = self.invocation_usage.clone();
        record.provider_stop_reason = self.raw_stop_reason.clone();
        record.provider_latency_ms = self.provider_latency_ms;
    }

    pub(super) fn push(
        &mut self,
        event: ConverseStreamOutput,
    ) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.stop_reason.is_some() && !matches!(event, ConverseStreamOutput::Metadata(_)) {
            return Err(invalid("Content received after MessageStop"));
        }
        let mut events = Vec::new();
        match event {
            ConverseStreamOutput::MessageStart(_) => {}
            ConverseStreamOutput::ContentBlockStart(start) => {
                let index = start.content_block_index;
                if index < 0
                    || self.pending.contains_key(&index)
                    || self.completed.contains_key(&index)
                {
                    return Err(invalid("Duplicate or invalid content block index"));
                }
                let block = match start.start {
                    Some(ContentBlockStart::ToolUse(tool)) => PendingBlock::Tool {
                        id: tool.tool_use_id,
                        name: tool.name,
                        input: String::new(),
                    },
                    None => PendingBlock::Empty,
                    _ => return Err(invalid("Unsupported content block start")),
                };
                self.pending.insert(index, block);
            }
            ConverseStreamOutput::ContentBlockDelta(delta) => {
                let index = delta.content_block_index;
                if index < 0 || self.completed.contains_key(&index) {
                    return Err(invalid("Delta for a completed or invalid content block"));
                }
                // Text and reasoning blocks may begin directly with a delta.
                let block = self.pending.entry(index).or_default();
                match delta.delta {
                    Some(ContentBlockDelta::Text(text)) => {
                        if matches!(block, PendingBlock::Empty) {
                            *block = PendingBlock::Text(String::new());
                        }
                        let PendingBlock::Text(value) = block else {
                            return Err(invalid("Mixed content types in one block"));
                        };
                        value.push_str(&text);
                        events.push(StreamEvent::TextDelta(text));
                    }
                    Some(ContentBlockDelta::ToolUse(tool)) => {
                        let PendingBlock::Tool { input, .. } = block else {
                            return Err(invalid("Tool delta without a tool start"));
                        };
                        input.push_str(&tool.input);
                    }
                    Some(ContentBlockDelta::ReasoningContent(reasoning)) => match reasoning {
                        ReasoningContentBlockDelta::Text(text) => {
                            if matches!(block, PendingBlock::Empty) {
                                *block = PendingBlock::Thinking {
                                    text: String::new(),
                                    signature: String::new(),
                                };
                            }
                            let PendingBlock::Thinking { text: value, .. } = block else {
                                return Err(invalid("Mixed reasoning types in one block"));
                            };
                            value.push_str(&text);
                        }
                        ReasoningContentBlockDelta::Signature(signature) => {
                            if matches!(block, PendingBlock::Empty) {
                                *block = PendingBlock::Thinking {
                                    text: String::new(),
                                    signature: String::new(),
                                };
                            }
                            let PendingBlock::Thinking {
                                signature: value, ..
                            } = block
                            else {
                                return Err(invalid("Signature outside a reasoning block"));
                            };
                            value.push_str(&signature);
                        }
                        ReasoningContentBlockDelta::RedactedContent(bytes) => {
                            if matches!(block, PendingBlock::Empty) {
                                *block = PendingBlock::Redacted(Vec::new());
                            }
                            let PendingBlock::Redacted(value) = block else {
                                return Err(invalid("Mixed reasoning types in one block"));
                            };
                            value.extend_from_slice(bytes.as_ref());
                        }
                        _ => return Err(invalid("Unsupported reasoning delta")),
                    },
                    _ => return Err(invalid("Unsupported or empty content delta")),
                }
            }
            ConverseStreamOutput::ContentBlockStop(stop) => {
                let block = self
                    .pending
                    .remove(&stop.content_block_index)
                    .ok_or_else(|| invalid("ContentBlockStop without an open block"))?
                    .finish()?;
                if let ContentBlock::ToolUse(tool) = &block {
                    events.push(StreamEvent::ToolUse(tool.clone()));
                }
                self.completed.insert(stop.content_block_index, block);
            }
            ConverseStreamOutput::MessageStop(stop) => {
                if !self.pending.is_empty() {
                    return Err(invalid("Message stopped with unfinished content blocks"));
                }
                self.raw_stop_reason = Some(stop.stop_reason.as_str().to_owned());
                self.stop_reason = Some(from_bedrock_stop_reason(&stop.stop_reason));
            }
            ConverseStreamOutput::Metadata(metadata) => {
                if let Some(metrics) = metadata.metrics {
                    self.provider_latency_ms = u64::try_from(metrics.latency_ms).ok();
                }
                if let Some(usage) = metadata.usage {
                    self.invocation_usage = Some(super::InvocationUsage::from_bedrock(&usage));
                    self.usage = Some(TokenUsage {
                        input_tokens: usize::try_from(usage.input_tokens)
                            .map_err(|_| invalid("Negative input token count"))?,
                        output_tokens: usize::try_from(usage.output_tokens)
                            .map_err(|_| invalid("Negative output token count"))?,
                    });
                }
            }
            _ => return Err(invalid("Unsupported stream event")),
        }
        Ok(events)
    }

    pub(super) fn finish(self) -> Result<Vec<StreamEvent>, ProviderError> {
        let stop_reason = self
            .stop_reason
            .ok_or_else(|| invalid("Missing MessageStop"))?;
        let mut events: Vec<_> = self
            .completed
            .into_values()
            .map(StreamEvent::ContentBlock)
            .collect();
        events.push(StreamEvent::Stop {
            stop_reason,
            usage: self.usage,
        });
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_bedrockruntime::{
        primitives::Blob,
        types::{
            ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent,
            MessageStopEvent, ToolUseBlockDelta, ToolUseBlockStart,
        },
    };

    fn delta(index: i32, value: ContentBlockDelta) -> ConverseStreamOutput {
        ConverseStreamOutput::ContentBlockDelta(
            ContentBlockDeltaEvent::builder()
                .content_block_index(index)
                .delta(value)
                .build()
                .unwrap(),
        )
    }

    fn stop_block(index: i32) -> ConverseStreamOutput {
        ConverseStreamOutput::ContentBlockStop(
            ContentBlockStopEvent::builder()
                .content_block_index(index)
                .build()
                .unwrap(),
        )
    }

    fn stop_message() -> ConverseStreamOutput {
        ConverseStreamOutput::MessageStop(
            MessageStopEvent::builder()
                .stop_reason(aws_sdk_bedrockruntime::types::StopReason::EndTurn)
                .build()
                .unwrap(),
        )
    }

    fn start_tool(index: i32) -> ConverseStreamOutput {
        ConverseStreamOutput::ContentBlockStart(
            ContentBlockStartEvent::builder()
                .content_block_index(index)
                .start(ContentBlockStart::ToolUse(
                    ToolUseBlockStart::builder()
                        .tool_use_id("tool-1")
                        .name("lookup")
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
    }

    #[test]
    fn signed_reasoning_and_content_order_survive_streaming() {
        let mut state = StreamAssembler::default();
        for part in ["reason ", "carefully"] {
            state
                .push(delta(
                    0,
                    ContentBlockDelta::ReasoningContent(ReasoningContentBlockDelta::Text(
                        part.into(),
                    )),
                ))
                .unwrap();
        }
        for part in ["sig-", "one"] {
            state
                .push(delta(
                    0,
                    ContentBlockDelta::ReasoningContent(ReasoningContentBlockDelta::Signature(
                        part.into(),
                    )),
                ))
                .unwrap();
        }
        state.push(stop_block(0)).unwrap();
        state.push(start_tool(2)).unwrap();
        state
            .push(delta(
                2,
                ContentBlockDelta::ToolUse(
                    ToolUseBlockDelta::builder()
                        .input("{\"n\":1}")
                        .build()
                        .unwrap(),
                ),
            ))
            .unwrap();
        state.push(stop_block(2)).unwrap();
        state
            .push(delta(1, ContentBlockDelta::Text("checking".into())))
            .unwrap();
        state.push(stop_block(1)).unwrap();
        state.push(stop_message()).unwrap();
        let events = state.finish().unwrap();
        assert!(
            matches!(&events[0], StreamEvent::ContentBlock(ContentBlock::Thinking { thinking, signature })
            if thinking == "reason carefully" && signature == "sig-one")
        );
        assert!(
            matches!(&events[1], StreamEvent::ContentBlock(ContentBlock::Text(text)) if text == "checking")
        );
        assert!(
            matches!(&events[2], StreamEvent::ContentBlock(ContentBlock::ToolUse(tool)) if tool.input["n"] == 1)
        );
        assert!(matches!(&events[3], StreamEvent::Stop { usage: None, .. }));
    }

    #[test]
    fn redacted_reasoning_preserves_binary_chunks() {
        let mut state = StreamAssembler::default();
        for bytes in [vec![0, 255], vec![128, 1]] {
            state
                .push(delta(
                    0,
                    ContentBlockDelta::ReasoningContent(
                        ReasoningContentBlockDelta::RedactedContent(Blob::new(bytes)),
                    ),
                ))
                .unwrap();
        }
        state.push(stop_block(0)).unwrap();
        state.push(stop_message()).unwrap();
        let events = state.finish().unwrap();
        let StreamEvent::ContentBlock(ContentBlock::RedactedThinking { data }) = &events[0] else {
            panic!("Expected opaque reasoning")
        };
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap(),
            vec![0, 255, 128, 1]
        );
    }

    #[test]
    fn malformed_tool_arguments_are_never_replaced_with_empty_objects() {
        for json in ["", "{broken", "null", "[]", "42"] {
            let mut state = StreamAssembler::default();
            state.push(start_tool(0)).unwrap();
            state
                .push(delta(
                    0,
                    ContentBlockDelta::ToolUse(
                        ToolUseBlockDelta::builder().input(json).build().unwrap(),
                    ),
                ))
                .unwrap();
            assert!(state.push(stop_block(0)).is_err(), "Must reject {json:?}");
        }
    }

    #[test]
    fn incomplete_or_mixed_streams_fail_closed() {
        let mut state = StreamAssembler::default();
        state
            .push(delta(0, ContentBlockDelta::Text("partial".into())))
            .unwrap();
        assert!(state.finish().is_err());
        let mut state = StreamAssembler::default();
        state.push(start_tool(0)).unwrap();
        assert!(state.push(stop_message()).is_err());
        let mut state = StreamAssembler::default();
        state
            .push(delta(0, ContentBlockDelta::Text("text".into())))
            .unwrap();
        assert!(state
            .push(delta(
                0,
                ContentBlockDelta::ReasoningContent(ReasoningContentBlockDelta::Signature(
                    "signature".into()
                ))
            ))
            .is_err());
    }
}
