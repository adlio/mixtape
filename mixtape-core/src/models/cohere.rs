//! Cohere models

use super::define_model;

define_model!(
    /// Command R+ - Enterprise RAG and multi-step tool use model
    CohereCommandRPlus {
        display_name: "Command R+",
        bedrock_id: "cohere.command-r-plus-v1:0",
        context_tokens: 128_000,
        output_tokens: 4_096
    }
);
