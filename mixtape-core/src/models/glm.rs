//! Z.AI GLM models

use super::define_model;

define_model!(
    /// GLM 4.7 - 358B MoE frontier coding model with 131K output window
    GLM4_7 {
        display_name: "GLM 4.7",
        bedrock_id: "zai.glm-4.7",
        context_tokens: 202_752,
        output_tokens: 131_072
    }
);

define_model!(
    /// GLM 4.7 Flash - 31B lightweight coding model
    GLM4_7Flash {
        display_name: "GLM 4.7 Flash",
        bedrock_id: "zai.glm-4.7-flash",
        context_tokens: 202_752,
        output_tokens: 131_072
    }
);

define_model!(
    /// GLM 5 - Next-gen frontier model from Z.AI
    GLM5 {
        display_name: "GLM 5",
        bedrock_id: "zai.glm-5",
        context_tokens: 202_752,
        output_tokens: 131_072
    }
);
