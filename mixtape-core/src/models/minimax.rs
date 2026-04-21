//! MiniMax models

use super::define_model;

define_model!(
    /// MiniMax M2.1 - 229B MoE coding model with 128K output window
    MiniMaxM2_1 {
        display_name: "MiniMax M2.1",
        bedrock_id: "minimax.minimax-m2.1",
        context_tokens: 204_800,
        output_tokens: 131_072
    }
);

define_model!(
    /// MiniMax M2 - MoE model from MiniMax
    MiniMaxM2 {
        display_name: "MiniMax M2",
        bedrock_id: "minimax.minimax-m2",
        context_tokens: 204_800,
        output_tokens: 131_072
    }
);

define_model!(
    /// MiniMax M2.5 - Updated MoE model from MiniMax
    MiniMaxM2_5 {
        display_name: "MiniMax M2.5",
        bedrock_id: "minimax.minimax-m2.5",
        context_tokens: 204_800,
        output_tokens: 131_072
    }
);
