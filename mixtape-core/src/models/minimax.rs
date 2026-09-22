//! MiniMax models

use super::define_model;

define_model!(
    /// MiniMax M2.1 - 229B MoE coding model.
    /// Conservative Bedrock ceilings from the 196K context / 8K output model card;
    /// these are application limits, not an assertion that K means 1,000 tokens.
    MiniMaxM2_1 {
        display_name: "MiniMax M2.1",
        bedrock_id: "minimax.minimax-m2.1",
        context_tokens: 196_000,
        output_tokens: 8_000
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model;

    #[test]
    fn bedrock_limits_do_not_exceed_the_documented_196k_and_8k() {
        assert_eq!(MiniMaxM2_1.max_context_tokens(), 196_000);
        assert_eq!(MiniMaxM2_1.max_output_tokens(), 8_000);
    }
}
