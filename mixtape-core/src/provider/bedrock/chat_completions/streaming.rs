//! SSE payload assembly, separate from HTTP framing for deterministic fixtures.

use super::{conversion, protocol};
use crate::provider::bedrock::{BedrockInvocation, InvocationUsage};
use crate::provider::{ProviderError, StreamEvent};
use crate::types::{ContentBlock, StopReason};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct PendingTool {
    id: Option<String>,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub(super) struct StreamAssembler {
    model: Option<String>,
    text: String,
    has_text: bool,
    reasoning: String,
    has_reasoning: bool,
    refusal: String,
    tools: BTreeMap<u64, PendingTool>,
    finish_reason: Option<String>,
    usage: Option<InvocationUsage>,
    done: bool,
}

impl StreamAssembler {
    pub fn done(&self) -> bool {
        self.done
    }

    pub fn update_invocation(&self, record: &mut BedrockInvocation) {
        record.provider_reported_model = self.model.clone();
        record.provider_stop_reason = self.finish_reason.clone();
        record.usage = self.usage.clone();
    }

    pub fn rejected(&self) -> bool {
        !self.refusal.is_empty()
            || !matches!(self.finish_reason.as_deref(), Some("stop" | "tool_calls"))
    }

    pub fn push(&mut self, data: &str) -> Result<Vec<StreamEvent>, ProviderError> {
        if self.done {
            return Err(protocol(
                "Received data after the Chat Completions terminator",
            ));
        }
        if data == "[DONE]" {
            let events = self.finish()?;
            self.done = true;
            return Ok(events);
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|_| protocol("Malformed JSON in Chat Completions stream"))?;
        if value.get("error").is_some_and(|value| !value.is_null()) {
            return Err(protocol(
                "Bedrock returned an error in the Chat Completions stream",
            ));
        }
        if let Some(model) =
            conversion::optional_string(&value, "model")?.filter(|value| !value.is_empty())
        {
            if self.model.as_deref().is_some_and(|prior| prior != model) {
                return Err(protocol(
                    "Model identity changed within a Chat Completions stream",
                ));
            }
            self.model = Some(model.into());
        }
        if let Some(usage) = conversion::usage(&value["usage"])? {
            if self.usage.as_ref().is_some_and(|prior| *prior != usage) {
                return Err(protocol(
                    "Conflicting token usage in Chat Completions stream",
                ));
            }
            self.usage = Some(usage);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol("Missing Chat Completions stream choices"))?;
        if choices.is_empty() {
            if value.get("usage").is_none_or(Value::is_null) {
                return Err(protocol("Empty Chat Completions chunk without usage"));
            }
            return Ok(Vec::new());
        }
        if choices.len() != 1 || choices[0].get("index").and_then(Value::as_u64) != Some(0) {
            return Err(protocol(
                "Expected exactly one Chat Completions stream choice",
            ));
        }
        if self.finish_reason.is_some() {
            return Err(protocol(
                "Received another choice after Chat Completions finished",
            ));
        }
        let delta = &choices[0]["delta"];
        if !delta.is_object() {
            return Err(protocol("Missing Chat Completions delta"));
        }
        if conversion::optional_string(delta, "role")?.is_some_and(|role| role != "assistant") {
            return Err(protocol("Unexpected Chat Completions stream role"));
        }
        conversion::reject_unsupported_content(delta)?;
        let mut events = Vec::new();
        if let Some(text) = conversion::optional_string(delta, "content")? {
            self.text.push_str(text);
            self.has_text = true;
            if !text.is_empty() {
                events.push(StreamEvent::TextDelta(text.into()));
            }
        }
        if let Some(reasoning) = conversion::optional_string(delta, "reasoning_content")? {
            self.reasoning.push_str(reasoning);
            self.has_reasoning = true;
            if !reasoning.is_empty() {
                events.push(StreamEvent::ThinkingDelta(reasoning.into()));
            }
        }
        if let Some(refusal) = conversion::optional_string(delta, "refusal")? {
            self.refusal.push_str(refusal);
        }
        match delta.get("tool_calls") {
            None | Some(Value::Null) => {}
            Some(Value::Array(tools)) => {
                for tool in tools {
                    self.tool_delta(tool)?;
                }
            }
            _ => return Err(protocol("Chat Completions tool delta must be an array")),
        }
        if let Some(reason) = conversion::optional_string(&choices[0], "finish_reason")? {
            conversion::stop_reason(reason)?;
            self.finish_reason = Some(reason.into());
        }
        Ok(events)
    }

    fn tool_delta(&mut self, value: &Value) -> Result<(), ProviderError> {
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .filter(|index| *index < 128)
            .ok_or_else(|| protocol("Invalid Chat Completions tool index"))?;
        if conversion::optional_string(value, "type")?.is_some_and(|kind| kind != "function") {
            return Err(protocol("Unsupported Chat Completions tool type"));
        }
        let tool = self.tools.entry(index).or_default();
        if let Some(id) = conversion::optional_string(value, "id")? {
            if id.is_empty() || tool.id.as_deref().is_some_and(|prior| prior != id) {
                return Err(protocol("Conflicting Chat Completions tool ID"));
            }
            tool.id = Some(id.into());
        }
        let function = &value["function"];
        if !function.is_null() && !function.is_object() {
            return Err(protocol("Malformed Chat Completions function delta"));
        }
        if let Some(name) = conversion::optional_string(function, "name")? {
            tool.name.push_str(name);
        }
        if let Some(arguments) = conversion::optional_string(function, "arguments")? {
            tool.arguments.push_str(arguments);
        }
        Ok(())
    }

    fn finish(&self) -> Result<Vec<StreamEvent>, ProviderError> {
        let reason = self
            .finish_reason
            .as_deref()
            .ok_or_else(|| protocol("Chat Completions stream ended without a finish reason"))?;
        let mut reason = conversion::stop_reason(reason)?;
        if !self.refusal.is_empty() {
            reason = StopReason::ContentFiltered;
        }
        let mut value = json!({"role": "assistant", "content": if self.has_text { json!(self.text) } else { Value::Null }});
        if self.has_reasoning {
            value["reasoning_content"] = json!(self.reasoning);
        }
        let mut calls = Vec::new();
        for (expected, (index, tool)) in self.tools.iter().enumerate() {
            if *index != expected as u64 {
                return Err(protocol("Chat Completions stream has missing tool indexes"));
            }
            calls.push(json!({"type": "function", "id": tool.id,
                "function": {"name": tool.name, "arguments": tool.arguments}}));
        }
        if !calls.is_empty() {
            value["tool_calls"] = json!(calls);
        }
        let message = conversion::message(&value, reason)?;
        let mut events = Vec::new();
        for block in message.content {
            if let ContentBlock::ToolUse(tool) = &block {
                events.push(StreamEvent::ToolUse(tool.clone()));
            }
            events.push(StreamEvent::ContentBlock(block));
        }
        events.push(StreamEvent::Stop {
            stop_reason: reason,
            usage: conversion::agent_usage(self.usage.as_ref())?,
        });
        Ok(events)
    }
}
