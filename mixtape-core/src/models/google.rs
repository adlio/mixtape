//! Google models

use super::define_model;

define_model!(
    /// Gemma 3 27B - Open multimodal model from Google
    Gemma3_27B {
        display_name: "Gemma 3 27B",
        bedrock_id: "google.gemma-3-27b-it",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
