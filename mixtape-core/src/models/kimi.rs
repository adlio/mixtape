//! Moonshot Kimi models
//!
//! Note: Bedrock uses different vendor prefixes for these models —
//! `moonshot.` for K2 Thinking and `moonshotai.` for K2.5.

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

define_model!(
    /// Kimi K2.5 - Next-gen model from Moonshot AI
    KimiK2_5 {
        display_name: "Kimi K2.5",
        bedrock_id: "moonshotai.kimi-k2.5",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);
