//! Kimi K3 Chat Completions cache controls, live-verified on Runtime 2026-09-23.
//! <https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-moonshot-ai-kimi-k3.html>
//! Breakpoints use Chat `text` parts, not Responses `input_text` parts. The service
//! accepted a 30m TTL, rejected invalid TTL/mode values, and reused the marked
//! prefix with a changed suffix. This is not a measured retention guarantee.

use super::invalid;
use crate::provider::ProviderError;
use crate::types::{ContentBlock, Message, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Request up to four explicit text-prefix checkpoints with a 30-minute TTL.
///
/// Positions refer to original Mixtape messages, before tool results expand into
/// wire messages. Only system text and user messages ending in text are eligible.
/// Omit this configuration for provider-default caching; an empty configuration
/// is rejected rather than advertised as cache-off. Hits remain provider-dependent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BedrockChatCache {
    pub key: Option<String>,
    pub system: bool,
    pub messages: BTreeSet<usize>,
}

impl BedrockChatCache {
    pub(super) fn validate(&self, model: &str) -> Result<(), ProviderError> {
        if model != "moonshotai.kimi-k3" {
            return Err(invalid("Explicit cache request syntax is only verified for Kimi K3 on Runtime Chat Completions"));
        }
        let checkpoints = self.messages.len() + usize::from(self.system);
        if checkpoints == 0 || checkpoints > 4 {
            return Err(invalid("Chat caching requires one to four explicit checkpoints; an empty set is not cache-off"));
        }
        if self.key.as_ref().is_some_and(|key| {
            key.is_empty() || key.len() > 64 || key.chars().any(char::is_control)
        }) {
            return Err(invalid(
                "Chat cache key must contain 1 to 64 bytes without control characters",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_request(
        &self,
        messages: &[Message],
        system: Option<&str>,
    ) -> Result<(), ProviderError> {
        if self.system && system.is_none() {
            return Err(invalid("System cache checkpoint requires a system prompt"));
        }
        for index in &self.messages {
            let message = messages
                .get(*index)
                .ok_or_else(|| invalid("Chat cache checkpoint index is outside this request"))?;
            if message.role != Role::User
                || !matches!(message.content.last(), Some(ContentBlock::Text(_)))
            {
                return Err(invalid(
                    "Chat cache checkpoints must end at user text, not tools or assistant replay",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn apply_options(&self, body: &mut Value) {
        body["prompt_cache_options"] = json!({"mode":"explicit", "ttl":"30m"});
        if let Some(key) = &self.key {
            body["prompt_cache_key"] = json!(key);
        }
    }
}

pub(super) fn content(text: &str, checkpoint: bool) -> Value {
    if checkpoint {
        json!([{"type":"text", "text":text, "prompt_cache_breakpoint":{"mode":"explicit"}}])
    } else {
        json!(text)
    }
}
