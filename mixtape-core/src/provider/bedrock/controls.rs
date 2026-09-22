//! Converse-only request controls. Capability checks are API-specific; unknown
//! model contracts are rejected rather than silently omitted from a request.

use super::ConverseRequest;
use crate::provider::ProviderError;
use aws_sdk_bedrockruntime::types::{
    AnyToolChoice, AutoToolChoice, CachePointBlock, CachePointType, CacheTtl, ContentBlock,
    JsonSchemaDefinition, OutputConfig, OutputFormat, OutputFormatStructure, OutputFormatType,
    SpecificToolChoice, SystemContentBlock, Tool, ToolChoice,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

fn invalid(message: &str) -> ProviderError {
    ProviderError::Configuration(message.into())
}

/// TTL requested from a supported cache. A checkpoint does not guarantee a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BedrockCacheTtl {
    FiveMinutes,
    OneHour,
}

impl BedrockCacheTtl {
    fn block(self) -> Result<CachePointBlock, ProviderError> {
        CachePointBlock::builder()
            .r#type(CachePointType::Default)
            .ttl(match self {
                Self::FiveMinutes => CacheTtl::FiveMinutes,
                Self::OneHour => CacheTtl::OneHour,
            })
            .build()
            .map_err(|_| invalid("Unable to build a cache checkpoint"))
    }
}

/// Positions refer to the conversation passed to this provider call. Keep stable
/// instructions/evidence before checkpoints; do not guess token eligibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BedrockPromptCache {
    pub tools: Option<BedrockCacheTtl>,
    pub system: Option<BedrockCacheTtl>,
    /// Cache immediately after each selected message (zero-based index).
    pub messages: BTreeMap<usize, BedrockCacheTtl>,
}

/// Explicit selection, separate from the default behavior of omitting toolChoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "name", rename_all = "snake_case")]
pub enum BedrockToolChoice {
    Auto,
    None,
    Any,
    Tool(String),
}

/// A provider-enforced output shape. This is not an application validator: users
/// must still validate the returned result, citations, and current-run provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BedrockJsonSchema {
    pub name: String,
    pub schema: Value,
}

impl BedrockJsonSchema {
    fn output_config(&self) -> Result<OutputConfig, ProviderError> {
        if self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !self.schema.is_object()
        {
            return Err(invalid(
                "Structured output requires a schema object and a short alphanumeric name",
            ));
        }
        let schema = JsonSchemaDefinition::builder()
            .name(&self.name)
            .schema(self.schema.to_string())
            .build()
            .map_err(|_| invalid("Unable to build output schema"))?;
        let format = OutputFormat::builder()
            .r#type(OutputFormatType::JsonSchema)
            .structure(OutputFormatStructure::JsonSchema(schema))
            .build()
            .map_err(|_| invalid("Unable to build output format"))?;
        Ok(OutputConfig::builder().text_format(format).build())
    }
}

