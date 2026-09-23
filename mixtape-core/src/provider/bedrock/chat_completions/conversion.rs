//! Chat Completions wire conversion. Unsupported content is rejected, never
//! flattened into answer text or silently removed from conversation history.

use super::{invalid, protocol};
use crate::events::TokenUsage;
use crate::model::ModelResponse;
use crate::provider::bedrock::{BedrockInvocation, InvocationUsage};
use crate::provider::ProviderError;
use crate::tool::ToolResult;
use crate::types::{
    ContentBlock, Message, Role, StopReason, ToolDefinition, ToolResultStatus, ToolUseBlock,
};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) fn messages(
    input: &[Message],
    system: Option<&str>,
    cache: Option<&super::BedrockChatCache>,
) -> Result<Vec<Value>, ProviderError> {
    if let Some(cache) = cache {
        cache.validate_request(input, system)?;
    }
    let mut result = Vec::new();
    if let Some(system) = system {
        result.push(json!({"role": "system", "content": super::cache::content(system, cache.is_some_and(|cache| cache.system))}));
    }
    for (index, message) in input.iter().enumerate() {
        if message.content.is_empty() {
            return Err(invalid(
                "Chat messages must contain at least one content block",
            ));
        }
        match message.role {
            Role::Assistant => {
                let mut text = String::new();
                let mut reasoning = None::<String>;
                let mut tools = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text(value) => text.push_str(value),
                        ContentBlock::Thinking {
                            thinking,
                            signature,
                        } if signature.is_empty() => {
                            reasoning.get_or_insert_with(String::new).push_str(thinking);
                        }
                        ContentBlock::ToolUse(tool) => {
                            if tool.id.is_empty() || tool.name.is_empty() || !tool.input.is_object()
                            {
                                return Err(invalid(
                                    "Tool calls require IDs, names, and JSON object arguments",
                                ));
                            }
                            tools.push(json!({
                                "id": tool.id, "type": "function",
                                "function": {"name": tool.name, "arguments": tool.input.to_string()}
                            }));
                        }
                        ContentBlock::ResponsesReplay(_) => {
                            return Err(invalid(
                                "Responses history cannot be replayed through Chat Completions",
                            ));
                        }
                        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
                            return Err(invalid("Signed or redacted reasoning cannot be replayed through Chat Completions"));
                        }
                        ContentBlock::ToolResult(_) => {
                            return Err(invalid("Tool results must use the user role"));
                        }
                    }
                }
                let mut output = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
                if let Some(reasoning) = reasoning {
                    output["reasoning_content"] = json!(reasoning);
                }
                if !tools.is_empty() {
                    output["tool_calls"] = json!(tools);
                }
                result.push(output);
            }
            Role::User => {
                // Tool results are standalone Chat Completions messages. Preserve
                // order when the internal user message also includes plain text.
                let mut text = None::<String>;
                for block in &message.content {
                    match block {
                        ContentBlock::Text(value) => {
                            text.get_or_insert_with(String::new).push_str(value)
                        }
                        ContentBlock::ToolResult(tool) => {
                            if let Some(text) = text.take() {
                                result.push(json!({"role": "user", "content": text}));
                            }
                            if tool.tool_use_id.is_empty() {
                                return Err(invalid("Tool results require a tool call ID"));
                            }
                            let content = match &tool.content {
                                ToolResult::Text(text) => text.clone(),
                                ToolResult::Json(value) => value.to_string(),
                                ToolResult::Image { .. } | ToolResult::Document { .. } => {
                                    return Err(invalid("Image and document tool results are not supported by this Chat Completions adapter"));
                                }
                            };
                            let content = if tool.status == ToolResultStatus::Error {
                                json!({"is_error": true, "content": content}).to_string()
                            } else {
                                content
                            };
                            result.push(json!({"role": "tool", "tool_call_id": tool.tool_use_id, "content": content}));
                        }
                        _ => {
                            return Err(invalid(
                                "User messages cannot contain assistant reasoning or tool calls",
                            ))
                        }
                    }
                }
                if let Some(text) = text {
                    let checkpoint = cache.is_some_and(|cache| cache.messages.contains(&index));
                    result.push(json!({"role": "user", "content": super::cache::content(&text, checkpoint)}));
                }
            }
        }
    }
    if input.is_empty() {
        return Err(invalid("Chat Completions requires a conversation"));
    }
    Ok(result)
}

pub(super) fn tools(input: &[ToolDefinition]) -> Result<Vec<Value>, ProviderError> {
    let mut names = HashSet::new();
    input
        .iter()
        .map(|tool| {
            if tool.name.is_empty() || !names.insert(&tool.name) || !tool.input_schema.is_object() {
                return Err(invalid(
                    "Tools require unique names and JSON object schemas",
                ));
            }
            Ok(json!({"type": "function", "function": {
                "name": tool.name, "description": tool.description, "parameters": tool.input_schema
            }}))
        })
        .collect()
}

pub(super) fn stop_reason(value: &str) -> Result<StopReason, ProviderError> {
    match value {
        "stop" => Ok(StopReason::EndTurn),
        "tool_calls" => Ok(StopReason::ToolUse),
        "length" => Ok(StopReason::MaxTokens),
        "content_filter" => Ok(StopReason::ContentFiltered),
        _ => Err(protocol("Unknown Chat Completions finish reason")),
    }
}

pub(super) fn optional_string<'a>(
    value: &'a Value,
    field: &str,
) -> Result<Option<&'a str>, ProviderError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        _ => Err(protocol("Expected a string in Chat Completions response")),
    }
}

