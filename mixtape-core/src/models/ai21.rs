//! AI21 Labs Jamba models

use super::define_model;

define_model!(
    /// Jamba 1.5 Large - Large hybrid SSM-Transformer model
    AI21Jamba1_5Large {
        display_name: "Jamba 1.5 Large",
        bedrock_id: "ai21.jamba-1-5-large-v1:0",
        context_tokens: 256_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Jamba 1.5 Mini - Compact hybrid SSM-Transformer model
    AI21Jamba1_5Mini {
        display_name: "Jamba 1.5 Mini",
        bedrock_id: "ai21.jamba-1-5-mini-v1:0",
        context_tokens: 256_000,
        output_tokens: 4_096
    }
);

define_model!(
    /// Jamba Instruct - Instruction-tuned hybrid model
    AI21JambaInstruct {
        display_name: "Jamba Instruct",
        bedrock_id: "ai21.jamba-instruct-v1:0",
        context_tokens: 256_000,
        output_tokens: 4_096
    }
);
