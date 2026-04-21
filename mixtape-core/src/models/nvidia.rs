//! NVIDIA Nemotron models

use super::define_model;

define_model!(
    /// Nemotron Nano 2 - Compact model from NVIDIA
    NemotronNano2 {
        display_name: "Nemotron Nano 2",
        bedrock_id: "nvidia.nemotron-nano-2",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Nemotron Nano 2 VL - Vision-language variant of Nemotron Nano 2
    NemotronNano2VL {
        display_name: "Nemotron Nano 2 VL",
        bedrock_id: "nvidia.nemotron-nano-2-vl",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Nemotron 3 Nano 30B A3B - 30B MoE model with 3B active parameters
    Nemotron3Nano30BA3B {
        display_name: "Nemotron 3 Nano 30B A3B",
        bedrock_id: "nvidia.nemotron-3-nano-30b-a3b",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);

define_model!(
    /// Nemotron 3 Super 120B A12B - 120B MoE model with 12B active parameters
    Nemotron3Super120BA12B {
        display_name: "Nemotron 3 Super 120B A12B",
        bedrock_id: "nvidia.nemotron-3-super-120b-a12b",
        context_tokens: 128_000,
        output_tokens: 8_192
    }
);
