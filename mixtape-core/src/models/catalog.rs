//! Bedrock model descriptors for configuration-driven applications.
//!
//! Reviewed against AWS model cards on 2026-09-22. A catalog entry is not proof
//! of account access or a tested multi-turn tool loop. Card limit labels are
//! retained verbatim; they are not silently converted into exact token counts.

use crate::model::RuntimeBedrockModel;
use crate::provider::ProviderError;
use serde::Serialize;

/// The API required for the intended multi-turn conversation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BedrockConversationApi {
    Converse,
    /// Requires a separate Bedrock Chat Completions adapter. Do not silently
    /// select Converse and discard reasoning to work around its limitations.
    ChatCompletions,
}

/// Documented model identity and limits, separate from executable configuration.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BedrockModelDescriptor {
    pub model_id: &'static str,
    pub display_name: &'static str,
    pub context_window_label: &'static str,
    pub max_output_label: Option<&'static str>,
    pub conversation_api: BedrockConversationApi,
    pub requires_inference_profile: bool,
    pub model_card: &'static str,
}

impl BedrockModelDescriptor {
    /// Select explicit application token ceilings. They must be verified against
    /// the chosen endpoint before use; a missing card limit is not unlimited.
    /// Use `BedrockProvider::with_inference_profile` or an exact runtime target
    /// for models requiring a profile. No credentials or requests are used here.
    pub fn with_token_limits(
        &self,
        context_tokens: usize,
        output_tokens: usize,
    ) -> Result<RuntimeBedrockModel, ProviderError> {
        RuntimeBedrockModel::new(
            self.display_name,
            self.model_id,
            context_tokens,
            output_tokens,
        )
    }
}

macro_rules! descriptor {
    ($id:literal, $name:literal, $context:literal, $output:expr, $api:ident, $profile:literal, $card:literal) => {
        BedrockModelDescriptor {
            model_id: $id,
            display_name: $name,
            context_window_label: $context,
            max_output_label: $output,
            conversation_api: BedrockConversationApi::$api,
            requires_inference_profile: $profile,
            model_card: concat!(
                "https://docs.aws.amazon.com/bedrock/latest/userguide/",
                $card,
                ".html"
            ),
        }
    };
}

/// The 22 mapped model candidates. Historical typed models remain available;
/// routers and private aliases without a verified Bedrock identity are omitted.
/// Library metadata does not establish permission to send any particular data.
pub const BEDROCK_MODEL_CATALOG: &[BedrockModelDescriptor] = &[
    descriptor!(
        "anthropic.claude-opus-5",
        "Claude Opus 5.0",
        "1M",
        Some("128K"),
        Converse,
        true,
        "model-card-anthropic-claude-opus-5"
    ),
    descriptor!(
        "anthropic.claude-sonnet-5",
        "Claude Sonnet 5",
        "1M",
        Some("128K"),
        Converse,
        true,
        "model-card-anthropic-claude-sonnet-5"
    ),
    descriptor!(
        "anthropic.claude-opus-4-8",
        "Claude Opus 4.8",
        "1M",
        Some("128K"),
        Converse,
        true,
        "model-card-anthropic-claude-opus-4-8"
    ),
    descriptor!(
        "anthropic.claude-opus-4-7",
        "Claude Opus 4.7",
        "1M",
        Some("128K"),
        Converse,
        true,
        "model-card-anthropic-claude-opus-4-7"
    ),
    descriptor!(
        "anthropic.claude-sonnet-4-6",
        "Claude Sonnet 4.6",
        "1M",
        Some("64K"),
        Converse,
        true,
        "model-card-anthropic-claude-sonnet-4-6"
    ),
    descriptor!(
        "anthropic.claude-opus-4-5-20251101-v1:0",
        "Claude Opus 4.5",
        "200K",
        Some("64K"),
        Converse,
        true,
        "model-card-anthropic-claude-opus-4-5"
    ),
    descriptor!(
        "anthropic.claude-sonnet-4-5-20250929-v1:0",
        "Claude Sonnet 4.5",
        "200K",
        Some("64K"),
        Converse,
        true,
        "model-card-anthropic-claude-sonnet-4-5"
    ),
    descriptor!(
        "anthropic.claude-sonnet-4-20250514-v1:0",
        "Claude Sonnet 4",
        "200K",
        Some("64K"),
        Converse,
        true,
        "model-card-anthropic-claude-sonnet-4"
    ),
    descriptor!(
        "anthropic.claude-haiku-4-5-20251001-v1:0",
        "Claude Haiku 4.5",
        "200K",
        Some("64K"),
        Converse,
        true,
        "model-card-anthropic-claude-haiku-4-5"
    ),
    // Restricted previews: callers must establish data/use-case eligibility.
    descriptor!(
        "anthropic.claude-fable-5",
        "Claude Fable 5",
        "1M",
        Some("128K"),
        Converse,
        true,
        "model-card-anthropic-claude-fable-5"
    ),
    descriptor!(
        "anthropic.claude-fable-5-1",
        "Claude Fable 5.1",
        "1M",
        Some("128K"),
        Converse,
        true,
        "model-card-anthropic-claude-fable-5-1"
    ),
    descriptor!(
        "openai.gpt-6-astra",
        "GPT-6 Astra",
        "1,050,000",
        Some("128,000"),
        Converse,
        true,
        "model-card-openai-gpt-6-astra"
    ),
    descriptor!(
        "openai.gpt-5.6-sol",
        "GPT-5.6 Sol",
        "1M",
        None,
        Converse,
        true,
        "model-card-openai-gpt-56-sol"
    ),
    descriptor!(
        "openai.gpt-5.6-terra",
        "GPT-5.6 Terra",
        "1M",
        None,
        Converse,
        true,
        "model-card-openai-gpt-56-terra"
    ),
    descriptor!(
        "openai.gpt-5.6-luna",
        "GPT-5.6 Luna",
        "1M",
        None,
        Converse,
        true,
        "model-card-openai-gpt-56-luna"
    ),
    descriptor!(
        "minimax.minimax-m2.5",
        "MiniMax M2.5",
        "196K",
        Some("8K"),
        Converse,
        false,
        "model-card-minimax-minimax-m2-5"
    ),
    descriptor!(
        "minimax.minimax-m2.1",
        "MiniMax M2.1",
        "196K",
        Some("8K"),
        Converse,
        false,
        "model-card-minimax-minimax-m2-1"
    ),
    descriptor!(
        "zai.glm-5",
        "GLM 5",
        "200K",
        Some("128K"),
        Converse,
        false,
        "model-card-zai-glm-5"
    ),
    descriptor!(
        "deepseek.v3.2",
        "DeepSeek V3.2",
        "164K",
        Some("8K"),
        Converse,
        false,
        "model-card-deepseek-deepseek-v3-2"
    ),
    descriptor!(
        "qwen.qwen3-coder-next",
        "Qwen3 Coder Next",
        "256K",
        Some("16K"),
        Converse,
        false,
        "model-card-qwen-qwen3-coder-next"
    ),
    descriptor!(
        "xai.grok-4.6",
        "Grok 4.6",
        "500K",
        None,
        Converse,
        true,
        "model-card-xai-grok-4-6"
    ),
    descriptor!(
        "moonshotai.kimi-k3",
        "Kimi K3",
        "1M",
        None,
        ChatCompletions,
        true,
        "model-card-moonshot-ai-kimi-k3"
    ),
];

