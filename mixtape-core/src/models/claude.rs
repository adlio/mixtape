//! Anthropic Claude models

use super::define_model;
use crate::model::InferenceProfile;

// =============================================================================
// Claude 3 Models
// =============================================================================

define_model!(
    /// Claude 3 Haiku - Fast, compact model for quick responses
    Claude3Haiku {
        display_name: "Claude 3 Haiku",
        bedrock_id: "anthropic.claude-3-haiku-20240307-v1:0",
        context_tokens: 200_000,
        output_tokens: 4_096,
        anthropic_id: "claude-3-haiku-20240307"
    }
);

define_model!(
    /// Claude 3 Opus - Most capable Claude 3 model
    Claude3Opus {
        display_name: "Claude 3 Opus",
        bedrock_id: "anthropic.claude-3-opus-20240229-v1:0",
        context_tokens: 200_000,
        output_tokens: 4_096,
        anthropic_id: "claude-3-opus-20240229"
    }
);

define_model!(
    /// Claude 3 Sonnet - Balanced Claude 3 model
    Claude3Sonnet {
        display_name: "Claude 3 Sonnet",
        bedrock_id: "anthropic.claude-3-sonnet-20240229-v1:0",
        context_tokens: 200_000,
        output_tokens: 4_096,
        anthropic_id: "claude-3-sonnet-20240229"
    }
);

// =============================================================================
// Claude 3.5 Models
// =============================================================================

define_model!(
    /// Claude 3.5 Haiku - Fast, efficient model
    Claude3_5Haiku {
        display_name: "Claude 3.5 Haiku",
        bedrock_id: "anthropic.claude-3-5-haiku-20241022-v1:0",
        context_tokens: 200_000,
        output_tokens: 8_192,
        anthropic_id: "claude-3-5-haiku-20241022"
    }
);

define_model!(
    /// Claude 3.5 Sonnet v1 - First Claude 3.5 Sonnet release
    Claude3_5SonnetV1 {
        display_name: "Claude 3.5 Sonnet v1",
        bedrock_id: "anthropic.claude-3-5-sonnet-20240620-v1:0",
        context_tokens: 200_000,
        output_tokens: 8_192,
        anthropic_id: "claude-3-5-sonnet-20240620"
    }
);

define_model!(
    /// Claude 3.5 Sonnet v2 - Updated Claude 3.5 Sonnet with improvements
    Claude3_5SonnetV2 {
        display_name: "Claude 3.5 Sonnet v2",
        bedrock_id: "anthropic.claude-3-5-sonnet-20241022-v2:0",
        context_tokens: 200_000,
        output_tokens: 8_192,
        anthropic_id: "claude-3-5-sonnet-20241022"
    }
);

// =============================================================================
// Claude 3.7 Models
// =============================================================================

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
