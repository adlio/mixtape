//! Stateless Responses API on Bedrock Runtime. No stored conversation IDs,
//! background jobs, hosted tools, public vendor endpoint, or implicit fallback.
//!
//! <https://docs.aws.amazon.com/bedrock/latest/userguide/inference-responses-api.html>

mod cache;
mod conversion;
pub use cache::{BedrockResponsesCache, BedrockResponsesCacheMode};
mod streaming;
#[cfg(test)]
mod tests;

use super::chat_completions::transport::{
    self, bounded_body, HttpResponse, OpenAiApi, RuntimeClient, SignedClient,
};
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

fn invalid(message: &str) -> ProviderError {
    ProviderError::Configuration(message.into())
}
fn protocol(message: &str) -> ProviderError {
    ProviderError::Model(message.into())
}

/// Select this provider explicitly with `Agent::builder().provider(provider)`.
/// Requests always set `store=false`, retain full output items for local replay,
/// and use only client-side function tools. Construction never invokes a model.
#[derive(Clone)]
pub struct BedrockResponsesProvider {
    client: Arc<dyn RuntimeClient>,
    base_model_id: String,
    target: String,
    model_name: String,
    max_context_tokens: usize,
    max_output_tokens: usize,
    max_tokens: i32,
    reasoning_effort: Option<String>,
    tool_choice: Option<BedrockToolChoice>,
    output_schema: Option<BedrockJsonSchema>,
    prompt_cache: Option<BedrockResponsesCache>,
    retry_config: RetryConfig,
    on_retry: Option<RetryCallback>,
    on_invocation: Option<Arc<dyn Fn(BedrockInvocation) + Send + Sync>>,
}

impl BedrockResponsesProvider {
    pub async fn new(model: impl BedrockModel) -> Result<Self, ProviderError> {
        let config = aws_config::load_from_env().await;
        Self::from_sdk_config(&config, model)
    }

    /// Reuse a refreshable SDK credential provider and an explicit Region.
    /// Endpoint overrides are rejected; transport timeouts are 10s connect/300s total.
    pub fn from_sdk_config(
        config: &aws_config::SdkConfig,
        model: impl BedrockModel,
    ) -> Result<Self, ProviderError> {
        Self::with_transport(
            Arc::new(SignedClient::from_config(config, OpenAiApi::Responses)?),
            model,
        )
    }

