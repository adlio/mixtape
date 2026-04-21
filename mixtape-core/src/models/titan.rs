//! Amazon Titan Text models

use super::define_model;

define_model!(
    /// Titan Text Express - Fast general-purpose text model
    TitanTextExpress {
        display_name: "Titan Text Express",
        bedrock_id: "amazon.titan-text-express-v1",
        context_tokens: 8_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Titan Text Lite - Lightweight text model for simple tasks
    TitanTextLite {
        display_name: "Titan Text Lite",
        bedrock_id: "amazon.titan-text-lite-v1",
        context_tokens: 4_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Titan Text Premier - High-capability text model
    TitanTextPremier {
        display_name: "Titan Text Premier",
        bedrock_id: "amazon.titan-text-premier-v1:0",
        context_tokens: 32_000,
        output_tokens: 8_192
    }
);
