//! OpenAI GPT-OSS models

use super::define_model;

define_model!(
    /// GPT OSS 20B - Open-source 20B model from OpenAI
    GptOss20B {
        display_name: "GPT OSS 20B",
        bedrock_id: "openai.gpt-oss-20b",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// GPT OSS 120B - Open-source 120B model from OpenAI
    GptOss120B {
        display_name: "GPT OSS 120B",
        bedrock_id: "openai.gpt-oss-120b",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// GPT OSS Safeguard 20B - Safety-focused 20B model from OpenAI
    GptOssSafeguard20B {
        display_name: "GPT OSS Safeguard 20B",
        bedrock_id: "openai.gpt-oss-safeguard-20b",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// GPT OSS Safeguard 120B - Safety-focused 120B model from OpenAI
    GptOssSafeguard120B {
        display_name: "GPT OSS Safeguard 120B",
        bedrock_id: "openai.gpt-oss-safeguard-120b",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
