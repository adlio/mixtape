//! Mistral AI models

use super::define_model;

define_model!(
    /// Mistral Large 3 - Flagship 675B MoE model with 41B active parameters
    MistralLarge3 {
        display_name: "Mistral Large 3",
        bedrock_id: "mistral.mistral-large-3-675b-instruct",
        context_tokens: 256_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Magistral Small - Efficient 24B reasoning model with vision
    MagistralSmall {
        display_name: "Magistral Small",
        bedrock_id: "mistral.magistral-small-2509",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
