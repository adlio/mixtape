//! Opus 5.5 Converse contracts from the AWS model card and Anthropic migration
//! guide (reviewed 2026-09-25). These tests never dispatch network requests.

#![cfg(feature = "bedrock")]

use aws_sdk_bedrockruntime::{config::Region, Client};
use mixtape_core::{
    models::{bedrock_model_descriptor, BedrockConversationApi},
    provider::bedrock::{
        BedrockCacheTtl, BedrockJsonSchema, BedrockPromptCache, BedrockToolChoice,
    },
    BedrockModel, BedrockProvider, InferenceProfile, RuntimeBedrockModel,
};
use serde_json::json;

const OPUS: &str = "anthropic.claude-opus-5-5";

fn provider() -> BedrockProvider {
    let config = aws_sdk_bedrockruntime::Config::builder()
        .behavior_version_latest()
        .region(Region::new("us-west-2"))
        .build();
    BedrockProvider::with_client(
        Client::from_conf(config),
        RuntimeBedrockModel::new("Claude Opus 5.5", OPUS, 128_000, 4096).unwrap(),
    )
    .with_inference_profile(InferenceProfile::US)
}

#[test]
fn catalog_preserves_documented_limits_and_requires_explicit_routing() {
    let descriptor = bedrock_model_descriptor(OPUS).expect("Opus 5.5 must be registered");
    assert_eq!(descriptor.model_id, OPUS);
    assert_eq!(descriptor.display_name, "Claude Opus 5.5");
    assert_eq!(descriptor.context_window_label, "1M");
    assert_eq!(descriptor.max_output_label, Some("128K"));
    assert_eq!(
        descriptor.conversation_api,
        BedrockConversationApi::Converse
    );
    assert!(descriptor.requires_inference_profile);
    assert!(descriptor
        .model_card
        .ends_with("model-card-anthropic-claude-opus-5-5.html"));
    let model = descriptor.with_token_limits(128_000, 4096).unwrap();
    assert_eq!(model.bedrock_id(), OPUS);
    assert_eq!(model.default_inference_profile(), InferenceProfile::None);
    assert!(provider()
        .with_inference_profile(InferenceProfile::None)
        .validate_configuration()
        .is_err());
}

#[test]
fn geographic_targets_preserve_model_identity() {
    for prefix in ["us", "eu", "au", "jp", "global"] {
        let target = format!("{prefix}.{OPUS}");
        assert_eq!(bedrock_model_descriptor(&target).unwrap().model_id, OPUS);
        provider()
            .with_inference_target(target)
            .unwrap()
            .validate_configuration()
            .unwrap();
    }
    assert!(provider()
        .with_inference_target("us.anthropic.claude-opus-5")
        .is_err());
}

#[test]
fn default_and_adaptive_thinking_accept_all_five_efforts() {
    provider().validate_configuration().unwrap();
    provider()
        .with_adaptive_thinking()
        .validate_configuration()
        .unwrap();
    for display in ["omitted", "summarized"] {
        provider()
            .with_additional_field("thinking", json!({"type":"adaptive","display":display}))
            .validate_configuration()
            .unwrap();
    }
    for effort in ["low", "medium", "high", "xhigh", "max"] {
        provider()
            .with_thinking_effort(effort)
            .validate_configuration()
            .unwrap();
        provider()
            .with_adaptive_thinking()
            .with_thinking_effort(effort)
            .validate_configuration()
            .unwrap();
    }
}

#[test]
fn thinking_cannot_be_disabled_or_given_a_manual_budget() {
    assert!(provider()
        .with_disabled_thinking()
        .validate_configuration()
        .is_err());
    assert!(provider()
        .with_thinking(1024)
        .validate_configuration()
        .is_err());
    assert!(provider()
        .with_additional_field("thinking", json!({"type":"adaptive","budget_tokens":1024}))
        .validate_configuration()
        .is_err());
    assert!(provider()
        .with_thinking_effort("ultra")
        .validate_configuration()
        .is_err());
}

#[test]
fn tool_choice_is_automatic_or_none_never_forced() {
    for choice in [BedrockToolChoice::Auto, BedrockToolChoice::None] {
        provider()
            .with_tool_choice(choice)
            .validate_configuration()
            .unwrap();
    }
    for choice in [
        BedrockToolChoice::Any,
        BedrockToolChoice::Tool("lookup".into()),
    ] {
        assert!(provider()
            .with_tool_choice(choice.clone())
            .validate_configuration()
            .is_err());
        assert!(provider()
            .with_adaptive_thinking()
            .with_tool_choice(choice.clone())
            .validate_configuration()
            .is_err());
        assert!(provider()
            .with_disabled_thinking()
            .with_tool_choice(choice)
            .validate_configuration()
            .is_err());
    }
}

#[test]
fn sampling_parameters_must_be_omitted_including_custom_fields() {
    // Require omission instead of guessing which explicit defaults each Bedrock
    // API accepts. Sampling overrides are not supported by this model.
    for configured in [
        provider().with_temperature(0.0),
        provider().with_temperature(1.0),
        provider().with_top_p(0.9),
        provider().with_top_k(50),
        provider().with_additional_field("temperature", json!(0.0)),
        provider().with_additional_field("top_p", json!(0.9)),
        provider().with_additional_field("top_k", json!(50)),
    ] {
        assert!(configured.validate_configuration().is_err());
    }
}

#[test]
fn both_documented_cache_ttls_are_supported_but_native_schema_is_not() {
    for ttl in [BedrockCacheTtl::FiveMinutes, BedrockCacheTtl::OneHour] {
        provider()
            .with_prompt_cache(BedrockPromptCache {
                tools: Some(ttl),
                system: Some(ttl),
                messages: [(0, ttl)].into(),
            })
            .validate_configuration()
            .unwrap();
    }
    assert!(provider()
        .with_output_schema(BedrockJsonSchema {
            name: "summary".into(),
            schema: json!({"type":"object"}),
        })
        .validate_configuration()
        .is_err());
}