pub(super) fn validate(
    model_id: &str,
    cache: &BedrockPromptCache,
    choice: Option<&BedrockToolChoice>,
    schema: Option<&BedrockJsonSchema>,
    thinking: Option<&Value>,
) -> Result<(), ProviderError> {
    let mut ttls = Vec::new();
    ttls.extend(cache.tools);
    ttls.extend(cache.system);
    ttls.extend(cache.messages.values().copied());
    if ttls.len() > 4 {
        return Err(invalid(
            "Converse supports at most four configured cache checkpoints",
        ));
    }
    if !ttls.is_empty() {
        // AWS prompt-caching table reviewed 2026-09-22. This deliberately does not
        // infer support for all models in a vendor family or other APIs.
        let extended = matches!(
            model_id,
            "anthropic.claude-opus-5"
                | "anthropic.claude-sonnet-5"
                | "anthropic.claude-opus-4-8"
                | "anthropic.claude-opus-4-7"
                | "anthropic.claude-opus-4-6-v1"
                | "anthropic.claude-sonnet-4-6"
                | "anthropic.claude-opus-4-5-20251101-v1:0"
                | "anthropic.claude-sonnet-4-5-20250929-v1:0"
                | "anthropic.claude-haiku-4-5-20251001-v1:0"
                | "anthropic.claude-fable-5"
                | "anthropic.claude-fable-5-1"
        );
        let five_minutes = extended
            || matches!(
                model_id,
                "anthropic.claude-3-7-sonnet-20250219-v1:0"
                    | "anthropic.claude-3-5-sonnet-20241022-v2:0"
            );
        if !five_minutes || (!extended && ttls.contains(&BedrockCacheTtl::OneHour)) {
            return Err(invalid(
                "The selected cache TTL is not verified for this model on Converse",
            ));
        }
        let mut short_seen = false;
        for ttl in ttls {
            if ttl == BedrockCacheTtl::FiveMinutes {
                short_seen = true;
            } else if short_seen {
                return Err(invalid("One-hour checkpoints must precede five-minute checkpoints in tools/system/messages order"));
            }
        }
    }
    if matches!(
        choice,
        Some(BedrockToolChoice::Any | BedrockToolChoice::Tool(_))
    ) {
        if !model_id.starts_with("anthropic.claude-") && !model_id.starts_with("amazon.nova-") {
            return Err(invalid(
                "Forced tool choice is not verified for this model on Converse",
            ));
        }
        let enabled = match thinking
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
        {
            Some("disabled") => false,
            Some("enabled" | "adaptive") => true,
            _ => {
                matches!(
                    model_id,
                    "anthropic.claude-opus-5" | "anthropic.claude-sonnet-5"
                ) || model_id.starts_with("anthropic.claude-fable-")
            }
        };
        if enabled && model_id.starts_with("anthropic.") {
            return Err(invalid("Claude thinking requires automatic tool choice; disable thinking before forcing a tool"));
        }
    }
    if let Some(schema) = schema {
        // Opus 5's card explicitly lists native structured outputs as unsupported.
        // Positive support is currently confirmed for these two Converse models.
        if !matches!(
            model_id,
            "anthropic.claude-opus-4-5-20251101-v1:0" | "anthropic.claude-sonnet-4-5-20250929-v1:0"
        ) {
            return Err(invalid("Native JSON schema output is not verified for this model on Converse; application validation is still required"));
        }
        schema.output_config()?;
    }
    Ok(())
}

