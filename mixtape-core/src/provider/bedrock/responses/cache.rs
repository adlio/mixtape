//! GPT-5.6, Kimi K3, and GPT-6 Astra Runtime Responses cache controls.
//! Astra Runtime breakpoints and 30m TTL were live-verified on 2026-09-23;
//! its model card documents caching under Mantle but is silent about Runtime.
//! No endpoint fallback is performed and TTL retention has not been measured.
//! <https://docs.aws.amazon.com/bedrock/latest/userguide/prompt-caching.html>
//! <https://aws.amazon.com/blogs/machine-learning/introducing-kimi-k3-on-amazon-bedrock/>

use super::invalid;
use crate::provider::ProviderError;
use crate::types::{ContentBlock, Message, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BedrockResponsesCacheMode {
    /// Use explicit checkpoints in addition to the provider's automatic one.
    #[default]
    Implicit,
    /// Request explicit checkpoint selection. An empty set requests no caching
    /// on GPT-5.6. Kimi and Astra require a checkpoint: Kimi's empty set still
    /// read warmed cache, and Astra's empty-set semantics remain unverified.
    Explicit,
}

/// Positions refer to the original Mixtape messages, not flattened wire items.
/// Only developer instructions and user text boundaries are supported. TTL is
/// the documented 30 minutes; eligibility and hits are determined by Bedrock.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BedrockResponsesCache {
    pub mode: BedrockResponsesCacheMode,
    pub key: Option<String>,
    pub system: bool,
    pub messages: BTreeSet<usize>,
}

impl BedrockResponsesCache {
    pub(super) fn validate(&self, model: &str) -> Result<(), ProviderError> {
        if !matches!(
            model,
            "openai.gpt-5.6-sol"
                | "openai.gpt-5.6-terra"
                | "openai.gpt-5.6-luna"
                | "openai.gpt-6-astra"
                | "moonshotai.kimi-k3"
        ) {
            return Err(invalid(
                "Explicit cache request syntax is not verified for this model on Runtime Responses",
            ));
        }
        // Kimi's warmed cache still read with an empty explicit set. Astra's
        // empty-set behavior has not been verified. Neither may claim cache-off.
        if matches!(model, "moonshotai.kimi-k3" | "openai.gpt-6-astra")
            && matches!(self.mode, BedrockResponsesCacheMode::Explicit)
            && !self.system
            && self.messages.is_empty()
        {
            return Err(invalid(
                "This model requires an explicit cache checkpoint; an empty set is not verified to disable caching",
            ));
        }
        if self.messages.len() + usize::from(self.system) > 4 {
            return Err(invalid(
                "Responses supports at most four explicit cache checkpoints",
            ));
        }
        if self.key.as_ref().is_some_and(|key| {
            key.is_empty() || key.len() > 64 || key.chars().any(char::is_control)
        }) {
            return Err(invalid(
                "Responses cache key must contain 1 to 64 bytes without control characters",
            ));
        }
        Ok(())
    }

    pub(super) fn apply(
        &self,
        body: &mut Value,
        messages: &[Message],
        system: Option<&str>,
    ) -> Result<(), ProviderError> {
        let input = body["input"]
            .as_array_mut()
            .ok_or_else(|| invalid("Missing Responses cache input"))?;
        let mut offset = usize::from(system.is_some());
        if self.system {
            if system.is_none() {
                return Err(invalid("System cache checkpoint requires a system prompt"));
            }
            checkpoint(&mut input[0])?;
        }
        for (index, message) in messages.iter().enumerate() {
            let count = message
                .content
                .iter()
                .find_map(|block| {
                    if let ContentBlock::ResponsesReplay(replay) = block {
                        Some(replay.items.len())
                    } else {
                        None
                    }
                })
                .unwrap_or(message.content.len());
            if self.messages.contains(&index) {
                if message.role != Role::User
                    || !matches!(message.content.last(), Some(ContentBlock::Text(_)))
                {
                    return Err(invalid("Responses cache checkpoints must end at user text, not tools or assistant replay"));
                }
                let item = offset
                    .checked_add(count)
                    .and_then(|value| value.checked_sub(1))
                    .and_then(|index| input.get_mut(index))
                    .ok_or_else(|| invalid("Responses cache checkpoint is outside this request"))?;
                checkpoint(item)?;
            }
            offset += count;
        }
        if self
            .messages
            .last()
            .is_some_and(|index| *index >= messages.len())
            || offset != input.len()
        {
            return Err(invalid(
                "Responses cache checkpoint index is outside this request",
            ));
        }
        body["prompt_cache_options"] = json!({"mode":self.mode, "ttl":"30m"});
        if let Some(key) = &self.key {
            body["prompt_cache_key"] = json!(key);
        }
        Ok(())
    }
}

fn checkpoint(item: &mut Value) -> Result<(), ProviderError> {
    let part = item["content"]
        .as_array_mut()
        .and_then(|parts| parts.last_mut())
        .ok_or_else(|| invalid("Responses cache checkpoint needs a text content block"))?;
    if part["type"] != "input_text" {
        return Err(invalid("Responses cache checkpoint requires input_text"));
    }
    part["prompt_cache_breakpoint"] = json!({"mode":"explicit"});
    Ok(())
}
