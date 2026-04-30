//! Anthropic Claude models

use super::define_model;
use crate::model::InferenceProfile;

define_model!(
    /// Claude 3.7 Sonnet - Latest Claude 3.x with improved reasoning
    Claude3_7Sonnet {
        display_name: "Claude 3.7 Sonnet",
        bedrock_id: "anthropic.claude-3-7-sonnet-20250219-v1:0",
        context_tokens: 200_000,
        output_tokens: 64_000,
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
    /// Claude Opus 4.1 - Advanced reasoning model
    ClaudeOpus4_1 {
        display_name: "Claude Opus 4.1",
        bedrock_id: "anthropic.claude-opus-4-1-20250805-v1:0",
        context_tokens: 200_000,
        output_tokens: 32_000,
        anthropic_id: "claude-opus-4-1-20250805",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Opus 4.5 - High-capability reasoning and creative writing model
    ClaudeOpus4_5 {
        display_name: "Claude Opus 4.5",
        bedrock_id: "anthropic.claude-opus-4-5-20251101-v1:0",
        context_tokens: 200_000,
        output_tokens: 64_000,
        anthropic_id: "claude-opus-4-5-20251101",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Opus 4.6 - Flagship Claude model with 128K output
    ClaudeOpus4_6 {
        display_name: "Claude Opus 4.6",
        bedrock_id: "anthropic.claude-opus-4-6-v1",
        context_tokens: 200_000,
        output_tokens: 128_000,
        anthropic_id: "claude-opus-4-6",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Opus 4.7 - Next-generation flagship Claude model
    ClaudeOpus4_7 {
        display_name: "Claude Opus 4.7",
        bedrock_id: "anthropic.claude-opus-4-7-v1",
        context_tokens: 200_000,
        output_tokens: 128_000,
        anthropic_id: "claude-opus-4-7",
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Claude Mythos Preview - Preview model with advanced capabilities
    ClaudeMythosPreview {
        display_name: "Claude Mythos Preview",
        bedrock_id: "anthropic.claude-mythos-preview",
        context_tokens: 200_000,
        output_tokens: 128_000,
        anthropic_id: "claude-mythos-preview",
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
    /// Claude Sonnet 4.6 - Most capable Sonnet with 1M context beta
    ClaudeSonnet4_6 {
        display_name: "Claude Sonnet 4.6",
        bedrock_id: "anthropic.claude-sonnet-4-6",
        context_tokens: 200_000,
        output_tokens: 64_000,
        anthropic_id: "claude-sonnet-4-6",
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
        output_tokens: 64_000,
        anthropic_id: "claude-haiku-4-5-20251001",
        default_inference_profile: InferenceProfile::Global
    }
);
