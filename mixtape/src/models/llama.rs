//! Meta Llama models

use super::define_model;

// =============================================================================
// Llama 4 Models
// =============================================================================

define_model!(
    /// Llama 4 Scout 17B - Efficient MoE model with 10M context
    Llama4Scout17B {
        display_name: "Llama 4 Scout 17B",
        bedrock_id: "meta.llama4-scout-17b-instruct-v1:0",
        context_tokens: 10_000_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Llama 4 Maverick 17B - Larger MoE model with 1M context
    Llama4Maverick17B {
        display_name: "Llama 4 Maverick 17B",
        bedrock_id: "meta.llama4-maverick-17b-instruct-v1:0",
        context_tokens: 1_000_000,
        output_tokens: 4_096
    }
);

// =============================================================================
// Llama 3.3 Models
// =============================================================================

define_model!(
    /// Llama 3.3 70B Instruct - Latest Llama 3.x flagship
    Llama3_3_70B {
        display_name: "Llama 3.3 70B",
        bedrock_id: "meta.llama3-3-70b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

// =============================================================================
// Llama 3.2 Models
// =============================================================================

define_model!(
    /// Llama 3.2 90B Instruct - Large multimodal model
    Llama3_2_90B {
        display_name: "Llama 3.2 90B",
        bedrock_id: "meta.llama3-2-90b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Llama 3.2 11B Instruct - Medium multimodal model
    Llama3_2_11B {
        display_name: "Llama 3.2 11B",
        bedrock_id: "meta.llama3-2-11b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Llama 3.2 3B Instruct - Efficient small model
    Llama3_2_3B {
        display_name: "Llama 3.2 3B",
        bedrock_id: "meta.llama3-2-3b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Llama 3.2 1B Instruct - Lightweight model for edge deployment
    Llama3_2_1B {
        display_name: "Llama 3.2 1B",
        bedrock_id: "meta.llama3-2-1b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

// =============================================================================
// Llama 3.1 Models
// =============================================================================

define_model!(
    /// Llama 3.1 405B Instruct - Largest open-weights model
    Llama3_1_405B {
        display_name: "Llama 3.1 405B",
        bedrock_id: "meta.llama3-1-405b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Llama 3.1 70B Instruct - High capability model
    Llama3_1_70B {
        display_name: "Llama 3.1 70B",
        bedrock_id: "meta.llama3-1-70b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Llama 3.1 8B Instruct - Efficient general purpose model
    Llama3_1_8B {
        display_name: "Llama 3.1 8B",
        bedrock_id: "meta.llama3-1-8b-instruct-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);
