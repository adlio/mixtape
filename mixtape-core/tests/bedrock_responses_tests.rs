#![cfg(feature = "bedrock")]

use aws_credential_types::{provider::SharedCredentialsProvider, Credentials};
use aws_sdk_bedrockruntime::config::Region;
use mixtape_core::{BedrockResponsesProvider, ModelProvider, RuntimeBedrockModel};

fn config() -> aws_config::SdkConfig {
    aws_config::SdkConfig::builder()
        .region(Region::new("us-west-2"))
        .credentials_provider(SharedCredentialsProvider::new(Credentials::new(
            "fixture-access-key",
            "fixture-secret-key",
            Some("fixture-session-token".into()),
            None,
            "offline-test",
        )))
        .build()
}

fn model(id: &str) -> RuntimeBedrockModel {
    RuntimeBedrockModel::new("fixture model", id, 128_000, 4096).unwrap()
}

#[test]
fn responses_requires_explicit_matching_routing() {
    let provider =
        BedrockResponsesProvider::from_sdk_config(&config(), model("openai.gpt-6-astra")).unwrap();
    assert!(provider.validate_configuration().is_err());
    assert!(provider
        .clone()
        .with_inference_target("us.moonshotai.kimi-k3")
        .is_err());
    let provider = provider
        .with_inference_target("us.openai.gpt-6-astra")
        .unwrap();
    assert!(provider.validate_configuration().is_ok());
    assert_eq!(provider.effective_model_id(), "us.openai.gpt-6-astra");
    assert_eq!(provider.name(), "fixture model");
}

#[test]
fn responses_rejects_application_profiles_and_unsupported_models() {
    let provider =
        BedrockResponsesProvider::from_sdk_config(&config(), model("openai.gpt-6-astra")).unwrap();
    assert!(provider
        .with_inference_target(
            "arn:aws:bedrock:us-west-2:123456789012:application-inference-profile/fixture"
        )
        .is_err());
    for id in [
        "anthropic.claude-opus-5",
        "openai.gpt-oss-120b-1:0",
        "unknown.model",
    ] {
        let provider = BedrockResponsesProvider::from_sdk_config(&config(), model(id)).unwrap();
        assert!(provider.validate_configuration().is_err(), "{id}");
    }
}

#[test]
fn responses_rejects_custom_endpoints_and_missing_regions() {
    let custom = config()
        .to_builder()
        .endpoint_url("https://example.invalid")
        .build();
    assert!(
        BedrockResponsesProvider::from_sdk_config(&custom, model("openai.gpt-6-astra")).is_err()
    );
    assert!(BedrockResponsesProvider::from_sdk_config(
        &aws_config::SdkConfig::builder().build(),
        model("openai.gpt-6-astra"),
    )
    .is_err());
}

#[test]
fn invalid_limits_and_zero_attempts_are_rejected() {
    let provider =
        BedrockResponsesProvider::from_sdk_config(&config(), model("openai.gpt-6-astra"))
            .unwrap()
            .with_inference_target("us.openai.gpt-6-astra")
            .unwrap();
    for limit in [-1, 0, 4097] {
        assert!(provider
            .clone()
            .with_max_tokens(limit)
            .validate_configuration()
            .is_err());
    }
    assert!(provider
        .with_max_retries(0)
        .validate_configuration()
        .is_err());
}
