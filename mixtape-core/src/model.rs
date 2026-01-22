//! Model traits and types
//!
//! This module defines the core model abstraction:
//! - `Model` trait for model metadata (name, token limits)
//! - Provider-specific traits (`BedrockModel`, `AnthropicModel`) for API IDs
//!
//! Models are simple structs that implement these traits. All API interaction
//! goes through the provider (e.g., `BedrockProvider`).

use crate::events::TokenUsage;
use crate::types::{ContentBlock, Message, StopReason, ToolDefinition};

/// Request parameters for model completion
#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub max_tokens: i32,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub tools: Vec<ToolDefinition>,
}

/// Response from a model completion
#[derive(Debug, Clone)]
pub struct ModelResponse {
    /// The assistant's response message
    pub message: Message,
    /// Why the model stopped generating
    pub stop_reason: StopReason,
    /// Token usage statistics (if provided by the model)
    pub usage: Option<TokenUsage>,
}

/// Core model metadata trait
///
/// All models implement this to provide their capabilities.
/// This is provider-agnostic - the same model has the same
/// context window whether accessed via Bedrock or Anthropic.
pub trait Model: Send + Sync {
    /// Human-readable model name (e.g., "Claude Sonnet 4.5")
    fn name(&self) -> &'static str;

    /// Maximum input context tokens
    fn max_context_tokens(&self) -> usize;

    /// Maximum output tokens the model can generate
    fn max_output_tokens(&self) -> usize;

    /// Estimate token count for text
    ///
    /// Models should implement this to provide accurate token estimation.
    /// A simple heuristic (~4 characters per token) works reasonably well
    /// for most models but can be overridden with actual tokenization.
    fn estimate_token_count(&self, text: &str) -> usize;

    /// Estimate tokens for a conversation
    ///
    /// Default implementation sums token estimates for all content blocks
    /// plus overhead for message structure.
    fn estimate_message_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for message in messages {
            // Role overhead (~4 tokens for role marker and structure)
            total += 4;
            // Content blocks
            for block in &message.content {
                total += self.estimate_content_block_tokens(block);
            }
        }
        total
    }

    /// Estimate tokens for a single content block
    fn estimate_content_block_tokens(&self, block: &ContentBlock) -> usize {
        match block {
            ContentBlock::Text(text) => self.estimate_token_count(text),
            ContentBlock::ToolUse(tool_use) => {
                // Tool name + ID + JSON input
                self.estimate_token_count(&tool_use.name)
                    + self.estimate_token_count(&tool_use.id)
                    + self.estimate_token_count(&tool_use.input.to_string())
                    + 10 // Structure overhead
            }
            ContentBlock::ToolResult(result) => {
                // Tool use ID + content
                self.estimate_token_count(&result.tool_use_id)
                    + match &result.content {
                        crate::tool::ToolResult::Text(t) => self.estimate_token_count(t.as_str()),
                        crate::tool::ToolResult::Json(v) => {
                            self.estimate_token_count(&v.to_string())
                        }
                        crate::tool::ToolResult::Image { data, .. } => {
                            // Images are typically ~1 token per 750 bytes
                            data.len() / 750 + 85 // Base overhead for image
                        }
                        crate::tool::ToolResult::Document { data, .. } => {
                            // Documents vary; rough estimate
                            data.len() / 500 + 50 // Base overhead for document
                        }
                    }
                    + 10 // Structure overhead
            }
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                // Estimate tokens for thinking content
                self.estimate_token_count(thinking) + self.estimate_token_count(signature) + 10
            }
            ContentBlock::ServerToolUse(server_use) => {
                // Server tool use (informational)
                self.estimate_token_count(&server_use.name)
                    + self.estimate_token_count(&server_use.id)
                    + self.estimate_token_count(&server_use.input.to_string())
                    + 10
            }
            ContentBlock::ToolSearchResult(result) => {
                // Tool search result (informational)
                self.estimate_token_count(&result.tool_use_id)
                    + result.tool_references.len() * 10
                    + 10
            }
        }
    }
}

/// Cross-region inference profile configuration for Bedrock
///
/// Inference profiles enable cross-region load balancing for higher throughput
/// and improved reliability. When enabled, Bedrock automatically routes requests
/// to the optimal region within the specified geographic scope.
///
/// Some newer models (Claude 4/4.5, Nova 2 Lite) require inference profiles
/// and don't support direct single-region invocation.
///
/// See: <https://docs.aws.amazon.com/bedrock/latest/userguide/cross-region-inference.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InferenceProfile {
    /// No inference profile - single-region invocation (default)
    ///
    /// Requests go directly to the region configured in your AWS SDK.
    /// Use this for predictable routing and when data locality is important.
    #[default]
    None,

    /// US regions only (us-east-1, us-east-2, us-west-2, etc.)
    US,

    /// European regions only (eu-central-1, eu-west-1, eu-west-2, etc.)
    EU,

    /// Asia-Pacific regions (ap-northeast-1, ap-southeast-1, etc.)
    APAC,

    /// Global cross-region inference (all commercial AWS regions)
    ///
    /// Provides maximum throughput but may route to any region worldwide.
    Global,
}

