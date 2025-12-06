//! Anthropic direct API provider implementation

mod conversion;

use super::retry::{retry_with_backoff, RetryCallback, RetryConfig, RetryInfo};
use super::{ModelProvider, ProviderError, StreamEvent};
use crate::events::TokenUsage;
use crate::model::{AnthropicModel, ModelResponse};
use crate::types::{Message, StopReason, ThinkingConfig, ToolDefinition, ToolUseBlock};
use conversion::{
    from_anthropic_message, from_anthropic_stop_reason, to_anthropic_message, to_anthropic_tool,
};
use futures::stream::BoxStream;
use futures::StreamExt;
use mixtape_anthropic_sdk::{
    Anthropic, AnthropicError, ContentBlock as AnthropicContentBlock, ContentBlockDelta,
    MessageCreateParams, MessageStreamEvent, Tool as AnthropicTool,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Default maximum tokens to generate
const DEFAULT_MAX_TOKENS: i32 = 4096;

// ===== Error Classification =====

fn classify_anthropic_error(err: &AnthropicError) -> ProviderError {
    match err {
        AnthropicError::Authentication(msg) => ProviderError::Authentication(msg.clone()),
        AnthropicError::RateLimited(msg) => ProviderError::RateLimited(msg.clone()),
        AnthropicError::ServiceUnavailable(msg) => ProviderError::ServiceUnavailable(msg.clone()),
        AnthropicError::InvalidRequest(msg) => ProviderError::Configuration(msg.clone()),
        AnthropicError::InvalidResponse(msg) => {
            ProviderError::Other(format!("Invalid response: {}", msg))
        }
        AnthropicError::Model(msg) => ProviderError::Model(msg.clone()),
        AnthropicError::Network(msg) => ProviderError::Network(msg.clone()),
        AnthropicError::Configuration(msg) => ProviderError::Configuration(msg.clone()),
        AnthropicError::Json(e) => ProviderError::Other(format!("JSON error: {}", e)),
        AnthropicError::Stream(msg) => ProviderError::Other(format!("Stream error: {}", msg)),
        AnthropicError::Other(msg) => ProviderError::Other(msg.clone()),
    }
}

// ===== AnthropicProvider =====

/// Anthropic direct API model provider
///
/// The provider handles all API interaction with Anthropic's Messages API.
/// Create one by passing a model that implements `AnthropicModel`:
///
/// ```ignore
/// use mixtape_core::{AnthropicProvider, ClaudeSonnet4_5};
///
/// let provider = AnthropicProvider::from_env(ClaudeSonnet4_5)?;
/// ```
pub struct AnthropicProvider {
    client: Anthropic,
    model_id: String,
    model_name: &'static str,
    max_context_tokens: usize,
    max_output_tokens: usize,
    max_tokens: i32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    thinking_config: Option<ThinkingConfig>,
    retry_config: RetryConfig,
    on_retry: Option<RetryCallback>,
}

impl Clone for AnthropicProvider {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            model_id: self.model_id.clone(),
            model_name: self.model_name,
            max_context_tokens: self.max_context_tokens,
            max_output_tokens: self.max_output_tokens,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            thinking_config: self.thinking_config,
            retry_config: self.retry_config.clone(),
            on_retry: self.on_retry.clone(),
        }
    }
}

impl AnthropicProvider {
    /// Create a new Anthropic provider using API key from environment
    ///
    /// Uses `ANTHROPIC_API_KEY` environment variable.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::{AnthropicProvider, ClaudeSonnet4_5};
    ///
    /// let provider = AnthropicProvider::from_env(ClaudeSonnet4_5)?;
    /// ```
    pub fn from_env(model: impl AnthropicModel) -> Result<Self, ProviderError> {
        let client = Anthropic::from_env().map_err(|e| classify_anthropic_error(&e))?;
        Ok(Self::with_client(client, model))
    }

    /// Create a new Anthropic provider with an explicit API key
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::{AnthropicProvider, ClaudeSonnet4_5};
    ///
    /// let provider = AnthropicProvider::new("sk-ant-...", ClaudeSonnet4_5)?;
    /// ```
    pub fn new(
        api_key: impl Into<String>,
        model: impl AnthropicModel,
    ) -> Result<Self, ProviderError> {
        let client = Anthropic::new(api_key).map_err(|e| classify_anthropic_error(&e))?;
        Ok(Self::with_client(client, model))
    }