pub(super) fn apply(
    request: &mut ConverseRequest,
    cache: &BedrockPromptCache,
    choice: Option<&BedrockToolChoice>,
    schema: Option<&BedrockJsonSchema>,
) -> Result<(), ProviderError> {
    request.tool_choice = match choice {
        None => None,
        Some(BedrockToolChoice::None) => {
            request.tools.clear();
            None
        }
        Some(BedrockToolChoice::Auto) => Some(ToolChoice::Auto(AutoToolChoice::builder().build())),
        Some(BedrockToolChoice::Any) => Some(ToolChoice::Any(AnyToolChoice::builder().build())),
        Some(BedrockToolChoice::Tool(name)) => {
            if !request
                .tools
                .iter()
                .any(|tool| tool.as_tool_spec().is_ok_and(|spec| spec.name() == name))
            {
                return Err(invalid(
                    "The selected tool must exist in this request's tool definitions",
                ));
            }
            Some(ToolChoice::Tool(
                SpecificToolChoice::builder()
                    .name(name)
                    .build()
                    .map_err(|_| invalid("Invalid selected tool name"))?,
            ))
        }
    };
    if request.tool_choice.is_some() && request.tools.is_empty() {
        return Err(invalid("Tool choice requires tool definitions"));
    }
    if let Some(ttl) = cache.tools {
        if request.tools.is_empty() {
            return Err(invalid("Tool cache checkpoint requires tool definitions"));
        }
        request.tools.push(Tool::CachePoint(ttl.block()?));
    }
    if let Some(ttl) = cache.system {
        if request.system.is_empty() {
            return Err(invalid("System cache checkpoint requires a system prompt"));
        }
        request
            .system
            .push(SystemContentBlock::CachePoint(ttl.block()?));
    }
    for (&index, &ttl) in &cache.messages {
        let message = request
            .messages
            .get_mut(index)
            .ok_or_else(|| invalid("Cache checkpoint message index is outside this request"))?;
        if message.content.is_empty() {
            return Err(invalid(
                "A cache checkpoint needs preceding message content",
            ));
        }
        message.content.push(ContentBlock::CachePoint(ttl.block()?));
    }
    request.output_config = schema.map(BedrockJsonSchema::output_config).transpose()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::conversion;
    use super::*;
    use crate::{BedrockProvider, InferenceProfile, Message, RuntimeBedrockModel, ToolDefinition};
    use aws_sdk_bedrockruntime::{config::Region, Client};
    use serde_json::json;

    fn provider(model: &str) -> BedrockProvider {
        let config = aws_sdk_bedrockruntime::Config::builder()
            .behavior_version_latest()
            .region(Region::new("us-west-2"))
            .build();
        BedrockProvider::with_client(
            Client::from_conf(config),
            RuntimeBedrockModel::new("fixture", model, 128_000, 4096).unwrap(),
        )
        .with_inference_profile(InferenceProfile::US)
    }
    fn schema() -> BedrockJsonSchema {
        BedrockJsonSchema {
            name: "summary".into(),
            schema: json!({"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"],"additionalProperties":false}),
        }
    }

    #[test]
    fn checkpoint_order_positions_and_ttls_reach_native_request() {
        let cache = BedrockPromptCache {
            tools: Some(BedrockCacheTtl::OneHour),
            system: Some(BedrockCacheTtl::OneHour),
            messages: [(0, BedrockCacheTtl::FiveMinutes)].into(),
        };
        let provider = provider("anthropic.claude-opus-5").with_prompt_cache(cache);
        provider.validate_configuration().unwrap();
        let tool = conversion::to_bedrock_tool(&ToolDefinition {
            name: "evidence".into(),
            description: "fixture".into(),
            input_schema: json!({"type":"object"}),
        })
        .unwrap();
        let message = conversion::to_bedrock_message(&Message::user("fixture evidence")).unwrap();
        let request = provider
            .build_request(
                vec![message],
                vec![tool],
                Some("fixture instructions".into()),
            )
            .unwrap();
        assert_eq!(
            request.tools[1].as_cache_point().unwrap().ttl(),
            Some(&CacheTtl::OneHour)
        );
        assert_eq!(
            request.system[1].as_cache_point().unwrap().ttl(),
            Some(&CacheTtl::OneHour)
        );
        assert_eq!(
            request.messages[0].content[1]
                .as_cache_point()
                .unwrap()
                .ttl(),
            Some(&CacheTtl::FiveMinutes)
        );
    }

    #[test]
    fn invalid_cache_contracts_are_rejected_before_dispatch() {
        let reversed = BedrockPromptCache {
            tools: Some(BedrockCacheTtl::FiveMinutes),
            system: Some(BedrockCacheTtl::OneHour),
            ..Default::default()
        };
        assert!(provider("anthropic.claude-opus-5")
            .with_prompt_cache(reversed)
            .validate_configuration()
            .is_err());
        let unknown = BedrockPromptCache {
            system: Some(BedrockCacheTtl::FiveMinutes),
            ..Default::default()
        };
        assert!(provider("minimax.minimax-m2.1")
            .with_prompt_cache(unknown)
            .validate_configuration()
            .is_err());
        let too_many = BedrockPromptCache {
            messages: (0..5)
                .map(|index| (index, BedrockCacheTtl::FiveMinutes))
                .collect(),
            ..Default::default()
        };
        assert!(provider("anthropic.claude-opus-5")
            .with_prompt_cache(too_many)
            .validate_configuration()
            .is_err());
        let missing = BedrockPromptCache {
            messages: [(3, BedrockCacheTtl::FiveMinutes)].into(),
            ..Default::default()
        };
        assert!(provider("anthropic.claude-opus-5")
            .with_prompt_cache(missing)
            .build_request(vec![], vec![], None)
            .is_err());
    }

    #[test]
    fn opus_five_schema_and_forced_thinking_combinations_are_not_assumed_supported() {
        assert!(provider("anthropic.claude-opus-5")
            .with_output_schema(schema())
            .validate_configuration()
            .is_err());
        assert!(provider("anthropic.claude-opus-5")
            .with_tool_choice(BedrockToolChoice::Any)
            .validate_configuration()
            .is_err());
        assert!(provider("anthropic.claude-opus-5")
            .with_disabled_thinking()
            .with_tool_choice(BedrockToolChoice::Any)
            .validate_configuration()
            .is_ok());
    }

    #[test]
    fn native_schema_is_separate_from_claude_thinking_effort() {
        let provider = provider("anthropic.claude-opus-4-5-20251101-v1:0")
            .with_thinking_effort("high")
            .with_output_schema(schema());
        provider.validate_configuration().unwrap();
        let request = provider.build_request(vec![], vec![], None).unwrap();
        let format = request.output_config.unwrap().text_format.unwrap();
        let output = format.structure.unwrap();
        assert_eq!(output.as_json_schema().unwrap().name(), Some("summary"));
        assert_eq!(request.additional_fields["output_config"]["effort"], "high");
    }

    #[test]
    fn tool_none_omits_tools_and_unknown_forced_tool_is_rejected() {
        let provider = provider("anthropic.claude-opus-4-5-20251101-v1:0");
        assert!(provider
            .clone()
            .with_tool_choice(BedrockToolChoice::Tool("absent".into()))
            .build_request(vec![], vec![], None)
            .is_err());
        let tool = conversion::to_bedrock_tool(&ToolDefinition {
            name: "evidence".into(),
            description: "fixture".into(),
            input_schema: json!({"type":"object"}),
        })
        .unwrap();
        let request = provider
            .with_tool_choice(BedrockToolChoice::None)
            .build_request(vec![], vec![tool], None)
            .unwrap();
        assert!(request.tools.is_empty());
        assert!(request.tool_choice.is_none());
    }

    #[tokio::test]
    async fn controls_reach_both_converse_api_boundaries() {
        use super::super::{BedrockClient, ConverseRequest};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct CaptureClient(AtomicUsize);
        impl CaptureClient {
            fn capture(&self, request: ConverseRequest) {
                assert!(request.tools.last().unwrap().is_cache_point());
                assert!(request.system.last().unwrap().is_cache_point());
                assert!(request.messages[0].content.last().unwrap().is_cache_point());
                assert!(request.tool_choice.as_ref().unwrap().is_auto());
                assert!(request.output_config.is_some());
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        #[async_trait::async_trait]
        impl BedrockClient for CaptureClient {
            async fn converse(
                &self,
                request: ConverseRequest,
            ) -> Result<aws_sdk_bedrockruntime::operation::converse::ConverseOutput, ProviderError>
            {
                self.capture(request);
                Err(ProviderError::Model("fixture: stop before network".into()))
            }
            async fn converse_stream(
                &self,
                request: ConverseRequest,
            ) -> Result<
                aws_sdk_bedrockruntime::operation::converse_stream::ConverseStreamOutput,
                ProviderError,
            > {
                self.capture(request);
                Err(ProviderError::Model("fixture: stop before network".into()))
            }
        }
        let client = Arc::new(CaptureClient(AtomicUsize::new(0)));
        let provider = BedrockProvider::with_bedrock_client(client.clone(), crate::ClaudeOpus4_5)
            .with_output_schema(schema())
            .with_tool_choice(BedrockToolChoice::Auto)
            .with_prompt_cache(BedrockPromptCache {
                tools: Some(BedrockCacheTtl::OneHour),
                system: Some(BedrockCacheTtl::FiveMinutes),
                messages: [(0, BedrockCacheTtl::FiveMinutes)].into(),
            });
        let tools = vec![ToolDefinition {
            name: "evidence".into(),
            description: "fixture".into(),
            input_schema: json!({"type":"object"}),
        }];
        use crate::ModelProvider;
        assert!(provider
            .generate(
                vec![Message::user("fixture")],
                tools.clone(),
                Some("fixture".into())
            )
            .await
            .is_err());
        assert!(provider
            .generate_stream(
                vec![Message::user("fixture")],
                tools,
                Some("fixture".into())
            )
            .await
            .is_err());
        assert_eq!(client.0.load(Ordering::Relaxed), 2);
    }
}