/// Find a descriptor by base model ID or geographic inference-profile ID.
/// Exact application-profile ARNs need an account-side mapping; never infer one.
pub fn bedrock_model_descriptor(model_id: &str) -> Option<&'static BedrockModelDescriptor> {
    let base = ["us.", "eu.", "apac.", "au.", "in.", "jp.", "global."]
        .iter()
        .find_map(|prefix| model_id.strip_prefix(prefix))
        .unwrap_or(model_id);
    BEDROCK_MODEL_CATALOG
        .iter()
        .find(|model| model.model_id == base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BedrockModel, InferenceProfile, Model};
    use std::collections::HashSet;

    #[test]
    fn catalog_ids_are_unique_and_constructible_without_implicit_routing() {
        assert_eq!(BEDROCK_MODEL_CATALOG.len(), 22);
        let mut seen = HashSet::new();
        for descriptor in BEDROCK_MODEL_CATALOG {
            assert!(seen.insert(descriptor.model_id));
            let configured = descriptor.with_token_limits(128_000, 4096).unwrap();
            assert_eq!(configured.bedrock_id(), descriptor.model_id);
            assert_eq!(configured.name(), descriptor.display_name);
            assert_eq!(
                configured.default_inference_profile(),
                InferenceProfile::None
            );
            assert!(descriptor
                .model_card
                .starts_with("https://docs.aws.amazon.com/bedrock/"));
        }
    }

    #[test]
    fn unknown_limits_and_unmapped_aliases_stay_unknown() {
        assert!(bedrock_model_descriptor("openai.gpt-5.6-sol")
            .unwrap()
            .max_output_label
            .is_none());
        for alias in [
            "auto",
            "agi-nova-beta-1m",
            "kirin-inference-glm",
            "unknown.model",
        ] {
            assert!(bedrock_model_descriptor(alias).is_none());
        }
    }

    #[test]
    fn profile_lookup_does_not_substitute_models_or_infer_arns() {
        assert_eq!(
            bedrock_model_descriptor("us.anthropic.claude-opus-5")
                .unwrap()
                .display_name,
            "Claude Opus 5.0"
        );
        assert!(bedrock_model_descriptor(
            "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/example"
        )
        .is_none());
        assert_eq!(
            bedrock_model_descriptor("moonshotai.kimi-k3")
                .unwrap()
                .conversation_api,
            BedrockConversationApi::ChatCompletions
        );
    }
}
