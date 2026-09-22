//! Responses items are the replay authority; text/tools are checked projections.

use super::{invalid, protocol};
use crate::events::TokenUsage;
use crate::model::ModelResponse;
use crate::provider::bedrock::{BedrockInvocation, InvocationUsage};
use crate::provider::ProviderError;
use crate::tool::ToolResult;
use crate::types::{
    ContentBlock, Message, ResponsesReplay, Role, StopReason, ToolDefinition, ToolResultStatus,
    ToolUseBlock,
};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, ProviderError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| protocol("Missing or invalid string in Responses item"))
}

pub(super) fn optional_string<'a>(
    value: &'a Value,
    field: &str,
) -> Result<Option<&'a str>, ProviderError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        _ => Err(protocol("Invalid optional string in Responses item")),
    }
}

pub(super) fn input(
    messages: &[Message],
    system: Option<&str>,
    model: &str,
) -> Result<Vec<Value>, ProviderError> {
    if messages.is_empty() {
        return Err(invalid("Responses requires a conversation"));
    }
    let mut input = Vec::new();
    if let Some(system) = system {
        input.push(json!({"type":"message", "role":"developer", "content":[{"type":"input_text", "text":system}]}));
    }
    for message in messages {
        if message.content.is_empty() {
            return Err(invalid("Responses messages must contain content"));
        }
        let replay: Vec<_> = message
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::ResponsesReplay(replay) = block {
                    Some(replay)
                } else {
                    None
                }
            })
            .collect();
        if !replay.is_empty() {
            if replay.len() != 1 || message.role != Role::Assistant || replay[0].model_id != model {
                return Err(invalid(
                    "Responses replay requires one assistant turn from the same selected model",
                ));
            }
            let projected = project(&replay[0].items)?;
            let actual: Vec<_> = message
                .content
                .iter()
                .filter(|block| !matches!(block, ContentBlock::ResponsesReplay(_)))
                .collect();
            if actual.len() != projected.len()
                || !actual
                    .iter()
                    .zip(&projected)
                    .all(|(left, right)| same_block(left, right))
            {
                return Err(invalid(
                    "Responses replay no longer matches this message's text and tool calls",
                ));
            }
            input.extend(replay[0].items.clone());
            continue;
        }
        for block in &message.content {
            match (message.role, block) {
                (role, ContentBlock::Text(text)) => {
                    let kind = if role == Role::User { "input_text" } else { "output_text" };
                    let mut part = json!({"type":kind, "text":text});
                    if role == Role::Assistant { part["annotations"] = json!([]); }
                    input.push(json!({"type":"message", "role":role.to_string(), "content":[part]}));
                }
                (Role::Assistant, ContentBlock::ToolUse(tool)) => {
                    if tool.id.is_empty() || tool.name.is_empty() || !tool.input.is_object() {
                        return Err(invalid("Responses tools require IDs, names, and JSON object arguments"));
                    }
                    input.push(json!({"type":"function_call", "call_id":tool.id, "name":tool.name, "arguments":tool.input.to_string()}));
                }
                (Role::User, ContentBlock::ToolResult(result)) => {
                    if result.tool_use_id.is_empty() { return Err(invalid("Responses tool results require a call ID")); }
                    let content = match &result.content {
                        ToolResult::Text(text) => text.clone(),
                        ToolResult::Json(value) => value.to_string(),
                        _ => return Err(invalid("Binary tool results are not supported by this Responses adapter")),
                    };
                    let content = if result.status == ToolResultStatus::Error {
                        json!({"is_error":true, "content":content}).to_string()
                    } else { content };
                    input.push(json!({"type":"function_call_output", "call_id":result.tool_use_id, "output":content}));
                }
                _ => return Err(invalid("Unsupported role or cross-API content in Responses history; refusing lossy replay")),
            }
        }
    }
    Ok(input)
}

fn same_block(left: &ContentBlock, right: &ContentBlock) -> bool {
    match (left, right) {
        (ContentBlock::Text(left), ContentBlock::Text(right)) => left == right,
        (ContentBlock::ToolUse(left), ContentBlock::ToolUse(right)) => {
            left.id == right.id && left.name == right.name && left.input == right.input
        }
        _ => false,
    }
}