impl InferenceProfile {
    /// Apply this inference profile to a base model ID
    ///
    /// Returns the full model ID to use with Bedrock API.
    pub fn apply_to(&self, base_model_id: &str) -> String {
        match self.prefix() {
            Some(prefix) => format!("{}.{}", prefix, base_model_id),
            None => base_model_id.to_string(),
        }
    }

    /// Get the prefix for this inference profile, if any
    fn prefix(&self) -> Option<&'static str> {
        match self {
            InferenceProfile::None => None,
            InferenceProfile::US => Some("us"),
            InferenceProfile::EU => Some("eu"),
            InferenceProfile::APAC => Some("apac"),
            InferenceProfile::Global => Some("global"),
        }
    }
}

/// Trait for models available on AWS Bedrock
///
/// Models implement this to be usable with `BedrockProvider`.
pub trait BedrockModel: Model {
    /// The Bedrock model ID
    ///
    /// This is the full model identifier used in Bedrock API calls,
    /// e.g., "anthropic.claude-sonnet-4-5-20250929-v1:0"
    fn bedrock_id(&self) -> &'static str;

    /// The default inference profile for this model
    ///
    /// Models that require cross-region inference (Claude 4/4.5, Nova 2 Lite)
    /// should return `InferenceProfile::Global`. Other models default to
    /// `InferenceProfile::None` for single-region invocation.
    fn default_inference_profile(&self) -> InferenceProfile {
        InferenceProfile::None
    }
}

/// Trait for models available via Anthropic's direct API
///
/// Models implement this to be usable with a future `AnthropicProvider`.
pub trait AnthropicModel: Model {
    /// The Anthropic API model ID
    ///
    /// e.g., "claude-sonnet-4-5-20250929"
    fn anthropic_id(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{DocumentFormat, ImageFormat, ToolResult};
    use crate::types::{
        ContentBlock, Message, Role, ToolResultBlock, ToolResultStatus, ToolUseBlock,
    };

    /// Simple test model with predictable token estimation
    struct TestModel;

    impl Model for TestModel {
        fn name(&self) -> &'static str {
            "TestModel"
        }

        fn max_context_tokens(&self) -> usize {
            100_000
        }

        fn max_output_tokens(&self) -> usize {
            4096
        }

        fn estimate_token_count(&self, text: &str) -> usize {
            // Simple: ~4 chars per token, rounding up
            text.len().div_ceil(4)
        }
    }

    // ===== Token Estimation Tests =====

    #[test]
    fn test_estimate_message_tokens_empty() {
        let model = TestModel;
        let messages: Vec<Message> = vec![];
        assert_eq!(model.estimate_message_tokens(&messages), 0);
    }

    #[test]
    fn test_estimate_message_tokens_simple_text() {
        let model = TestModel;
        let messages = vec![Message::user("Hello world")]; // 11 chars = 3 tokens + 4 overhead = 7

        let tokens = model.estimate_message_tokens(&messages);
        assert_eq!(tokens, 7);
    }

    #[test]
    fn test_estimate_message_tokens_multiple_messages() {
        let model = TestModel;
        let messages = vec![
            Message::user("Hello"),         // 5 chars = 2 tokens + 4 overhead = 6
            Message::assistant("Hi there"), // 8 chars = 2 tokens + 4 overhead = 6
        ];

        let tokens = model.estimate_message_tokens(&messages);
        assert_eq!(tokens, 12);
    }

    #[test]
    fn test_estimate_content_block_tokens_text() {
        let model = TestModel;
        let block = ContentBlock::Text("test".to_string()); // 4 chars = 1 token
        assert_eq!(model.estimate_content_block_tokens(&block), 1);
    }

    #[test]
    fn test_estimate_content_block_tokens_text_empty() {
        let model = TestModel;
        let block = ContentBlock::Text(String::new());
        assert_eq!(model.estimate_content_block_tokens(&block), 0);
    }

    #[test]
    fn test_estimate_content_block_tokens_tool_use() {
        let model = TestModel;
        let block = ContentBlock::ToolUse(ToolUseBlock {
            id: "id12".to_string(),               // 4 chars = 1 token
            name: "search".to_string(),           // 6 chars = 2 tokens
            input: serde_json::json!({"q": "x"}), // ~10 chars = 3 tokens
        });

        // 1 + 2 + 3 + 10 (overhead) = 16
        let tokens = model.estimate_content_block_tokens(&block);
        assert!(tokens >= 10, "Should include overhead, got {}", tokens);
    }

    #[test]
    fn test_estimate_content_block_tokens_tool_result_text() {
        let model = TestModel;
        let block = ContentBlock::ToolResult(ToolResultBlock {
            tool_use_id: "id12".to_string(), // 4 chars = 1 token
            content: ToolResult::Text("result text".to_string()), // 11 chars = 3 tokens
            status: ToolResultStatus::Success,
        });

        // 1 + 3 + 10 (overhead) = 14
        let tokens = model.estimate_content_block_tokens(&block);
        assert!(tokens >= 10, "Should include overhead, got {}", tokens);
    }

