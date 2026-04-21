//! Writer AI Palmyra models

use super::define_model;

define_model!(
    /// Palmyra X4 - Enterprise language model from Writer
    WriterPalmyraX4 {
        display_name: "Palmyra X4",
        bedrock_id: "writer.palmyra-x4-v1:0",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Palmyra X5 - Latest enterprise language model from Writer
    WriterPalmyraX5 {
        display_name: "Palmyra X5",
        bedrock_id: "writer.palmyra-x5-v1:0",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