    fn with_transport(
        client: Arc<dyn RuntimeClient>,
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
            reasoning_effort: None,
            tool_choice: None,
            output_schema: None,
            prompt_cache: None,
            retry_config: RetryConfig::default(),
            on_retry: None,
            on_invocation: None,
        })
    }

    fn model_id(&self) -> &str {
        bedrock_model_descriptor(&self.base_model_id)
            .map(|model| model.model_id)
            .unwrap_or(&self.base_model_id)
    }

    /// Exact request target, distinct from any model identity returned by Bedrock.
    pub fn effective_model_id(&self) -> &str {
        &self.target
    }

    pub fn with_inference_profile(self, profile: InferenceProfile) -> Result<Self, ProviderError> {
        let target = profile.apply_to(self.model_id());
        self.with_inference_target(target)
    }

    /// Runtime Responses supports system inference profiles, not application
    /// profiles. Exact system-profile ARNs must name this same selected model.
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
        let id = if target.starts_with("arn:") {
            target.splitn(6, ':').nth(5).and_then(|resource| resource.strip_prefix("inference-profile/"))
                .ok_or_else(|| invalid("Responses requires a system inference profile, not an application profile or other ARN"))?
        } else {
            &target
        };
        let base = bedrock_model_descriptor(id)
            .map(|model| model.model_id)
            .unwrap_or(id);
        if base != self.model_id() {
            return Err(invalid(
                "Inference target names a different or unverified model",
            ));
        }
        self.target = target;
        Ok(self)
    }

    pub fn with_max_tokens(mut self, max_tokens: i32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
    pub fn with_max_retries(mut self, attempts: usize) -> Self {
        self.retry_config.max_attempts = attempts;
        self
    }
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }
    pub fn with_reasoning_effort(mut self, effort: impl Into<String>) -> Self {
        self.reasoning_effort = Some(effort.into());
        self
    }
    pub fn with_tool_choice(mut self, choice: BedrockToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }
    pub fn with_output_schema(mut self, schema: BedrockJsonSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    pub fn with_prompt_cache(mut self, cache: BedrockResponsesCache) -> Self {
        self.prompt_cache = Some(cache);
        self
    }

    pub fn with_retry_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(RetryInfo) + Send + Sync + 'static,
    {
        self.on_retry = Some(Arc::new(callback));
        self
    }

    /// Never includes prompt/output bodies, reasoning, or credentials. Dropping a
    /// request or stream emits Cancelled once; callbacks must not panic or block.
    pub fn with_invocation_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(BedrockInvocation) + Send + Sync + 'static,
    {
        self.on_invocation = Some(Arc::new(callback));
        self
    }

    /// Local validation is not proof of account access or model/data eligibility.
    pub fn validate_configuration(&self) -> Result<(), ProviderError> {
        if self.max_tokens <= 0 || self.max_tokens as usize > self.max_output_tokens {
            return Err(invalid(
                "max_tokens must be positive and within the configured output ceiling",
            ));
        }
        if self.retry_config.max_attempts == 0 {
            return Err(invalid("At least one request attempt is required"));
        }
        let model_id = self.model_id();
        if !matches!(
            model_id,
            "openai.gpt-6-astra"
                | "openai.gpt-5.6-sol"
                | "openai.gpt-5.6-terra"
                | "openai.gpt-5.6-luna"
                | "moonshotai.kimi-k3"
        ) {
            return Err(invalid("Responses on Bedrock Runtime is not verified for this model; no fallback is selected"));
        }
        if self.target == model_id {
            return Err(invalid(
                "This model requires an explicit system inference-profile ID or ARN",
            ));
        }
        // Validate default targets as well as targets set by the builder.
        self.clone().with_inference_target(self.target.clone())?;
        if let Some(effort) = &self.reasoning_effort {
            let supported = if model_id == "moonshotai.kimi-k3" {
                matches!(effort.as_str(), "low" | "high" | "max")
            } else {
                matches!(effort.as_str(), "low" | "medium" | "high" | "xhigh" | "max")
            };
            if !supported {
                return Err(invalid(
                    "Responses reasoning effort is not verified for this model/value",
                ));
            }
        }
        if let Some(schema) = &self.output_schema {
            // GPT cards list native structured outputs as unsupported on Runtime.
            if model_id != "moonshotai.kimi-k3" {
                return Err(invalid(
                    "Native JSON schema output is not verified for this model on Runtime Responses",
                ));
            }
            super::openai_controls::validate_schema(schema)?;
        }
        if let Some(cache) = &self.prompt_cache {
            cache.validate(model_id)?;
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
        let mut body = json!({
            "model": self.target,
            "input": conversion::input(messages, system, self.model_id())?,
            "store": false,
            "include": ["reasoning.encrypted_content"],
            "max_output_tokens": self.max_tokens,
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
            true,
        )?;
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning"] = json!({"effort": effort});
        }
        if let Some(schema) = &self.output_schema {
            body["text"] = json!({"format": {"type": "json_schema", "name": schema.name, "schema": schema.schema, "strict": true}});
        }
        if let Some(cache) = &self.prompt_cache {
            cache.apply(&mut body, messages, system)?;
        }
        serde_json::to_vec(&body).map_err(|_| invalid("Unable to serialize Responses request"))
    }

    fn guard(&self, streaming: bool) -> InvocationGuard {
        InvocationGuard::new(
            BedrockInvocation::new(
                &self.base_model_id,
                self.target.clone(),
                if streaming {
                    "responses_stream"
                } else {
                    "responses"
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
                    return Err(protocol("Unexpected Bedrock Responses content type"));
                }
                guard.record.request_id = response.request_id.clone();
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
impl ModelProvider for BedrockResponsesProvider {
    fn name(&self) -> &str {
        &self.model_name
    }
    fn max_context_tokens(&self) -> usize {
        self.max_context_tokens
    }
    fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    fn estimate_message_tokens(&self, messages: &[Message]) -> usize {
        // Debug intentionally redacts replay data. Count the serialized request
        // shape instead, so opaque reasoning cannot disappear from the estimate.
        match conversion::input(messages, None, self.model_id()).and_then(|input| {
            serde_json::to_string(&input).map_err(|_| invalid("Cannot estimate Responses input"))
        }) {
            Ok(input) => self.estimate_token_count(&input),
            Err(_) => self.max_context_tokens,
        }
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
                .map_err(|_| protocol("Invalid JSON in Bedrock Responses response"))?;
            conversion::response(&value, self.model_id(), &mut guard.record)
        }
        .await;
        guard.finish(match &result {
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
        });
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
        let model = self.model_id().to_owned();
        let stream = async_stream::stream! {
            let mut events = bounded_body(response.body).eventsource();
            let mut assembler = streaming::StreamAssembler::new(model);
            while let Some(event) = events.next().await {
                let result = match event {
                    Ok(event) => assembler.push(&event.event, &event.data, &mut guard.record),
                    Err(_) => Err(protocol("Bedrock Responses SSE framing or transport failed")),
                };
                match result {
                    Ok(events) => {
                        if let Some(outcome) = assembler.outcome() { guard.finish(outcome); }
                        for event in events { yield Ok(event); }
                        if assembler.outcome().is_some() { return; }
                    }
                    Err(error) => { guard.finish(InvocationOutcome::Failed); yield Err(error); return; }
                }
            }
            guard.finish(InvocationOutcome::Failed);
            yield Err(protocol("Bedrock Responses stream ended before a terminal response event"));
        };
        Ok(Box::pin(stream))
    }
}