    /// Create a provider with an existing client
    fn with_client(client: Anthropic, model: impl AnthropicModel) -> Self {
        Self {
            client,
            model_id: model.anthropic_id().to_string(),
            model_name: model.name(),
            max_context_tokens: model.max_context_tokens(),
            max_output_tokens: model.max_output_tokens(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking_config: None,
            retry_config: RetryConfig::default(),
            on_retry: None,
        }
    }

    /// Set the maximum number of tokens to generate per request
    pub fn with_max_tokens(mut self, max_tokens: i32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set the temperature (0.0 to 1.0)
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set top_p (0.0 to 1.0)
    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Set top_k (only sample from the top K options)
    pub fn with_top_k(mut self, top_k: u32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Enable extended thinking with specified token budget
    ///
    /// Extended thinking allows the model to reason through complex problems
    /// before providing a response. The budget_tokens parameter controls
    /// how many tokens the model can use for thinking (must be >= 1024).
    ///
    /// # Example
    /// ```ignore
    /// let provider = AnthropicProvider::from_env(ClaudeSonnet4_5)?
    ///     .with_thinking(4096);
    /// ```
    pub fn with_thinking(mut self, budget_tokens: u32) -> Self {
        self.thinking_config = Some(ThinkingConfig::Enabled { budget_tokens });
        self
    }

    /// Configure retry behavior for transient errors (throttling, rate limits)
    ///
    /// Default: 8 attempts with exponential backoff starting at 500ms, capped at 30s
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Set the maximum number of retry attempts for transient errors
    ///
    /// Default: 8
    pub fn with_max_retries(mut self, attempts: usize) -> Self {
        self.retry_config.max_attempts = attempts;
        self
    }

    /// Set the maximum delay between retries
    ///
    /// Default: 30 seconds
    pub fn with_max_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_config.max_delay_ms = delay.as_millis() as u64;
        self
    }

    /// Set the base delay for exponential backoff
    ///
    /// Default: 500ms
    pub fn with_base_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_config.base_delay_ms = delay.as_millis() as u64;
        self
    }

    /// Set a callback to be notified when retries occur
    ///
    /// # Example
    /// ```ignore
    /// let provider = AnthropicProvider::from_env(ClaudeSonnet4_5)?
    ///     .with_retry_callback(|info| {
    ///         eprintln!("⚠ Retry {}/{} in {:?}: {}",
    ///             info.attempt, info.max_attempts, info.delay, info.error);
    ///     });
    /// ```
    pub fn with_retry_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(RetryInfo) + Send + Sync + 'static,
    {
        self.on_retry = Some(Arc::new(callback));
        self
    }

    fn build_params(
        &self,
        messages: Vec<mixtape_anthropic_sdk::MessageParam>,
        tools: Vec<AnthropicTool>,
        system_prompt: Option<String>,
    ) -> MessageCreateParams {
        let mut builder =
            MessageCreateParams::builder(&self.model_id, self.max_tokens as u32).messages(messages);

        if let Some(system) = system_prompt {
            builder = builder.system(system);
        }
        if let Some(temp) = self.temperature {
            builder = builder.temperature(temp);
        }
        if let Some(top_p) = self.top_p {
            builder = builder.top_p(top_p);
        }
        if let Some(top_k) = self.top_k {
            builder = builder.top_k(top_k);
        }
        if !tools.is_empty() {
            builder = builder.tools(tools);
        }
        if let Some(config) = self.thinking_config {
            let sdk_config = match config {
                ThinkingConfig::Enabled { budget_tokens } => {
                    mixtape_anthropic_sdk::ThinkingConfig::enabled(budget_tokens)
                }
                ThinkingConfig::Disabled => mixtape_anthropic_sdk::ThinkingConfig::disabled(),
            };
            builder = builder.thinking_config(sdk_config);
        }

        builder.build()
    }
}

#[async_trait::async_trait]
impl ModelProvider for AnthropicProvider {
    fn name(&self) -> &str {
        self.model_name
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
        // Convert mixtape types to Anthropic types
        let anthropic_messages: Vec<mixtape_anthropic_sdk::MessageParam> = messages
            .iter()
            .map(to_anthropic_message)
            .collect::<Result<Vec<_>, _>>()?;

        let anthropic_tools: Vec<AnthropicTool> = tools
            .iter()
            .map(to_anthropic_tool)
            .collect::<Result<Vec<_>, _>>()?;

        let params = self.build_params(anthropic_messages, anthropic_tools, system_prompt);

        let response = retry_with_backoff(
            || async {
                self.client
                    .messages()
                    .create(params.clone())
                    .await
                    .map_err(|e| classify_anthropic_error(&e))
            },
            &self.retry_config,
            &self.on_retry,
        )
        .await?;

        // Convert Anthropic types back to mixtape types
        let message = from_anthropic_message(&response);
        let stop_reason = response
            .stop_reason
            .as_ref()
            .map(from_anthropic_stop_reason)
            .unwrap_or(StopReason::Unknown);

        // Extract token usage
        let usage = Some(TokenUsage {
            input_tokens: response.usage.input_tokens as usize,
            output_tokens: response.usage.output_tokens as usize,
        });

        Ok(ModelResponse {
            message,
            stop_reason,
            usage,
        })
    }

