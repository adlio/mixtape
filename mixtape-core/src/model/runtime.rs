//! Runtime-owned Bedrock model configuration.

use super::{BedrockModel, Model};
use crate::provider::ProviderError;

/// A model selected at runtime, including an exact inference-profile ID or ARN.
///
/// Token ceilings are supplied by the caller; this type does not discover account
/// access, service limits, or model capabilities. Constructing it performs no I/O.
/// Unlike some historical typed models, it never opts into global routing.
#[derive(Debug, Clone)]
pub struct RuntimeBedrockModel {
    name: String,
    model_id: String,
    context_tokens: usize,
    output_tokens: usize,
}

impl RuntimeBedrockModel {
    /// Build an owned configuration without leaking strings or selecting a route.
    pub fn new(
        name: impl Into<String>,
        model_id: impl Into<String>,
        context_tokens: usize,
        output_tokens: usize,
    ) -> Result<Self, ProviderError> {
        let name = name.into();
        let model_id = model_id.into();
        if name.trim().is_empty() {
            return Err(ProviderError::Configuration("Model name is empty".into()));
        }
        // IDs and ARNs are identifiers, not URLs. Bedrock remains authoritative
        // for account access and supported routes; do not guess those here.
        if model_id.is_empty()
            || model_id.contains("://")
            || (model_id.starts_with("arn:") && !model_id.contains(":bedrock:"))
            || !model_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(&c))
        {
            return Err(ProviderError::Configuration(
                "Expected a Bedrock model ID, inference-profile ID, or ARN".into(),
            ));
        }
        if context_tokens == 0 || output_tokens == 0 || output_tokens > i32::MAX as usize {
            return Err(ProviderError::Configuration(
                "Token ceilings must be positive; output must fit Bedrock's i32 maxTokens".into(),
            ));
        }
        Ok(Self {
            name,
            model_id,
            context_tokens,
            output_tokens,
        })
    }
}

impl Model for RuntimeBedrockModel {
    fn name(&self) -> &str {
        &self.name
    }

    fn max_context_tokens(&self) -> usize {
        self.context_tokens
    }

    fn max_output_tokens(&self) -> usize {
        self.output_tokens
    }

    fn estimate_token_count(&self, text: &str) -> usize {
        text.len().div_ceil(4)
    }
}

impl BedrockModel for RuntimeBedrockModel {
    fn bedrock_id(&self) -> &str {
        &self.model_id
    }
}