pub(super) fn tool_call(value: &Value) -> Result<ToolUseBlock, ProviderError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let function = value.get("function");
    let name = function
        .and_then(|f| f.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty());
    let arguments = function
        .and_then(|f| f.get("arguments"))
        .and_then(Value::as_str);
    if value.get("type").and_then(Value::as_str) != Some("function") {
        return Err(protocol("Unsupported Chat Completions tool type"));
    }
    let (Some(id), Some(name), Some(arguments)) = (id, name, arguments) else {
        return Err(protocol("Incomplete Chat Completions tool call"));
    };
    let input: Value = serde_json::from_str(arguments)
        .map_err(|_| protocol("Invalid JSON in Chat Completions tool arguments"))?;
    if !input.is_object() {
        return Err(protocol(
            "Chat Completions tool arguments must be a JSON object",
        ));
    }
    Ok(ToolUseBlock {
        id: id.into(),
        name: name.into(),
        input,
    })
}

pub(super) fn message(value: &Value, reason: StopReason) -> Result<Message, ProviderError> {
    if value.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err(protocol("Expected an assistant Chat Completions response"));
    }
    reject_unsupported_content(value)?;
    let mut content = Vec::new();
    if let Some(reasoning) = optional_string(value, "reasoning_content")? {
        content.push(ContentBlock::Thinking {
            thinking: reasoning.into(),
            signature: String::new(),
        });
    }
    if let Some(text) = optional_string(value, "content")? {
        content.push(ContentBlock::Text(text.into()));
    }
    let mut tool_ids = HashSet::new();
    match value.get("tool_calls") {
        None | Some(Value::Null) => {}
        Some(Value::Array(tools)) => {
            for tool in tools {
                let tool = tool_call(tool)?;
                if !tool_ids.insert(tool.id.clone()) {
                    return Err(protocol("Duplicate Chat Completions tool call ID"));
                }
                content.push(ContentBlock::ToolUse(tool));
            }
        }
        _ => return Err(protocol("Chat Completions tool_calls must be an array")),
    }
    if (reason == StopReason::ToolUse && tool_ids.is_empty())
        || (reason == StopReason::EndTurn && !tool_ids.is_empty())
    {
        return Err(protocol(
            "Chat Completions finish reason disagrees with tool calls",
        ));
    }
    if reason == StopReason::EndTurn
        && !content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text(text) if !text.is_empty()))
    {
        return Err(protocol("Chat Completions stopped without answer content"));
    }
    Ok(Message::assistant_with_content(content))
}

pub(super) fn reject_unsupported_content(value: &Value) -> Result<(), ProviderError> {
    for field in ["reasoning", "reasoning_details", "audio", "function_call"] {
        if value.get(field).is_some_and(|value| !value.is_null()) {
            return Err(protocol(
                "Unsupported Chat Completions content; refusing lossy replay",
            ));
        }
    }
    Ok(())
}

fn counter(value: &Value, field: &str) -> Result<Option<u64>, ProviderError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(number) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| protocol("Invalid Chat Completions token counter")),
    }
}

pub(super) fn usage(value: &Value) -> Result<Option<InvocationUsage>, ProviderError> {
    if value.is_null() {
        return Ok(None);
    }
    if !value.is_object() {
        return Err(protocol("Invalid Chat Completions usage object"));
    }
    for field in ["prompt_tokens_details", "completion_tokens_details"] {
        if value
            .get(field)
            .is_some_and(|details| !details.is_null() && !details.is_object())
        {
            return Err(protocol("Invalid Chat Completions token details"));
        }
    }
    let input_details = &value["prompt_tokens_details"];
    Ok(Some(InvocationUsage {
        input_tokens: counter(value, "prompt_tokens")?,
        output_tokens: counter(value, "completion_tokens")?,
        cache_read_input_tokens: counter(input_details, "cached_tokens")?,
        cache_write_input_tokens: counter(input_details, "cache_write_tokens")?,
        reasoning_tokens: counter(&value["completion_tokens_details"], "reasoning_tokens")?,
    }))
}

pub(super) fn agent_usage(
    value: Option<&InvocationUsage>,
) -> Result<Option<TokenUsage>, ProviderError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match (value.input_tokens, value.output_tokens) {
        (Some(input), Some(output)) => Ok(Some(TokenUsage {
            input_tokens: input
                .try_into()
                .map_err(|_| protocol("Input token counter overflow"))?,
            output_tokens: output
                .try_into()
                .map_err(|_| protocol("Output token counter overflow"))?,
        })),
        _ => Ok(None),
    }
}

pub(super) fn response(
    value: &Value,
    record: &mut BedrockInvocation,
) -> Result<ModelResponse, ProviderError> {
    if value.get("error").is_some_and(|value| !value.is_null()) {
        return Err(protocol(
            "Bedrock returned a Chat Completions error envelope",
        ));
    }
    record.provider_reported_model = optional_string(value, "model")?
        .filter(|model| !model.is_empty())
        .map(str::to_owned);
    record.usage = usage(&value["usage"])?;
    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .filter(|choices| choices.len() == 1)
        .ok_or_else(|| protocol("Expected exactly one Chat Completions choice"))?;
    if choices[0].get("index").and_then(Value::as_u64) != Some(0) {
        return Err(protocol("Unexpected Chat Completions choice index"));
    }
    let finish = choices[0]
        .get("finish_reason")
        .and_then(Value::as_str)
        .ok_or_else(|| protocol("Chat Completions response lacks a finish reason"))?;
    record.provider_stop_reason = Some(finish.into());
    let mut reason = stop_reason(finish)?;
    let raw_message = &choices[0]["message"];
    if optional_string(raw_message, "refusal")?.is_some_and(|text| !text.is_empty()) {
        reason = StopReason::ContentFiltered;
    }
    Ok(ModelResponse {
        message: message(raw_message, reason)?,
        stop_reason: reason,
        usage: agent_usage(record.usage.as_ref())?,
    })
}