    async fn generate_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        // Convert mixtape types to Anthropic types
        let anthropic_messages: Vec<mixtape_anthropic_sdk::MessageParam> = messages
            .iter()
            .map(to_anthropic_message)
            .collect::<Result<Vec<_>, _>>()?;

        let anthropic_tools: Vec<AnthropicTool> = tools
            .iter()
            .map(to_anthropic_tool)
            .collect::<Result<Vec<_>, _>>()?;

        let params = self.build_params(anthropic_messages, anthropic_tools, system_prompt);

        let stream = retry_with_backoff(
            || async {
                self.client
                    .messages()
                    .stream(params.clone())
                    .await
                    .map_err(|e| classify_anthropic_error(&e))
            },
            &self.retry_config,
            &self.on_retry,
        )
        .await?;

        // Convert the SDK stream into our StreamEvent stream
        let event_stream = async_stream::stream! {
            let mut stream = stream;
            let mut tool_uses_in_progress: HashMap<usize, (String, String, String)> = HashMap::new();
            let mut input_tokens: usize = 0;
            let mut output_tokens: usize = 0;

            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(event) => match event {
                        MessageStreamEvent::MessageStart { message } => {
                            // Capture input tokens from the initial message
                            input_tokens = message.usage.input_tokens as usize;
                        }
                        MessageStreamEvent::ContentBlockStart {
                            index,
                            content_block: AnthropicContentBlock::ToolUse { id, name, .. },
                        } => {
                            tool_uses_in_progress.insert(index, (id, name, String::new()));
                        }
                        MessageStreamEvent::ContentBlockStart { .. } => {
                            // Ignore non-tool-use content blocks (e.g., text blocks)
                        }
                        MessageStreamEvent::ContentBlockDelta { index, delta } => {
                            match delta {
                                ContentBlockDelta::TextDelta { text } => {
                                    yield Ok(StreamEvent::TextDelta(text));
                                }
                                ContentBlockDelta::InputJsonDelta { partial_json } => {
                                    if let Some(entry) = tool_uses_in_progress.get_mut(&index) {
                                        entry.2.push_str(&partial_json);
                                    }
                                }
                                ContentBlockDelta::ThinkingDelta { thinking } => {
                                    yield Ok(StreamEvent::ThinkingDelta(thinking));
                                }
                                // Signature deltas are internal to thinking verification
                                ContentBlockDelta::SignatureDelta { .. } => {}
                            }
                        }
                        MessageStreamEvent::ContentBlockStop { index } => {
                            if let Some((id, name, input_json)) = tool_uses_in_progress.remove(&index) {
                                let input = serde_json::from_str(&input_json).unwrap_or_default();
                                yield Ok(StreamEvent::ToolUse(ToolUseBlock { id, name, input }));
                            }
                        }
                        MessageStreamEvent::MessageStop => {
                            // Don't emit another Stop - the real stop_reason
                            // was already sent via MessageDelta
                            break;
                        }
                        MessageStreamEvent::MessageDelta { delta, usage } => {
                            // Capture output tokens from delta
                            if let Some(u) = usage {
                                output_tokens = u.output_tokens as usize;
                            }
                            if let Some(stop_reason) = delta.stop_reason {
                                yield Ok(StreamEvent::Stop {
                                    stop_reason: from_anthropic_stop_reason(&stop_reason),
                                    usage: Some(TokenUsage { input_tokens, output_tokens }),
                                });
                            }
                        }
                        _ => {}
                    },
                    Err(e) => {
                        yield Err(classify_anthropic_error(&e));
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(event_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;

    /// Test model for unit tests
    struct TestModel {
        name: &'static str,
        anthropic_id: &'static str,
    }

    impl Model for TestModel {
        fn name(&self) -> &'static str {
            self.name
        }
        fn max_context_tokens(&self) -> usize {
            200_000
        }
        fn max_output_tokens(&self) -> usize {
            64_000
        }
        fn estimate_token_count(&self, text: &str) -> usize {
            text.len().div_ceil(4)
        }
    }

    impl AnthropicModel for TestModel {
        fn anthropic_id(&self) -> &'static str {
            self.anthropic_id
        }
    }

    #[test]
    fn test_builder_max_tokens() {
        // Skip if no API key available
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            return;
        }

        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::from_env(test_model)
            .unwrap()
            .with_max_tokens(2048);

        assert_eq!(provider.max_tokens, 2048);
    }