pub(super) fn tools(tools: &[ToolDefinition]) -> Result<Vec<Value>, ProviderError> {
    let mut names = HashSet::new();
    tools.iter().map(|tool| {
        if tool.name.is_empty() || !names.insert(&tool.name) || !tool.input_schema.is_object() {
            return Err(invalid("Responses tools require unique names and JSON object schemas"));
        }
        // Strict output-schema control is independent. Do not silently rewrite a
        // caller's function schema to satisfy strict mode's required properties.
        Ok(json!({"type":"function", "name":tool.name, "description":tool.description, "parameters":tool.input_schema, "strict":false}))
    }).collect()
}

fn complete(item: &Value) -> Result<(), ProviderError> {
    if optional_string(item, "status")?.is_some_and(|status| status != "completed") {
        return Err(protocol(
            "A completed Responses turn contains an unfinished output item",
        ));
    }
    Ok(())
}

/// Produce only semantic text/tool blocks. Opaque data is retained separately,
/// never concatenated into text, presented as an answer, or parsed as tool JSON.
pub(super) fn project(items: &[Value]) -> Result<Vec<ContentBlock>, ProviderError> {
    let mut blocks = Vec::new();
    let mut ids = HashSet::new();
    let mut calls = HashSet::new();
    for item in items {
        complete(item)?;
        if let Some(id) = optional_string(item, "id")? {
            if id.is_empty() || !ids.insert(id) {
                return Err(protocol("Duplicate or empty Responses item ID"));
            }
        }
        match string(item, "type")? {
            "reasoning" => {
                string(item, "id")?;
                let summary = item
                    .get("summary")
                    .and_then(Value::as_array)
                    .ok_or_else(|| protocol("Responses reasoning requires its summary array"))?;
                for part in summary {
                    if string(part, "type")? != "summary_text" || !part["text"].is_string() {
                        return Err(protocol("Invalid Responses reasoning summary"));
                    }
                }
                let encrypted = optional_string(item, "encrypted_content")?
                    .is_some_and(|value| !value.is_empty());
                let mut has_content = false;
                match item.get("content") {
                    None | Some(Value::Null) => {}
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            if string(part, "type")? != "reasoning_text"
                                || !part["text"].is_string()
                            {
                                return Err(protocol("Unsupported Responses reasoning content"));
                            }
                            has_content |=
                                part["text"].as_str().is_some_and(|text| !text.is_empty());
                        }
                    }
                    _ => return Err(protocol("Invalid Responses reasoning content array")),
                }
                if !encrypted && !has_content {
                    return Err(protocol(
                        "Stateless Responses reasoning lacks replayable content",
                    ));
                }
            }
            "message" => {
                string(item, "id")?;
                if string(item, "role")? != "assistant" {
                    return Err(protocol("Expected an assistant Responses output message"));
                }
                optional_string(item, "phase")?;
                let content = item
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| protocol("Responses output message lacks content"))?;
                for part in content {
                    if string(part, "type")? != "output_text" {
                        return Err(protocol("Unsupported Responses output content"));
                    }
                    let text = part
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| protocol("Invalid Responses output text"))?;
                    blocks.push(ContentBlock::Text(text.into()));
                }
            }
            "function_call" => {
                if item.get("namespace").is_some_and(|value| !value.is_null())
                    || item
                        .get("async")
                        .is_some_and(|value| !value.is_null() && value != &json!(false))
                    || item
                        .get("caller")
                        .is_some_and(|caller| !caller.is_null() && caller["type"] != "direct")
                {
                    return Err(protocol(
                        "Unsupported namespaced, asynchronous, or hosted Responses function call",
                    ));
                }
                let id = string(item, "call_id")?;
                if !calls.insert(id) {
                    return Err(protocol("Duplicate Responses function call ID"));
                }
                let name = string(item, "name")?;
                let arguments = string(item, "arguments")?;
                let input: Value = serde_json::from_str(arguments)
                    .map_err(|_| protocol("Malformed Responses tool JSON"))?;
                if !input.is_object() {
                    return Err(protocol("Responses tool arguments must be a JSON object"));
                }
                blocks.push(ContentBlock::ToolUse(ToolUseBlock {
                    id: id.into(),
                    name: name.into(),
                    input,
                }));
            }
            _ => {
                return Err(protocol(
                    "Unsupported Responses output item; refusing lossy replay",
                ))
            }
        }
    }
    Ok(blocks)
}

