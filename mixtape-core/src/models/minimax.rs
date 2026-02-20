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
