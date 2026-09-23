//! Bedrock Runtime Chat Completions provider.
//!
//! Uses AWS SigV4 on `/openai/v1/chat/completions`, not a public vendor API.
//! Intended first for Kimi's `reasoning_content` tool conversations. Signed and
//! opaque reasoning formats require their own API adapter and are rejected.
//!
//! References:
//! - <https://docs.aws.amazon.com/bedrock/latest/userguide/inference-chat-completions.html>
//! - <https://docs.aws.amazon.com/bedrock/latest/userguide/model-card-moonshot-ai-kimi-k3.html>

mod cache;
mod conversion;
pub use cache::BedrockChatCache;
mod streaming;
#[cfg(test)]
mod tests;
pub(super) mod transport;

use super::telemetry::InvocationGuard;
use super::{BedrockInvocation, BedrockJsonSchema, BedrockToolChoice, InvocationOutcome};
use crate::model::{BedrockModel, InferenceProfile, ModelResponse, RuntimeBedrockModel};
use crate::models::bedrock_model_descriptor;
use crate::provider::retry::{retry_with_backoff, RetryCallback, RetryConfig, RetryInfo};
use crate::provider::{ModelProvider, ProviderError, StreamEvent};
use crate::types::{Message, StopReason, ToolDefinition};
use eventsource_stream::Eventsource;
use futures::{stream::BoxStream, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use transport::{bounded_body, HttpResponse, OpenAiApi, RuntimeClient as ChatClient, SignedClient};

type InvocationCallback = Arc<dyn Fn(BedrockInvocation) + Send + Sync>;

fn invalid(message: &str) -> ProviderError {
    ProviderError::Configuration(message.into())
}
fn protocol(message: &str) -> ProviderError {
    ProviderError::Model(message.into())
}

/// Explicit alternate API selection. Existing `BedrockProvider` callers continue
/// to use Converse; neither provider silently falls back to the other's API.
///
/// Build with an AWS SDK configuration, then pass to `Agent::builder().provider`.
/// Construction makes no model request. Model availability and data eligibility
/// must be checked separately from this provider's local configuration validation.
#[derive(Clone)]
pub struct BedrockChatCompletionsProvider {
    client: Arc<dyn ChatClient>,
    base_model_id: String,
    target: String,
    model_name: String,
    max_context_tokens: usize,
    max_output_tokens: usize,
    max_tokens: i32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    reasoning_effort: Option<String>,
    tool_choice: Option<BedrockToolChoice>,
    output_schema: Option<BedrockJsonSchema>,
    prompt_cache: Option<BedrockChatCache>,
    retry_config: RetryConfig,
    on_retry: Option<RetryCallback>,
    on_invocation: Option<InvocationCallback>,
}

impl BedrockChatCompletionsProvider {
    /// Load the standard AWS configuration and refreshable credential provider.
    pub async fn new(model: impl BedrockModel) -> Result<Self, ProviderError> {
        let config = aws_config::load_from_env().await;
        Self::from_sdk_config(&config, model)
    }

    /// Reuse a caller's Region and credential provider. Custom endpoint overrides
    /// are rejected: this adapter sends credentials only to Bedrock Runtime.
    /// HTTP requests have a 10-second connect timeout and 300-second total timeout.
    pub fn from_sdk_config(
        config: &aws_config::SdkConfig,
        model: impl BedrockModel,
    ) -> Result<Self, ProviderError> {
        Self::with_transport(
            Arc::new(SignedClient::from_config(
                config,
                OpenAiApi::ChatCompletions,
            )?),
            model,
        )
    }

    fn with_transport(
        client: Arc<dyn ChatClient>,
        model: impl BedrockModel,
    ) -> Result<Self, ProviderError> {
        RuntimeBedrockModel::new(
            model.name(),
            model.bedrock_id(),
            model.max_context_tokens(),
            model.max_output_tokens(),
        )?;
        Ok(Self {
            client,
            base_model_id: model.bedrock_id().into(),
            target: model
                .default_inference_profile()
                .apply_to(model.bedrock_id()),
            model_name: model.name().into(),
            max_context_tokens: model.max_context_tokens(),
            max_output_tokens: model.max_output_tokens(),
            max_tokens: model.max_output_tokens().min(4096) as i32,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            tool_choice: None,
            output_schema: None,
            prompt_cache: None,
            retry_config: RetryConfig::default(),
            on_retry: None,
            on_invocation: None,
        })
    }

    /// Exact ID written into the request body, not provider-reported identity.
    pub fn effective_model_id(&self) -> &str {
        &self.target
    }

    /// Explicit geographic routing, retaining the selected base model.
    pub fn with_inference_profile(self, profile: InferenceProfile) -> Result<Self, ProviderError> {
        let target = profile.apply_to(&self.base_model_id);
        self.with_inference_target(target)
    }

    /// Select a matching model/profile ID or an opaque inference-profile ARN.
    /// Callers must verify an opaque ARN's model mapping using account metadata.
    pub fn with_inference_target(
        mut self,
        target: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let target = target.into();
        RuntimeBedrockModel::new(
            &self.model_name,
            &target,
            self.max_context_tokens,
            self.max_output_tokens,
        )?;
        let base = bedrock_model_descriptor(&self.base_model_id)
            .map(|model| model.model_id)
            .unwrap_or(&self.base_model_id);
        let target_base = ["us.", "eu.", "apac.", "au.", "in.", "jp.", "global."]
            .iter()
            .find_map(|prefix| target.strip_prefix(prefix))
            .unwrap_or(&target);
        if !target.starts_with("arn:") && target_base != base {
            return Err(invalid("Inference target names a different model"));
        }
        self.target = target;
        Ok(self)
    }

    pub fn with_max_tokens(mut self, max_tokens: i32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Kimi K3's documented effort values: low, high, or max. Omission preserves
    /// the model default. This is not Claude's adaptive/manual thinking contract.
    pub fn with_reasoning_effort(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }

    /// Use the selected model's verified Chat tool-choice contract.
    pub fn with_tool_choice(mut self, choice: BedrockToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Request a native schema where the Bedrock model card supports it.
    /// Application validation of the result is still required.
    pub fn with_output_schema(mut self, schema: BedrockJsonSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Request explicit Kimi text checkpoints. Omit for provider-default caching.
    pub fn with_prompt_cache(mut self, cache: BedrockChatCache) -> Self {
        self.prompt_cache = Some(cache);
        self
    }

    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }
    pub fn with_max_retries(mut self, attempts: usize) -> Self {
        self.retry_config.max_attempts = attempts;
        self
    }

    pub fn with_retry_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(RetryInfo) + Send + Sync + 'static,
    {
        self.on_retry = Some(Arc::new(callback));
        self
    }

    /// Metadata only. Dropped requests/streams produce Cancelled, never Completed.
    /// The callback must return promptly and must not panic, including on drop.
    pub fn with_invocation_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(BedrockInvocation) + Send + Sync + 'static,
    {
        self.on_invocation = Some(Arc::new(callback));
        self
    }

    pub fn validate_configuration(&self) -> Result<(), ProviderError> {
        if self.max_tokens <= 0 || self.max_tokens as usize > self.max_output_tokens {
            return Err(invalid(
                "max_tokens must be positive and within the configured output ceiling",
            ));
        }
        if self.retry_config.max_attempts == 0 {
            return Err(invalid("At least one request attempt is required"));
        }
        if self
            .temperature
            .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
            || self
                .top_p
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(invalid("Invalid Chat Completions sampling value"));
        }
        if self
            .target
            .splitn(6, ':')
            .nth(5)
            .is_some_and(|resource| resource.starts_with("application-inference-profile/"))
        {
            return Err(invalid(
                "Chat Completions does not support application inference profiles",
            ));
        }
        if let Some(model) = bedrock_model_descriptor(&self.base_model_id) {
            if model.model_id.starts_with("anthropic.") {
                return Err(invalid("Claude requires Bedrock Converse or Messages; Chat Completions is not supported"));
            }
            if model.requires_inference_profile && self.target == model.model_id {
                return Err(invalid(
                    "This model requires an explicit inference-profile ID or ARN",
                ));
            }
        }
        if let Some(effort) = &self.reasoning_effort {
            let base = bedrock_model_descriptor(&self.base_model_id)
                .map(|model| model.model_id)
                .unwrap_or(&self.base_model_id);
            if base != "moonshotai.kimi-k3" || !matches!(effort.as_str(), "low" | "high" | "max") {
                return Err(invalid(
                    "Reasoning effort is not verified for this model/value",
                ));
            }
        }
        let base = bedrock_model_descriptor(&self.base_model_id)
            .map(|model| model.model_id)
            .unwrap_or(&self.base_model_id);
        if self.output_schema.is_some()
            || matches!(
                self.tool_choice,
                Some(BedrockToolChoice::Any | BedrockToolChoice::Tool(_))
            )
        {
            if base != "moonshotai.kimi-k3" {
                return Err(invalid("Native schema or forced tools are not verified for this model on Chat Completions"));
            }
            if let Some(schema) = &self.output_schema {
                super::openai_controls::validate_schema(schema)?;
            }
        }
        if let Some(cache) = &self.prompt_cache {
            cache.validate(base)?;
        }
        Ok(())
    }

    fn request(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: Option<&str>,
        streaming: bool,
    ) -> Result<Vec<u8>, ProviderError> {
        self.validate_configuration()?;
        let base = bedrock_model_descriptor(&self.base_model_id)
            .map(|model| model.model_id)
            .unwrap_or(&self.base_model_id);
        if base == "openai.gpt-6-astra" && !tools.is_empty() {
            return Err(invalid(
                "GPT-6 Astra function calling requires the Responses adapter",
            ));
        }
        let mut body = json!({
            "model": self.target,
            "messages": conversion::messages(messages, system, self.prompt_cache.as_ref())?,
            "max_tokens": self.max_tokens,
            "stream": streaming,
        });
        let definitions = conversion::tools(tools)?;
        if !definitions.is_empty() {
            body["tools"] = json!(definitions);
        }
        super::openai_controls::apply_tool_choice(
            &mut body,
            tools,
            self.tool_choice.as_ref(),
            false,
        )?;
        if let Some(schema) = &self.output_schema {
            body["response_format"] = json!({"type":"json_schema", "json_schema": {
                "name":schema.name, "schema":schema.schema, "strict":true
            }});
        }
        if let Some(temperature) = self.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(top_p) = self.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
        if let Some(cache) = &self.prompt_cache {
            cache.apply_options(&mut body);
        }
        if streaming {
            body["stream_options"] = json!({"include_usage": true});
        }
        serde_json::to_vec(&body)
            .map_err(|_| invalid("Unable to serialize Chat Completions request"))
    }

    fn guard(&self, streaming: bool) -> InvocationGuard {
        InvocationGuard::new(
            BedrockInvocation::new(
                &self.base_model_id,
                self.target.clone(),
                if streaming {
                    "chat_completions_stream"
                } else {
                    "chat_completions"
                },
                Some(self.client.region().into()),
            ),
            self.on_invocation.clone(),
            Some(0),
        )
    }

    async fn send(
        &self,
        body: Vec<u8>,
        streaming: bool,
        guard: &mut InvocationGuard,
    ) -> Result<HttpResponse, ProviderError> {
        let response = retry_with_backoff(
            || {
                guard.attempts.fetch_add(1, Ordering::Relaxed);
                let body = body.clone();
                let request_id = guard.last_request_id.clone();
                async move {
                    let response = self.client.send(body, streaming).await?;
                    *request_id.lock().unwrap_or_else(|error| error.into_inner()) =
                        response.request_id.clone();
                    if let Some(error) = transport::status_error(response.status) {
                        return Err(error);
                    }
                    Ok(response)
                }
            },
            &self.retry_config,
            &self.on_retry,
        )
        .await;
        match response {
            Ok(response) => {
                guard.record.request_id = response.request_id.clone();
                let expected = if streaming {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                if !response.content_type.as_deref().is_some_and(|value| {
                    value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case(expected)
                }) {
                    guard.finish(InvocationOutcome::Failed);
                    return Err(protocol("Unexpected Bedrock Chat Completions content type"));
                }
                Ok(response)
            }
            Err(error) => {
                guard.finish(InvocationOutcome::Failed);
                Err(error)
            }
        }
    }
}

#[async_trait::async_trait]
impl ModelProvider for BedrockChatCompletionsProvider {
    fn name(&self) -> &str {
        &self.model_name
    }
    fn max_context_tokens(&self) -> usize {
        self.max_context_tokens
    }
    fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    async fn generate(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        let body = self.request(&messages, &tools, system_prompt.as_deref(), false)?;
        let mut guard = self.guard(false);
        let response = self.send(body, false, &mut guard).await?;
        let result = async {
            let mut bytes = Vec::new();
            let mut body = bounded_body(response.body);
            while let Some(chunk) = body.next().await {
                bytes.extend(chunk?);
            }
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| protocol("Invalid JSON in Bedrock Chat Completions response"))?;
            conversion::response(&value, &mut guard.record)
        }
        .await;
        let outcome = match &result {
            Ok(response)
                if matches!(
                    response.stop_reason,
                    StopReason::EndTurn | StopReason::ToolUse
                ) =>
            {
                InvocationOutcome::Completed
            }
            Ok(_) => InvocationOutcome::Rejected,
            Err(_) => InvocationOutcome::Failed,
        };
        guard.finish(outcome);
        result
    }

    async fn generate_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        let body = self.request(&messages, &tools, system_prompt.as_deref(), true)?;
        let mut guard = self.guard(true);
        let response = self.send(body, true, &mut guard).await?;
        // Retry only the initial HTTP request. Reconnecting SSE can duplicate
        // generated text or tool execution and would hide partial failures.
        let stream = async_stream::stream! {
            let mut events = bounded_body(response.body).eventsource();
            let mut assembler = streaming::StreamAssembler::default();
            while let Some(event) = events.next().await {
                let result = match event {
                    Ok(event) if event.event == "message" || event.event.is_empty() => assembler.push(&event.data),
                    Ok(_) => Err(protocol("Unexpected Bedrock Chat Completions SSE event type")),
                    Err(_) => Err(protocol("Bedrock Chat Completions SSE framing or transport failed")),
                };
                assembler.update_invocation(&mut guard.record);
                match result {
                    Ok(events) => {
                        if assembler.done() {
                            guard.finish(if assembler.rejected() { InvocationOutcome::Rejected } else { InvocationOutcome::Completed });
                        }
                        for event in events { yield Ok(event); }
                        if assembler.done() { return; }
                    }
                    Err(error) => {
                        guard.finish(InvocationOutcome::Failed);
                        yield Err(error);
                        return;
                    }
                }
            }
            guard.finish(InvocationOutcome::Failed);
            yield Err(protocol("Bedrock Chat Completions stream ended before [DONE]"));
        };
        Ok(Box::pin(stream))
    }
}