fn counter(value: &Value, field: &str) -> Result<Option<u64>, ProviderError> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| protocol("Invalid Responses usage counter")),
    }
}

fn usage(value: &Value) -> Result<Option<InvocationUsage>, ProviderError> {
    if value.is_null() {
        return Ok(None);
    }
    if !value.is_object() {
        return Err(protocol("Invalid Responses usage object"));
    }
    for name in ["input_tokens_details", "output_tokens_details"] {
        if value
            .get(name)
            .is_some_and(|details| !details.is_null() && !details.is_object())
        {
            return Err(protocol("Invalid Responses token details"));
        }
    }
    Ok(Some(InvocationUsage {
        input_tokens: counter(value, "input_tokens")?,
        output_tokens: counter(value, "output_tokens")?,
        cache_read_input_tokens: counter(&value["input_tokens_details"], "cached_tokens")?,
        cache_write_input_tokens: counter(&value["input_tokens_details"], "cache_write_tokens")?,
        reasoning_tokens: counter(&value["output_tokens_details"], "reasoning_tokens")?,
    }))
}

fn agent_usage(value: Option<&InvocationUsage>) -> Result<Option<TokenUsage>, ProviderError> {
    let Some(value) = value else { return Ok(None) };
    match (value.input_tokens, value.output_tokens) {
        (Some(input), Some(output)) => Ok(Some(TokenUsage {
            input_tokens: input
                .try_into()
                .map_err(|_| protocol("Responses input usage overflow"))?,
            output_tokens: output
                .try_into()
                .map_err(|_| protocol("Responses output usage overflow"))?,
        })),
        _ => Ok(None),
    }
}

pub(super) fn response(
    value: &Value,
    model: &str,
    record: &mut BedrockInvocation,
) -> Result<ModelResponse, ProviderError> {
    string(value, "id")?;
    record.provider_reported_model = optional_string(value, "model")?
        .filter(|model| !model.is_empty())
        .map(str::to_owned);
    record.usage = usage(&value["usage"])?;
    let status = string(value, "status")?;
    record.provider_stop_reason = Some(status.into());
    if value.get("error").is_some_and(|error| !error.is_null())
        || !matches!(status, "completed" | "incomplete")
    {
        return Err(protocol(
            "Bedrock Responses failed or did not reach a supported terminal state",
        ));
    }
    let items = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol("Responses output must be an array"))?;
    let refusal = items.iter().any(|item| {
        item["type"] == "message"
            && item["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "refusal"))
    });
    let mut reason = if status == "incomplete" {
        let reason = string(&value["incomplete_details"], "reason")?;
        record.provider_stop_reason = Some(format!("incomplete:{reason}"));
        match reason {
            "max_output_tokens" => StopReason::MaxTokens,
            "content_filter" => StopReason::ContentFiltered,
            _ => return Err(protocol("Unsupported Responses incomplete reason")),
        }
    } else if refusal {
        StopReason::ContentFiltered
    } else {
        StopReason::EndTurn
    };
    if matches!(reason, StopReason::MaxTokens | StopReason::ContentFiltered) {
        // Never release partially generated tool calls or replay state from a
        // rejected turn. Direct callers can inspect partial text with its status.
        let content = items
            .iter()
            .filter_map(|item| item["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == "output_text")
            .filter_map(|part| part["text"].as_str())
            .map(|text| ContentBlock::Text(text.into()))
            .collect();
        return Ok(ModelResponse {
            message: Message::assistant_with_content(content),
            stop_reason: reason,
            usage: agent_usage(record.usage.as_ref())?,
        });
    }
    if value
        .get("incomplete_details")
        .is_some_and(|details| !details.is_null())
    {
        return Err(protocol("Completed Responses turn has incomplete details"));
    }
    let mut content = project(items)?;
    if content
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse(_)))
    {
        reason = StopReason::ToolUse;
    } else if !content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text(text) if !text.is_empty()))
    {
        return Err(protocol(
            "Responses completed without answer text or tool calls",
        ));
    }
    content.push(ContentBlock::ResponsesReplay(ResponsesReplay {
        model_id: model.into(),
        items: items.clone(),
    }));
    Ok(ModelResponse {
        message: Message::assistant_with_content(content),
        stop_reason: reason,
        usage: agent_usage(record.usage.as_ref())?,
    })
}