    #[test]
    fn test_estimate_content_block_tokens_tool_result_json() {
        let model = TestModel;
        let block = ContentBlock::ToolResult(ToolResultBlock {
            tool_use_id: "id".to_string(),
            content: ToolResult::Json(serde_json::json!({"key": "value"})),
            status: ToolResultStatus::Success,
        });

        let tokens = model.estimate_content_block_tokens(&block);
        assert!(tokens >= 10, "Should include overhead, got {}", tokens);
    }

    #[test]
    fn test_estimate_content_block_tokens_image() {
        let model = TestModel;
        // 7500 bytes / 750 + 85 = 95 tokens
        let data = vec![0u8; 7500];
        let block = ContentBlock::ToolResult(ToolResultBlock {
            tool_use_id: "img".to_string(),
            content: ToolResult::Image {
                format: ImageFormat::Png,
                data,
            },
            status: ToolResultStatus::Success,
        });

        let tokens = model.estimate_content_block_tokens(&block);
        // 7500/750 + 85 = 10 + 85 = 95 + tool_use_id tokens + overhead
        assert!(
            tokens >= 95,
            "Expected at least 95 tokens for image, got {}",
            tokens
        );
    }

    #[test]
    fn test_estimate_content_block_tokens_document() {
        let model = TestModel;
        // 5000 bytes / 500 + 50 = 60 tokens
        let data = vec![0u8; 5000];
        let block = ContentBlock::ToolResult(ToolResultBlock {
            tool_use_id: "doc".to_string(),
            content: ToolResult::Document {
                format: DocumentFormat::Pdf,
                data,
                name: Some("test.pdf".to_string()),
            },
            status: ToolResultStatus::Success,
        });

        let tokens = model.estimate_content_block_tokens(&block);
        // 5000/500 + 50 = 10 + 50 = 60 + overhead
        assert!(
            tokens >= 60,
            "Expected at least 60 tokens for document, got {}",
            tokens
        );
    }

    #[test]
    fn test_estimate_content_block_tokens_thinking() {
        let model = TestModel;
        let block = ContentBlock::Thinking {
            thinking: "complex reasoning here".to_string(), // 22 chars = 6 tokens
            signature: "sig".to_string(),                   // 3 chars = 1 token
        };

        // 6 + 1 + 10 (overhead) = 17
        let tokens = model.estimate_content_block_tokens(&block);
        assert!(tokens >= 10, "Should include overhead, got {}", tokens);
    }

    #[test]
    fn test_estimate_message_with_multiple_content_blocks() {
        let model = TestModel;
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text("Let me search".to_string()),
                ContentBlock::ToolUse(ToolUseBlock {
                    id: "1".to_string(),
                    name: "search".to_string(),
                    input: serde_json::json!({"q": "test"}),
                }),
            ],
        }];

        let tokens = model.estimate_message_tokens(&messages);
        // 4 (overhead) + text tokens + tool use tokens
        assert!(tokens > 4, "Should have content tokens plus overhead");
    }

    // ===== InferenceProfile Tests =====

    #[test]
    fn test_inference_profile_apply_none() {
        let profile = InferenceProfile::None;
        assert_eq!(profile.apply_to("anthropic.claude-3"), "anthropic.claude-3");
    }

    #[test]
    fn test_inference_profile_apply_us() {
        let profile = InferenceProfile::US;
        assert_eq!(
            profile.apply_to("anthropic.claude-3"),
            "us.anthropic.claude-3"
        );
    }

    #[test]
    fn test_inference_profile_apply_eu() {
        let profile = InferenceProfile::EU;
        assert_eq!(
            profile.apply_to("anthropic.claude-3"),
            "eu.anthropic.claude-3"
        );
    }

    #[test]
    fn test_inference_profile_apply_apac() {
        let profile = InferenceProfile::APAC;
        assert_eq!(profile.apply_to("model-id"), "apac.model-id");
    }

    #[test]
    fn test_inference_profile_apply_global() {
        let profile = InferenceProfile::Global;
        assert_eq!(profile.apply_to("model-id"), "global.model-id");
    }

    #[test]
    fn test_inference_profile_all_variants() {
        let cases = [
            (InferenceProfile::None, "model", "model"),
            (InferenceProfile::US, "model", "us.model"),
            (InferenceProfile::EU, "model", "eu.model"),
            (InferenceProfile::APAC, "model", "apac.model"),
            (InferenceProfile::Global, "model", "global.model"),
        ];

        for (profile, base, expected) in cases {
            assert_eq!(profile.apply_to(base), expected, "Failed for {:?}", profile);
        }
    }

    #[test]
    fn test_inference_profile_default() {
        let profile = InferenceProfile::default();
        assert_eq!(profile, InferenceProfile::None);
    }
}
