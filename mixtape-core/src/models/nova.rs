//! Amazon Nova models

use super::define_model;
use crate::model::InferenceProfile;

define_model!(
    /// Nova Micro - Lightweight, text-only model for simple tasks
    NovaMicro {
        display_name: "Nova Micro",
        bedrock_id: "amazon.nova-micro-v1:0",
        context_tokens: 128_000,
        output_tokens: 5_000
    }
);

define_model!(
    /// Nova Lite - Multimodal model for image, video, and text
    NovaLite {
        display_name: "Nova Lite",
        bedrock_id: "amazon.nova-lite-v1:0",
        context_tokens: 300_000,
        output_tokens: 5_000
    }
);

define_model!(
    /// Nova 2 Lite - Fast reasoning model with extended thinking support
    Nova2Lite {
        display_name: "Nova 2 Lite",
        bedrock_id: "amazon.nova-2-lite-v1:0",
        context_tokens: 1_000_000,
        output_tokens: 65_535,
        default_inference_profile: InferenceProfile::Global
    }
);

define_model!(
    /// Nova Pro - Balanced multimodal model
    NovaPro {
        display_name: "Nova Pro",
        bedrock_id: "amazon.nova-pro-v1:0",
        context_tokens: 300_000,
        output_tokens: 5_000
    }
);

define_model!(
    /// Nova Premier - Highest capability Nova model with 1M context
    NovaPremier {
        display_name: "Nova Premier",
        bedrock_id: "amazon.nova-premier-v1:0",
        context_tokens: 1_000_000,
        output_tokens: 5_000
    }
);
