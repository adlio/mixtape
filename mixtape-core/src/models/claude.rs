//! Anthropic Claude models

use super::define_model;
use crate::model::InferenceProfile;

define_model!(
    /// Claude 3.7 Sonnet - Latest Claude 3.x with improved reasoning
    Claude3_7Sonnet {
        display_name: "Claude 3.7 Sonnet",
        bedrock_id: "anthropic.claude-3-7-sonnet-20250219-v1:0",
        context_tokens: 200_000,
        output_tokens: 8_192,
        anthropic_id: "claude-3-7-sonnet-20250219"
    }
);

define_model!(
    /// Claude Opus 4 - High capability reasoning model
    ClaudeOpus4 {
        display_name: "Claude Opus 4",
        bedrock_id: "anthropic.claude-opus-4-20250514-v1:0",
        context_tokens: 200_000,
        output_tokens: 32_000,
        anthropic_id: "claude-opus-4-20250514",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Sonnet 4 - Balanced performance and cost
    ClaudeSonnet4 {
        display_name: "Claude Sonnet 4",
        bedrock_id: "anthropic.claude-sonnet-4-20250514-v1:0",
        context_tokens: 200_000,
        output_tokens: 64_000,
        anthropic_id: "claude-sonnet-4-20250514",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Sonnet 4.5 - Latest Sonnet with improved capabilities
    ClaudeSonnet4_5 {
        display_name: "Claude Sonnet 4.5",
        bedrock_id: "anthropic.claude-sonnet-4-5-20250929-v1:0",
        context_tokens: 200_000,
        output_tokens: 64_000,
        anthropic_id: "claude-sonnet-4-5-20250929",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Haiku 4.5 - Fast, efficient model for high-throughput tasks
    ClaudeHaiku4_5 {
        display_name: "Claude Haiku 4.5",
        bedrock_id: "anthropic.claude-haiku-4-5-20251001-v1:0",
        context_tokens: 200_000,
        output_tokens: 8_192,
        anthropic_id: "claude-haiku-4-5-20251001",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Opus 4.5 - Most capable Claude model
    ClaudeOpus4_5 {
        display_name: "Claude Opus 4.5",
        bedrock_id: "anthropic.claude-opus-4-5-20251101-v1:0",
        context_tokens: 200_000,
        output_tokens: 32_000,
        anthropic_id: "claude-opus-4-5-20251101",
        default_inference_profile: InferenceProfile::Global
    }
);
