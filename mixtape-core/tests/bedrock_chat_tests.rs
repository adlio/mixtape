#![cfg(feature = "bedrock")]

use aws_credential_types::{provider::SharedCredentialsProvider, Credentials};
use aws_sdk_bedrockruntime::config::Region;
use mixtape_core::{BedrockChatCompletionsProvider, ModelProvider, RuntimeBedrockModel};

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

fn kimi() -> RuntimeBedrockModel {
    RuntimeBedrockModel::new("Kimi K3", "moonshotai.kimi-k3", 128_000, 4096).unwrap()
}

#[test]
fn runtime_chat_provider_requires_explicit_routing() {
    let provider = BedrockChatCompletionsProvider::from_sdk_config(&config(), kimi()).unwrap();
    assert!(provider.validate_configuration().is_err());
    let provider = provider
        .with_inference_target("us.moonshotai.kimi-k3")
        .unwrap();
    assert!(provider.validate_configuration().is_ok());
    assert_eq!(provider.effective_model_id(), "us.moonshotai.kimi-k3");
    assert_eq!(provider.name(), "Kimi K3");
}

#[test]
fn exact_target_cannot_select_a_different_named_model() {
    let provider = BedrockChatCompletionsProvider::from_sdk_config(&config(), kimi()).unwrap();
    assert!(provider
        .with_inference_target("us.anthropic.claude-opus-5")
        .is_err());
}

#[test]
fn arbitrary_endpoints_and_missing_regions_are_rejected() {
    let custom = config()
        .to_builder()
        .endpoint_url("https://example.invalid/openai/v1")
        .build();
    assert!(BedrockChatCompletionsProvider::from_sdk_config(&custom, kimi()).is_err());
    assert!(BedrockChatCompletionsProvider::from_sdk_config(
        &aws_config::SdkConfig::builder().build(),
        kimi(),
    )
    .is_err());
}
