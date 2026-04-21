//! Mistral AI models

use super::define_model;

define_model!(
    /// Mistral Large 2 - Previous generation flagship model
    MistralLarge2 {
        display_name: "Mistral Large 2",
        bedrock_id: "mistral.mistral-large-2407-v1:0",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Mistral Small - Compact instruction model
    MistralSmall {
        display_name: "Mistral Small",
        bedrock_id: "mistral.mistral-small-2402-v1:0",
        context_tokens: 32_000,
        output_tokens: 8_192
    }
);

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

define_model!(
    /// Ministral 3B - Compact 3B instruction model
    Ministral3B {
        display_name: "Ministral 3B",
        bedrock_id: "mistral.ministral-3-3b-instruct",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Ministral 8B - Efficient 8B instruction model
    Ministral8B {
        display_name: "Ministral 8B",
        bedrock_id: "mistral.ministral-3-8b-instruct",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Ministral 14B - Mid-size 14B instruction model
    Ministral14B {
        display_name: "Ministral 14B",
        bedrock_id: "mistral.ministral-3-14b-instruct",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Pixtral Large - Vision-capable large model
    PixtralLarge {
        display_name: "Pixtral Large",
        bedrock_id: "mistral.pixtral-large-2502-v1:0",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Voxtral Mini 3B - Speech and text input model
    VoxtralMini3B {
        display_name: "Voxtral Mini 3B",
        bedrock_id: "mistral.voxtral-mini-3b-2507",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Voxtral Small 24B - Speech and text input model
    VoxtralSmall24B {
        display_name: "Voxtral Small 24B",
        bedrock_id: "mistral.voxtral-small-24b-2507",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