    #[test]
    fn test_builder_temperature() {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            return;
        }

        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::from_env(test_model)
            .unwrap()
            .with_temperature(0.7);

        assert_eq!(provider.temperature, Some(0.7));
    }

    #[test]
    fn test_builder_chaining() {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            return;
        }

        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::from_env(test_model)
            .unwrap()
            .with_max_tokens(1000)
            .with_temperature(0.5)
            .with_top_p(0.8)
            .with_top_k(50);

        assert_eq!(provider.model_id, "claude-test-model");
        assert_eq!(provider.model_name, "Test Model");
        assert_eq!(provider.max_tokens, 1000);
        assert_eq!(provider.temperature, Some(0.5));
        assert_eq!(provider.top_p, Some(0.8));
        assert_eq!(provider.top_k, Some(50));
    }

    #[test]
    fn test_provider_with_retry_config() {
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            return;
        }

        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let config = RetryConfig {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 5000,
        };

        let provider = AnthropicProvider::from_env(test_model)
            .unwrap()
            .with_retry_config(config);

        assert_eq!(provider.retry_config.max_attempts, 5);
        assert_eq!(provider.retry_config.base_delay_ms, 100);
        assert_eq!(provider.retry_config.max_delay_ms, 5000);
    }

    #[test]
    fn test_from_env_missing_key() {
        // Temporarily remove the API key
        let original = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");

        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let result = AnthropicProvider::from_env(test_model);
        assert!(result.is_err());

        // Restore if it was set
        if let Some(key) = original {
            std::env::set_var("ANTHROPIC_API_KEY", key);
        }
    }

    // ===== Tests that don't require API keys =====

    #[test]
    fn test_new_with_explicit_key() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        // Creating with an invalid key should succeed (connection fails later)
        let provider = AnthropicProvider::new("sk-ant-invalid-key", test_model).unwrap();
        assert_eq!(provider.model_id, "claude-test-model");
        assert_eq!(provider.model_name, "Test Model");
    }

    #[test]
    fn test_builder_with_thinking() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model)
            .unwrap()
            .with_thinking(4096);

        assert!(provider.thinking_config.is_some());
        match provider.thinking_config {
            Some(ThinkingConfig::Enabled { budget_tokens }) => {
                assert_eq!(budget_tokens, 4096);
            }
            _ => panic!("Expected Enabled thinking config"),
        }
    }

    #[test]
    fn test_builder_max_retries() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model)
            .unwrap()
            .with_max_retries(3);

        assert_eq!(provider.retry_config.max_attempts, 3);
    }

    #[test]
    fn test_builder_max_retry_delay() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model)
            .unwrap()
            .with_max_retry_delay(Duration::from_secs(60));

        assert_eq!(provider.retry_config.max_delay_ms, 60000);
    }

    #[test]
    fn test_builder_base_retry_delay() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model)
            .unwrap()
            .with_base_retry_delay(Duration::from_millis(250));

        assert_eq!(provider.retry_config.base_delay_ms, 250);
    }

    #[test]
    fn test_builder_retry_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };

        let callback_set = Arc::new(AtomicBool::new(false));
        let callback_clone = callback_set.clone();

        let provider = AnthropicProvider::new("sk-ant-test", test_model)
            .unwrap()
            .with_retry_callback(move |_| {
                callback_clone.store(true, Ordering::SeqCst);
            });

        // Just verify the callback was set
        assert!(provider.on_retry.is_some());
    }

    #[test]
    fn test_provider_clone() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model)
            .unwrap()
            .with_max_tokens(1024)
            .with_temperature(0.5)
            .with_top_p(0.9)
            .with_top_k(40)
            .with_thinking(2048);

        let cloned = provider.clone();

        assert_eq!(cloned.model_id, provider.model_id);
        assert_eq!(cloned.model_name, provider.model_name);
        assert_eq!(cloned.max_tokens, provider.max_tokens);
        assert_eq!(cloned.temperature, provider.temperature);
        assert_eq!(cloned.top_p, provider.top_p);
        assert_eq!(cloned.top_k, provider.top_k);
        assert_eq!(cloned.thinking_config, provider.thinking_config);
    }

    #[test]
    fn test_provider_default_values() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model).unwrap();

        // Default max tokens
        assert_eq!(provider.max_tokens, DEFAULT_MAX_TOKENS);
        // No temperature by default
        assert!(provider.temperature.is_none());
        // No top_p by default
        assert!(provider.top_p.is_none());
        // No top_k by default
        assert!(provider.top_k.is_none());
        // No thinking by default
        assert!(provider.thinking_config.is_none());
    }

    #[test]
    fn test_model_provider_trait_methods() {
        let test_model = TestModel {
            name: "Test Model",
            anthropic_id: "claude-test-model",
        };
        let provider = AnthropicProvider::new("sk-ant-test", test_model).unwrap();

        // Test ModelProvider trait methods
        assert_eq!(provider.name(), "Test Model");
        assert_eq!(provider.max_context_tokens(), 200_000);
        assert_eq!(provider.max_output_tokens(), 64_000);
    }

    // ===== Error Classification Tests =====

    #[test]
    fn test_classify_anthropic_error_authentication() {
        let err = mixtape_anthropic_sdk::AnthropicError::Authentication("Invalid API key".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Authentication(_)));
    }

    #[test]
    fn test_classify_anthropic_error_rate_limited() {
        let err = mixtape_anthropic_sdk::AnthropicError::RateLimited("Too many requests".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::RateLimited(_)));
    }

    #[test]
    fn test_classify_anthropic_error_service_unavailable() {
        let err = mixtape_anthropic_sdk::AnthropicError::ServiceUnavailable("Service down".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::ServiceUnavailable(_)));
    }

    #[test]
    fn test_classify_anthropic_error_invalid_request() {
        let err = mixtape_anthropic_sdk::AnthropicError::InvalidRequest("Bad input".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Configuration(_)));
    }

    #[test]
    fn test_classify_anthropic_error_model() {
        let err = mixtape_anthropic_sdk::AnthropicError::Model("Model not found".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Model(_)));
    }

    #[test]
    fn test_classify_anthropic_error_network() {
        let err = mixtape_anthropic_sdk::AnthropicError::Network("Connection refused".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Network(_)));
    }

    #[test]
    fn test_classify_anthropic_error_configuration() {
        let err = mixtape_anthropic_sdk::AnthropicError::Configuration("Missing config".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Configuration(_)));
    }

    #[test]
    fn test_classify_anthropic_error_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let err = mixtape_anthropic_sdk::AnthropicError::Json(json_err);
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Other(_)));
        if let ProviderError::Other(msg) = provider_err {
            assert!(msg.contains("JSON"));
        }
    }

    #[test]
    fn test_classify_anthropic_error_stream() {
        let err = mixtape_anthropic_sdk::AnthropicError::Stream("Stream error".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Other(_)));
    }

    #[test]
    fn test_classify_anthropic_error_other() {
        let err = mixtape_anthropic_sdk::AnthropicError::Other("Unknown error".into());
        let provider_err = classify_anthropic_error(&err);
        assert!(matches!(provider_err, ProviderError::Other(_)));
    }
}
