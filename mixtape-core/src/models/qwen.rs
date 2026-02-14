//! Alibaba Qwen models

use super::define_model;

define_model!(
    /// Qwen3 235B - Large MoE model with 22B active parameters
    Qwen3_235B {
        display_name: "Qwen3 235B",
        bedrock_id: "qwen.qwen3-235b-a22b-2507-v1:0",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Qwen3 Coder 480B - Large coding-focused MoE model
    Qwen3Coder480B {
        display_name: "Qwen3 Coder 480B",
        bedrock_id: "qwen.qwen3-coder-480b-a35b-v1:0",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Qwen3 32B - Dense 32B model
    Qwen3_32B {
        display_name: "Qwen3 32B",
        bedrock_id: "qwen.qwen3-32b-v1:0",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Qwen3 Coder 30B - Compact coding-focused MoE model
    Qwen3Coder30B {
        display_name: "Qwen3 Coder 30B",
        bedrock_id: "qwen.qwen3-coder-30b-a3b-v1:0",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Qwen3 Next 80B - Next-gen MoE model
    Qwen3Next80B {
        display_name: "Qwen3 Next 80B",
        bedrock_id: "qwen.qwen3-next-80b-a3b",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Qwen3 VL 235B - Vision-language MoE model
    Qwen3VL235B {
        display_name: "Qwen3 VL 235B",
        bedrock_id: "qwen.qwen3-vl-235b-a22b",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);
