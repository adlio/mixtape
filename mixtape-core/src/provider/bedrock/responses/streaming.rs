//! Incremental display with authoritative, lossless items at the terminal event.
//! Tool calls are held until completion, preventing execution on truncated SSE.

use super::{conversion, protocol};
use crate::provider::bedrock::{BedrockInvocation, InvocationOutcome};
use crate::provider::{ProviderError, StreamEvent};
use crate::types::{ContentBlock, StopReason};
use serde_json::Value;
use std::collections::BTreeMap;

struct PendingItem {
    added: Value,
    done: Option<Value>,
    fragments: BTreeMap<(&'static str, usize), String>,
    arguments: Option<String>,
}

pub(super) struct StreamAssembler {
    model: String,
    response_id: Option<String>,
    provider_model: Option<String>,
    sequence: Option<u64>,
    items: BTreeMap<usize, PendingItem>,
    outcome: Option<InvocationOutcome>,
}

impl StreamAssembler {
    pub fn new(model: String) -> Self {
        Self {
            model,
            response_id: None,
            provider_model: None,
            sequence: None,
            items: BTreeMap::new(),
            outcome: None,
        }
    }

    pub fn outcome(&self) -> Option<InvocationOutcome> {
        self.outcome
    }

    pub fn push(
        &mut self,
        event_name: &str,
        data: &str,
        record: &mut BedrockInvocation,
    ) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.outcome.is_some() {
            return Err(protocol("Responses data followed its terminal event"));
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|_| protocol("Malformed JSON in Responses stream"))?;
        let kind = conversion::string(&value, "type")?;
        if !event_name.is_empty() && event_name != "message" && event_name != kind {
            return Err(protocol(
                "Responses SSE event name disagrees with its payload",
            ));
        }
        if let Some(sequence) = value.get("sequence_number") {
            let sequence = sequence
                .as_u64()
                .ok_or_else(|| protocol("Invalid Responses event sequence"))?;
            if self.sequence.is_some_and(|previous| sequence <= previous) {
                return Err(protocol(
                    "Responses event sequence is repeated or out of order",
                ));
            }
            self.sequence = Some(sequence);
        }
        match kind {
            "response.created" | "response.in_progress" => {
                self.identity(&value["response"], record)?;
                if conversion::string(&value["response"], "status")? != "in_progress" {
                    return Err(protocol("Unexpected Responses progress status"));
                }
                Ok(vec![])
            }
            "response.output_item.added" => {
                self.require_started()?;
                let index = index(&value, "output_index")?;
                if index != self.items.len() || index >= 2048 {
                    return Err(protocol(
                        "Responses output indexes are missing, repeated, or excessive",
                    ));
                }
                let item = &value["item"];
                conversion::string(item, "id")?;
                if !matches!(
                    conversion::string(item, "type")?,
                    "message" | "reasoning" | "function_call"
                ) {
                    return Err(protocol("Unsupported Responses streamed output item"));
                }
                self.items.insert(
                    index,
                    PendingItem {
                        added: item.clone(),
                        done: None,
                        fragments: BTreeMap::new(),
                        arguments: None,
                    },
                );
                Ok(vec![])
            }
            "response.output_item.done" => {
                let item = &value["item"];
                let pending = self.pending(&value, false)?;
                for field in ["id", "type", "call_id", "name", "role"] {
                    if pending
                        .added
                        .get(field)
                        .is_some_and(|prior| !prior.is_null() && item.get(field) != Some(prior))
                    {
                        return Err(protocol("Responses output item identity changed"));
                    }
                }
                validate_fragments(pending, item)?;
                pending.done = Some(item.clone());
                Ok(vec![])
            }
            "response.output_text.delta" => {
                self.delta(&value, "content", "content_index", "text", false)
            }
            "response.refusal.delta" => {
                self.delta(&value, "content", "content_index", "refusal", false)
            }
            "response.reasoning_summary_text.delta" => {
                self.delta(&value, "summary", "summary_index", "text", true)
            }
            // Kimi uses reasoning.*, while OpenAI uses reasoning_text.*.
            // Both carry the same indexed reasoning_text content contract.
            "response.reasoning_text.delta" | "response.reasoning.delta" => {
                self.delta(&value, "reasoning", "content_index", "text", true)
            }
            "response.output_text.done" => {
                self.text_done(&value, "content", "content_index", "text")
            }
            "response.refusal.done" => {
                self.text_done(&value, "content", "content_index", "refusal")
            }
            "response.reasoning_summary_text.done" => {
                self.text_done(&value, "summary", "summary_index", "text")
            }
            "response.reasoning_text.done" | "response.reasoning.done" => {
                self.text_done(&value, "reasoning", "content_index", "text")
            }
            "response.function_call_arguments.delta" => {
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| protocol("Invalid Responses function argument delta"))?;
                let pending = self.pending(&value, true)?;
                if pending.added["type"] != "function_call" {
                    return Err(protocol("Argument delta belongs to a non-function item"));
                }
                pending
                    .arguments
                    .get_or_insert_with(String::new)
                    .push_str(delta);
                Ok(vec![])
            }
            "response.function_call_arguments.done" => {
                let arguments = conversion::string(&value, "arguments")?;
                let pending = self.pending(&value, true)?;
                if pending.added["type"] != "function_call"
                    || pending
                        .arguments
                        .as_deref()
                        .is_some_and(|prior| prior != arguments)
                {
                    return Err(protocol(
                        "Final Responses arguments disagree with streamed arguments",
                    ));
                }
                pending.arguments = Some(arguments.into());
                Ok(vec![])
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done" => {
                let summary = kind.contains("reasoning_summary");
                index(
                    &value,
                    if summary {
                        "summary_index"
                    } else {
                        "content_index"
                    },
                )?;
                let pending = self.pending(&value, true)?;
                let part = &value["part"];
                let part_type = conversion::string(part, "type")?;
                let expected_item = if summary || part_type == "reasoning_text" {
                    "reasoning"
                } else {
                    "message"
                };
                if pending.added["type"] != expected_item
                    || !matches!(
                        part_type,
                        "output_text" | "refusal" | "summary_text" | "reasoning_text"
                    )
                {
                    return Err(protocol("Unsupported Responses content part"));
                }
                Ok(vec![])
            }
            "response.output_text.annotation.added" => {
                index(&value, "content_index")?;
                let pending = self.pending(&value, true)?;
                if pending.added["type"] != "message" {
                    return Err(protocol(
                        "Response annotation belongs to a non-message item",
                    ));
                }
                // Complete annotations are preserved in output_item.done.
                Ok(vec![])
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                self.finish(kind, &value["response"], record)
            }
            "error" => Err(protocol("Bedrock returned a Responses stream error")),
            _ => Err(protocol(
                "Unsupported Responses SSE event; refusing incomplete replay",
            )),
        }
    }

    fn identity(
        &mut self,
        response: &Value,
        record: &mut BedrockInvocation,
    ) -> Result<(), ProviderError> {
        let id = conversion::string(response, "id")?;
        if self
            .response_id
            .as_deref()
            .is_some_and(|previous| previous != id)
        {
            return Err(protocol("Responses identity changed within the stream"));
        }
        self.response_id = Some(id.into());
        if let Some(model) =
            conversion::optional_string(response, "model")?.filter(|model| !model.is_empty())
        {
            if self
                .provider_model
                .as_deref()
                .is_some_and(|previous| previous != model)
            {
                return Err(protocol(
                    "Provider model identity changed within the Responses stream",
                ));
            }
            self.provider_model = Some(model.into());
        }
        record.provider_reported_model = self.provider_model.clone();
        Ok(())
    }

    fn require_started(&self) -> Result<(), ProviderError> {
        if self.response_id.is_none() {
            return Err(protocol("Responses stream lacks its creation event"));
        }
        Ok(())
    }

    fn pending(
        &mut self,
        value: &Value,
        check_item_id: bool,
    ) -> Result<&mut PendingItem, ProviderError> {
        self.require_started()?;
        let pending = self
            .items
            .get_mut(&index(value, "output_index")?)
            .ok_or_else(|| protocol("Responses event references an unknown output item"))?;
        if pending.done.is_some() {
            return Err(protocol("Responses output item changed after completion"));
        }
        if check_item_id && value.get("item_id") != pending.added.get("id") {
            return Err(protocol("Responses event item ID does not match its index"));
        }
        Ok(pending)
    }

    fn delta(
        &mut self,
        value: &Value,
        field: &'static str,
        index_field: &str,
        text_field: &str,
        thinking: bool,
    ) -> Result<Vec<StreamEvent>, ProviderError> {
        let position = index(value, index_field)?;
        let delta = value
            .get("delta")
            .and_then(Value::as_str)
            .ok_or_else(|| protocol("Invalid Responses text delta"))?;
        let pending = self.pending(value, true)?;
        let expected = if thinking { "reasoning" } else { "message" };
        if pending.added["type"] != expected {
            return Err(protocol(
                "Responses text delta belongs to the wrong item type",
            ));
        }
        let field = if text_field == "refusal" {
            "refusal"
        } else {
            field
        };
        pending
            .fragments
            .entry((field, position))
            .or_default()
            .push_str(delta);
        if text_field == "refusal" || delta.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![if thinking {
            StreamEvent::ThinkingDelta(delta.into())
        } else {
            StreamEvent::TextDelta(delta.into())
        }])
    }

    fn text_done(
        &mut self,
        value: &Value,
        field: &'static str,
        index_field: &str,
        text_field: &str,
    ) -> Result<Vec<StreamEvent>, ProviderError> {
        let position = index(value, index_field)?;
        let text = value
            .get(text_field)
            .and_then(Value::as_str)
            .ok_or_else(|| protocol("Invalid Responses final text"))?;
        let pending = self.pending(value, true)?;
        let field = if text_field == "refusal" {
            "refusal"
        } else {
            field
        };
        if pending
            .fragments
            .get(&(field, position))
            .is_some_and(|prior| prior != text)
        {
            return Err(protocol(
                "Final Responses text disagrees with streamed text",
            ));
        }
        pending.fragments.insert((field, position), text.into());
        Ok(vec![])
    }

    fn finish(
        &mut self,
        kind: &str,
        value: &Value,
        record: &mut BedrockInvocation,
    ) -> Result<Vec<StreamEvent>, ProviderError> {
        self.require_started()?;
        self.identity(value, record)?;
        if conversion::string(value, "status")? != kind.trim_start_matches("response.") {
            return Err(protocol(
                "Responses terminal event disagrees with response status",
            ));
        }
        let response = conversion::response(value, &self.model, record)?;
        // A terminal response may omit the model field; do not erase identity
        // actually reported earlier, or synthesize one from the request target.
        record.provider_reported_model = self.provider_model.clone();
        let completed = matches!(
            response.stop_reason,
            StopReason::EndTurn | StopReason::ToolUse
        );
        if completed {
            let items = value["output"]
                .as_array()
                .ok_or_else(|| protocol("Missing Responses output"))?;
            if items.len() != self.items.len() {
                return Err(protocol(
                    "Terminal Responses output differs from streamed items",
                ));
            }
            for (index, item) in items.iter().enumerate() {
                let pending = self
                    .items
                    .get(&index)
                    .ok_or_else(|| protocol("Missing Responses output item index"))?;
                if pending.done.as_ref() != Some(item) {
                    return Err(protocol(
                        "Terminal Responses output differs from completed output items",
                    ));
                }
            }
        }
        self.outcome = Some(if completed {
            InvocationOutcome::Completed
        } else {
            InvocationOutcome::Rejected
        });
        let mut events = Vec::new();
        for block in response.message.content {
            if let ContentBlock::ToolUse(tool) = &block {
                events.push(StreamEvent::ToolUse(tool.clone()));
            }
            events.push(StreamEvent::ContentBlock(block));
        }
        events.push(StreamEvent::Stop {
            stop_reason: response.stop_reason,
            usage: response.usage,
        });
        Ok(events)
    }
}

fn index(value: &Value, field: &str) -> Result<usize, ProviderError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .filter(|value| *value < 2048)
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| protocol("Invalid Responses content/output index"))
}

fn validate_fragments(pending: &PendingItem, item: &Value) -> Result<(), ProviderError> {
    if pending
        .arguments
        .as_deref()
        .is_some_and(|arguments| item["arguments"].as_str() != Some(arguments))
    {
        return Err(protocol(
            "Completed Responses arguments differ from streamed arguments",
        ));
    }
    for ((field, index), text) in &pending.fragments {
        let array = if *field == "summary" {
            "summary"
        } else {
            "content"
        };
        let key = if *field == "refusal" {
            "refusal"
        } else {
            "text"
        };
        if item[array]
            .as_array()
            .and_then(|parts| parts.get(*index))
            .and_then(|part| part[key].as_str())
            != Some(text)
        {
            return Err(protocol(
                "Completed Responses content differs from streamed content",
            ));
        }
    }
    Ok(())
}
