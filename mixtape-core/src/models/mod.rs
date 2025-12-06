//! Pre-configured model definitions
//!
//! This module contains model structs for various LLM providers.
//! Each model implements the `Model` trait and provider-specific traits
//! like `BedrockModel` or `AnthropicModel`.
//!
//! Models are organized by vendor:
//! - `claude` - Anthropic Claude models
//! - `llama` - Meta Llama models
//! - `nova` - Amazon Nova models
//! - `mistral` - Mistral AI models
//! - `cohere` - Cohere models
//! - `qwen` - Alibaba Qwen models
//! - `google` - Google models
//! - `deepseek` - DeepSeek models
//! - `kimi` - Moonshot Kimi models

mod claude;
mod cohere;
mod deepseek;
mod google;
mod kimi;
mod llama;
mod mistral;
mod nova;
mod qwen;

// Re-export all models at the module level
pub use claude::*;
pub use cohere::*;
pub use deepseek::*;
pub use google::*;
pub use kimi::*;
pub use llama::*;
pub use mistral::*;
pub use nova::*;
pub use qwen::*;

/// Macro to generate model structs with trait implementations
///
/// This macro creates a model struct that implements:
/// - `Model` trait (always)
/// - `BedrockModel` trait (always)
/// - `AnthropicModel` trait (if `anthropic_id` is provided)
///
/// Optional fields:
/// - `anthropic_id` - Anthropic API model ID (enables AnthropicModel trait)
/// - `default_inference_profile` - Default inference profile for Bedrock (e.g., Global)
macro_rules! define_model {
    (
        $(#[$meta:meta])*
        $name:ident {
            display_name: $display_name:expr,
            bedrock_id: $bedrock_id:expr,
            context_tokens: $context_tokens:expr,
            output_tokens: $output_tokens:expr
            $(, anthropic_id: $anthropic_id:expr)?
            $(, default_inference_profile: $profile:expr)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name;

        impl $crate::model::Model for $name {
            fn name(&self) -> &'static str {
                $display_name
            }

            fn max_context_tokens(&self) -> usize {
                $context_tokens
            }

            fn max_output_tokens(&self) -> usize {
                $output_tokens
            }

            fn estimate_token_count(&self, text: &str) -> usize {
                // Default heuristic: ~4 characters per token
                text.len().div_ceil(4)
            }
        }

        impl $crate::model::BedrockModel for $name {
            fn bedrock_id(&self) -> &'static str {
                $bedrock_id
            }

            $crate::models::define_model!(@inference_profile $($profile)?);
        }

        $(
            impl $crate::model::AnthropicModel for $name {
                fn anthropic_id(&self) -> &'static str {
                    $anthropic_id
                }
            }
        )?
    };

    // Helper: generate default_inference_profile method if profile is specified
    (@inference_profile $profile:expr) => {
        fn default_inference_profile(&self) -> $crate::model::InferenceProfile {
            $profile
        }
    };

    // Helper: no-op if no profile specified (uses trait default)
    (@inference_profile) => {};
}

// Make the macro available to submodules
pub(crate) use define_model;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnthropicModel, BedrockModel, Model};

    #[test]
    fn test_claude_implements_both_traits() {
        let model = ClaudeSonnet4_5;

        // Model trait
        assert_eq!(model.name(), "Claude Sonnet 4.5");
        assert_eq!(model.max_context_tokens(), 200_000);
        assert_eq!(model.max_output_tokens(), 64_000);

        // BedrockModel trait
        assert!(model.bedrock_id().contains("claude-sonnet-4-5"));

        // AnthropicModel trait
        assert!(model.anthropic_id().contains("claude-sonnet-4-5"));
    }

    #[test]
    fn test_nova_only_implements_bedrock() {
        let model = NovaMicro;

        // Model trait
        assert_eq!(model.name(), "Nova Micro");

        // BedrockModel trait
        assert!(model.bedrock_id().contains("nova-micro"));

        // NovaMicro does NOT implement AnthropicModel - compile-time check
    }

    #[test]
    fn test_models_are_copy() {
        let model = ClaudeSonnet4_5;
        let copy = model;
        assert_eq!(model.name(), copy.name());
    }

    #[test]
    fn test_model_ids_are_valid() {
        // Verify model ID format (no spaces, valid characters)
        let models: Vec<&dyn BedrockModel> = vec![
            &Claude3_7Sonnet,
            &ClaudeOpus4,
            &ClaudeSonnet4,
            &ClaudeSonnet4_5,
            &ClaudeHaiku4_5,
            &ClaudeOpus4_5,
            &NovaMicro,
            &NovaLite,
            &Nova2Lite,
            &NovaPro,
            &NovaPremier,
            &MistralLarge3,
            &MagistralSmall,
            &CohereCommandRPlus,
            &Qwen3_235B,
            &Qwen3Coder480B,
            &Gemma3_27B,
            &DeepSeekR1,
            &DeepSeekV3,
            &KimiK2Thinking,
            &Llama4Scout17B,
            &Llama4Maverick17B,
            &Llama3_3_70B,
            &Llama3_2_90B,
            &Llama3_2_11B,
            &Llama3_2_3B,
            &Llama3_2_1B,
            &Llama3_1_405B,
            &Llama3_1_70B,
            &Llama3_1_8B,
        ];

        for model in models {
            let id = model.bedrock_id();
            assert!(
                !id.contains(' '),
                "Model ID should not contain spaces: {}",
                id
            );
            assert!(
                id.contains('.'),
                "Model ID should contain provider prefix: {}",
                id
            );
        }
    }
}
