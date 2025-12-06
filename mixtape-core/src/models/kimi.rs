//! Moonshot Kimi models

use super::define_model;

define_model!(
    /// Kimi K2 Thinking - Reasoning-enhanced model from Moonshot AI
    KimiK2Thinking {
        display_name: "Kimi K2 Thinking",
        bedrock_id: "moonshot.kimi-k2-thinking",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
