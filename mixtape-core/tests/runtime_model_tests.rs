use mixtape_core::{BedrockModel, ClaudeSonnet4_5, InferenceProfile, Model, RuntimeBedrockModel};

#[test]
fn runtime_model_owns_configuration_strings() {
    let model = {
        let name = String::from("Configured model");
        let target = String::from("us.anthropic.claude-opus-5");
        RuntimeBedrockModel::new(name, target, 1_000_000, 16_000).unwrap()
    };
    assert_eq!(model.name(), "Configured model");
    assert_eq!(model.bedrock_id(), "us.anthropic.claude-opus-5");
    assert_eq!(model.max_context_tokens(), 1_000_000);
    assert_eq!(model.max_output_tokens(), 16_000);
    assert_eq!(model.default_inference_profile(), InferenceProfile::None);
    assert_eq!(model.estimate_token_count("hello"), 2);
}

#[test]
fn exact_targets_are_never_prefixed_or_rerouted() {
    for target in [
        "us.anthropic.claude-opus-5",
        "eu.anthropic.claude-opus-5",
        "au.anthropic.claude-opus-5",
        "in.openai.gpt-5.6-terra",
        "global.anthropic.claude-opus-5",
        "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/example",
        "arn:aws-us-gov:bedrock:us-gov-west-1:123456789012:provisioned-model/example",
    ] {
        let model = RuntimeBedrockModel::new("Example", target, 128_000, 4096).unwrap();
        assert_eq!(model.bedrock_id(), target);
        assert_eq!(
            InferenceProfile::Global.apply_to(model.bedrock_id()),
            target
        );
        assert_eq!(InferenceProfile::US.apply_to(model.bedrock_id()), target);
    }
}

#[test]
fn invalid_configuration_fails_without_an_api_call() {
    for target in [
        "",
        " ",
        "not a model",
        "https://example.com/model",
        "model\n",
    ] {
        assert!(RuntimeBedrockModel::new("Example", target, 128_000, 4096).is_err());
    }
    assert!(RuntimeBedrockModel::new("", "test.model", 128_000, 4096).is_err());
    assert!(RuntimeBedrockModel::new("Example", "test.model", 0, 4096).is_err());
    assert!(RuntimeBedrockModel::new("Example", "test.model", 128_000, 0).is_err());
    assert!(RuntimeBedrockModel::new("Example", "test.model", 128_000, usize::MAX).is_err());
}

#[test]
fn typed_models_keep_their_existing_ids_and_profile_defaults() {
    assert_eq!(ClaudeSonnet4_5.name(), "Claude Sonnet 4.5");
    assert_eq!(
        ClaudeSonnet4_5.default_inference_profile(),
        InferenceProfile::Global
    );
    assert_eq!(
        InferenceProfile::US.apply_to(ClaudeSonnet4_5.bedrock_id()),
        "us.anthropic.claude-sonnet-4-5-20250929-v1:0"
    );
}
