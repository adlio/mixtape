//! Pre-configured model definitions
//!
//! This module contains model structs for various LLM providers.
//! Each model implements the `Model` trait and provider-specific traits
//! like `BedrockModel` or `AnthropicModel`.
//!
//! Models are organized by vendor:
//! - `ai21` - AI21 Labs Jamba models
//! - `claude` - Anthropic Claude models
//! - `cohere` - Cohere models
//! - `deepseek` - DeepSeek models
//! - `glm` - Z.AI GLM models
//! - `google` - Google models
//! - `kimi` - Moonshot Kimi models
//! - `llama` - Meta Llama models
//! - `minimax` - MiniMax models
//! - `mistral` - Mistral AI models
//! - `nova` - Amazon Nova models
//! - `nvidia` - NVIDIA Nemotron models
//! - `openai` - OpenAI GPT-OSS models
//! - `qwen` - Alibaba Qwen models
//! - `titan` - Amazon Titan Text models
//! - `writer` - Writer AI Palmyra models

mod ai21;
mod claude;
mod cohere;
mod deepseek;
mod glm;
mod google;
mod kimi;
mod llama;
mod minimax;
mod mistral;
mod nova;
mod nvidia;
mod openai;
mod qwen;
mod titan;
mod writer;

// Re-export all models at the module level
pub use ai21::*;
pub use claude::*;
pub use cohere::*;
pub use deepseek::*;
pub use glm::*;
pub use google::*;
pub use kimi::*;
pub use llama::*;
pub use minimax::*;
pub use mistral::*;
pub use nova::*;
pub use nvidia::*;
pub use openai::*;
pub use qwen::*;
pub use titan::*;
pub use writer::*;

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
    use crate::model::{AnthropicModel, BedrockModel, InferenceProfile, Model};

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
            // Anthropic Claude
            &Claude3Haiku,
            &Claude3Opus,
            &Claude3Sonnet,
            &Claude3_5Haiku,
            &Claude3_5SonnetV1,
            &Claude3_5SonnetV2,
            &Claude3_7Sonnet,
            &ClaudeOpus4,
            &ClaudeSonnet4,
            &ClaudeSonnet4_5,
            &ClaudeSonnet4_6,
            &ClaudeHaiku4_5,
            &ClaudeOpus4_5,
            &ClaudeOpus4_1,
            &ClaudeOpus4_6,
            // Amazon Nova
            &NovaMicro,
            &NovaLite,
            &Nova2Lite,
            &NovaPro,
            &NovaPremier,
            &Nova2Sonic,
            // Amazon Titan
            &TitanTextExpress,
            &TitanTextLite,
            &TitanTextPremier,
            // Mistral
            &MistralLarge2,
            &MistralSmall,
            &MistralLarge3,
            &MagistralSmall,
            &Ministral3B,
            &Ministral8B,
            &Ministral14B,
            &PixtralLarge,
            &VoxtralMini3B,
            &VoxtralSmall24B,
            &Devstral2_135B,
            // Cohere
            &CohereCommandR,
            &CohereCommandRPlus,
            &CohereCommandRPlusV2,
            // Qwen
            &Qwen3_235B,
            &Qwen3Coder480B,
            &Qwen3_32B,
            &Qwen3Coder30B,
            &Qwen3Next80B,
            &Qwen3VL235B,
            &Qwen3CoderNext,
            // NVIDIA
            &NemotronNano2,
            &NemotronNano2VL,
            &Nemotron3Nano30BA3B,
            &Nemotron3Super120BA12B,
            // OpenAI
            &GptOss20B,
            &GptOss120B,
            &GptOssSafeguard20B,
            &GptOssSafeguard120B,
            // Z.AI GLM
            &GLM4_7,
            &GLM4_7Flash,
            &GLM5,
            // Google
            &Gemma3_27B,
            &Gemma3_12B,
            &Gemma3_4B,
            // DeepSeek
            &DeepSeekR1,
            &DeepSeekV3_1,
            &DeepSeekV3_2,
            // Moonshot Kimi
            &KimiK2Thinking,
            &KimiK2_5,
            // MiniMax
            &MiniMaxM2_1,
            &MiniMaxM2,
            &MiniMaxM2_5,
            // Meta Llama
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
            &Llama3_70B,
            &Llama3_8B,
            // Writer
            &WriterPalmyraX4,
            &WriterPalmyraX5,
            &WriterPalmyraVision7B,
            // AI21
            &AI21Jamba1_5Large,
            &AI21Jamba1_5Mini,
            &AI21JambaInstruct,
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

    #[test]
    fn test_global_inference_profile_models() {
        // Models that require Global inference profile should return it
        let global_models: Vec<&dyn BedrockModel> = vec![
            &ClaudeOpus4,
            &ClaudeOpus4_1,
            &ClaudeOpus4_5,
            &ClaudeOpus4_6,
            &ClaudeSonnet4,
            &ClaudeSonnet4_5,
            &ClaudeSonnet4_6,
            &ClaudeHaiku4_5,
            &Nova2Lite,
            &Nova2Sonic,
        ];

        for model in global_models {
            assert_eq!(
                model.default_inference_profile(),
                InferenceProfile::Global,
                "{} should have Global inference profile",
                model.bedrock_id()
            );
        }
    }

    #[test]
    fn test_default_inference_profile_models() {
        // Models without an explicit profile should return None (the default)
        let default_models: Vec<&dyn BedrockModel> = vec![
            &Claude3Haiku,
            &Claude3Opus,
            &Claude3Sonnet,
            &Claude3_5Haiku,
            &Claude3_5SonnetV1,
            &Claude3_5SonnetV2,
            &Claude3_7Sonnet,
            &NovaMicro,
            &NovaLite,
            &NovaPro,
            &NovaPremier,
            &TitanTextExpress,
            &TitanTextLite,
            &TitanTextPremier,
            &MistralLarge2,
            &MistralSmall,
            &MistralLarge3,
            &Gemma3_27B,
            &DeepSeekR1,
            &KimiK2Thinking,
            &CohereCommandR,
            &CohereCommandRPlus,
            &CohereCommandRPlusV2,
            &WriterPalmyraX4,
            &WriterPalmyraX5,
            &AI21Jamba1_5Large,
            &AI21Jamba1_5Mini,
            &AI21JambaInstruct,
            &Llama3_70B,
            &Llama3_8B,
        ];

        for model in default_models {
            assert_eq!(
                model.default_inference_profile(),
                InferenceProfile::None,
                "{} should have None (default) inference profile",
                model.bedrock_id()
            );
        }
    }
}
