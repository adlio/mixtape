//! DeepSeek models

use super::define_model;

define_model!(
    /// DeepSeek R1 - Reasoning-focused model
    DeepSeekR1 {
        display_name: "DeepSeek R1",
        bedrock_id: "deepseek.r1-v1:0",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// DeepSeek V3.1 - General purpose model
    DeepSeekV3 {
        display_name: "DeepSeek V3.1",
        bedrock_id: "deepseek.v3-v1:0",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
