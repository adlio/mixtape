//! AWS Bedrock provider implementation

#[cfg(test)]
mod cancellation_tests;
mod chat_completions;
mod controls;
mod conversion;
mod openai_controls;
mod responses;
mod streaming;
mod telemetry;

pub use responses::{BedrockResponsesCache, BedrockResponsesCacheMode, BedrockResponsesProvider};

pub use chat_completions::BedrockChatCompletionsProvider;
pub use controls::{BedrockCacheTtl, BedrockJsonSchema, BedrockPromptCache, BedrockToolChoice};
pub use telemetry::{BedrockInvocation, InvocationOutcome, InvocationUsage};

use super::retry::{retry_with_backoff, RetryCallback, RetryConfig, RetryInfo};
use super::{ModelProvider, ProviderError, StreamEvent};
use crate::events::TokenUsage;
use crate::model::{BedrockModel, ModelResponse};
use crate::types::{Message, ThinkingConfig, ToolDefinition};
use aws_sdk_bedrockruntime::error::SdkError;
use aws_sdk_bedrockruntime::operation::RequestId;
use aws_sdk_bedrockruntime::{
    operation::converse::ConverseOutput,
    operation::converse_stream::ConverseStreamOutput as StreamOutputResult,
    types::{
        Message as BedrockMessage, SystemContentBlock, Tool as BedrockTool, ToolConfiguration,
    },
    Client,
};
use conversion::{
    from_bedrock_message, from_bedrock_stop_reason, json_to_document, to_bedrock_message,
    to_bedrock_tool,
};
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

// ===== Error Handling Helpers =====

/// Extract a user-friendly error message from an AWS SDK error
///
/// Walks the error chain to find the most meaningful message and
/// classifies it into the appropriate ProviderError variant.
fn classify_aws_error<E, R>(err: SdkError<E, R>) -> ProviderError
where
    E: StdError + 'static,
    R: std::fmt::Debug,
{
    // Collect all messages in the error chain
    let mut messages = Vec::new();
    let err_ref: &dyn StdError = &err;
    collect_error_messages(err_ref, &mut messages);

    // Look for the most specific/useful message (usually innermost)
    let root_message = messages
        .last()
        .cloned()
        .unwrap_or_else(|| "Unknown error".to_string());

    // Check for specific error patterns and classify appropriately
    let combined = messages.join(" ");

    classify_error_message(&combined, root_message)
}

/// Classify an error based on the combined error message text.
///
/// This matches patterns from AWS Bedrock error types:
/// - ThrottlingException (429): quota exceeded
/// - ServiceUnavailableException (503): service temporarily unavailable
/// - InternalServerException (500): internal server error
/// - AccessDeniedException (403): permission denied
/// - ValidationException (400): invalid input
/// - ModelTimeoutException (408): processing timeout
/// - ModelErrorException (424): model processing error
///
/// Reference: https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
/// SDK error format: https://docs.rs/aws-sdk-bedrockruntime/latest/aws_sdk_bedrockruntime/operation/converse/enum.ConverseError.html
fn classify_error_message(combined: &str, root_message: String) -> ProviderError {
    let lower = combined.to_lowercase();

    // Authentication errors (AccessDeniedException, credential issues)
    // https://docs.aws.amazon.com/bedrock/latest/APIReference/CommonErrors.html
    if lower.contains("unauthorized")
        || lower.contains("session token")
        || lower.contains("security token")
        || lower.contains("access denied")
        || lower.contains("accessdeniedexception")
        || lower.contains("invalid credentials")
        || lower.contains("expired token")
        || lower.contains("credentials")
    {
        ProviderError::Authentication(root_message)
    }
    // Rate limiting (ThrottlingException - HTTP 429)
    // Format: "ThrottlingException: Your request was denied due to exceeding account quotas"
    // https://docs.aws.amazon.com/bedrock/latest/userguide/troubleshooting-api-error-codes.html
    else if lower.contains("throttl")
        || lower.contains("too many requests")
        || lower.contains("rate exceeded")
        || lower.contains("limit exceeded")
    {
        ProviderError::RateLimited(root_message)
    }
    // Service unavailability (ServiceUnavailableException - HTTP 503, InternalServerException - HTTP 500)
    // Format: "ServiceUnavailableException: The service isn't currently available"
    // Format: "InternalServerException: An internal server error occurred"
    // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
    else if lower.contains("serviceunavailable")
        || lower.contains("service unavailable")
        || lower.contains("temporarily unavailable")
        || lower.contains("internalserver")
        || lower.contains("internal server error")
        || lower.contains("503")
        || lower.contains("500")
    {
        ProviderError::ServiceUnavailable(root_message)
    }
    // Model content/limit errors (not retryable)
    else if lower.contains("content filtered")
        || lower.contains("max tokens")
        || lower.contains("context length")
        || lower.contains("too many tokens")
    {
        ProviderError::Model(root_message)
    }
    // Network/timeout errors (ModelTimeoutException - HTTP 408, connection issues)
    // Format: "ModelTimeoutException: The request took too long to process"
    else if lower.contains("timeout")
        || lower.contains("modeltimeout")
        || lower.contains("connection")
        || lower.contains("network")
        || lower.contains("dns")
        || lower.contains("resolve")
    {
        ProviderError::Network(root_message)
    }
    // Configuration errors (ValidationException, ResourceNotFoundException, ModelNotReadyException)
    // Format: "ValidationException: The input fails to satisfy constraints"
    // Format: "ResourceNotFoundException: The specified resource ARN was not found"
    else if lower.contains("validationexception")
        || lower.contains("validation")
        || lower.contains("resourcenotfound")
        || lower.contains("not found")
        || lower.contains("modelnotready")
        || lower.contains("model")
    {
        ProviderError::Configuration(root_message)
    } else {
        ProviderError::Other(root_message)
    }
}

/// Recursively collect error messages from an error chain
fn collect_error_messages(err: &dyn StdError, messages: &mut Vec<String>) {
    let msg = err.to_string();
    // Skip generic wrapper messages that don't add useful info
    if !msg.is_empty()
        && !msg.starts_with("dispatch failure")
        && !msg.starts_with("connector error")
        && !msg.starts_with("unhandled error")
    {
        messages.push(msg);
    }

    if let Some(source) = err.source() {
        collect_error_messages(source, messages);
    }
}

/// Default maximum tokens to generate
const DEFAULT_MAX_TOKENS: i32 = 4096;

// Re-export InferenceProfile from model module for backwards compatibility
pub use crate::model::InferenceProfile;

// ===== Internal Request Type =====

/// Request parameters for converse API calls (using Bedrock types internally)
#[derive(Clone)]
struct ConverseRequest {
    model_id: String,
    messages: Vec<BedrockMessage>,
    max_tokens: i32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    thinking_config: Option<ThinkingConfig>,
    additional_fields: HashMap<String, serde_json::Value>,
    system: Vec<SystemContentBlock>,
    tools: Vec<BedrockTool>,
    tool_choice: Option<aws_sdk_bedrockruntime::types::ToolChoice>,
    output_config: Option<aws_sdk_bedrockruntime::types::OutputConfig>,
}

/// Trait for interacting with Bedrock API
/// This abstraction allows for testing without AWS credentials
#[async_trait::async_trait]
trait BedrockClient: Send + Sync {
    fn region(&self) -> Option<String> {
        None
    }

    /// Execute a non-streaming converse request
    async fn converse(&self, request: ConverseRequest) -> Result<ConverseOutput, ProviderError>;

    /// Execute a streaming converse request
    async fn converse_stream(
        &self,
        request: ConverseRequest,
    ) -> Result<StreamOutputResult, ProviderError>;
}

/// Production implementation wrapping the AWS SDK client
struct SdkBedrockClient {
    client: Client,
}

impl SdkBedrockClient {
    fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl BedrockClient for SdkBedrockClient {
    fn region(&self) -> Option<String> {
        self.client
            .config()
            .region()
            .map(|region| region.as_ref().to_owned())
    }

    async fn converse(&self, req: ConverseRequest) -> Result<ConverseOutput, ProviderError> {
        let mut request = self
            .client
            .converse()
            .model_id(req.model_id)
            .set_messages(Some(req.messages))
            .inference_config(
                aws_sdk_bedrockruntime::types::InferenceConfiguration::builder()
                    .max_tokens(req.max_tokens)
                    .set_temperature(req.temperature)
                    .set_top_p(req.top_p)
                    .build(),
            );

        if !req.system.is_empty() {
            request = request.set_system(Some(req.system));
        }
        request = request.set_output_config(req.output_config);

        if !req.tools.is_empty() {
            request = request.tool_config(
                ToolConfiguration::builder()
                    .set_tools(Some(req.tools))
                    .set_tool_choice(req.tool_choice)
                    .build()
                    .map_err(|e| ProviderError::Configuration(e.to_string()))?,
            );
        }

        // Build additional model request fields for top_k, thinking, and custom fields
        let additional_fields =
            build_additional_model_fields(req.top_k, req.thinking_config, &req.additional_fields);
        if let Some(fields) = additional_fields {
            request = request.additional_model_request_fields(fields);
        }

        request.send().await.map_err(classify_aws_error)
    }

    async fn converse_stream(
        &self,
        req: ConverseRequest,
    ) -> Result<StreamOutputResult, ProviderError> {
        let mut request = self
            .client
            .converse_stream()
            .model_id(req.model_id)
            .set_messages(Some(req.messages))
            .inference_config(
                aws_sdk_bedrockruntime::types::InferenceConfiguration::builder()
                    .max_tokens(req.max_tokens)
                    .set_temperature(req.temperature)
                    .set_top_p(req.top_p)
                    .build(),
            );

        if !req.system.is_empty() {
            request = request.set_system(Some(req.system));
        }
        request = request.set_output_config(req.output_config);

        if !req.tools.is_empty() {
            request = request.tool_config(
                ToolConfiguration::builder()
                    .set_tools(Some(req.tools))
                    .set_tool_choice(req.tool_choice)
                    .build()
                    .map_err(|e| ProviderError::Configuration(e.to_string()))?,
            );
        }

        // Build additional model request fields for top_k, thinking, and custom fields
        let additional_fields =
            build_additional_model_fields(req.top_k, req.thinking_config, &req.additional_fields);
        if let Some(fields) = additional_fields {
            request = request.additional_model_request_fields(fields);
        }

        request.send().await.map_err(classify_aws_error)
    }
}

/// Build additional model request fields for parameters not in InferenceConfiguration
fn build_additional_model_fields(
    top_k: Option<u32>,
    thinking_config: Option<ThinkingConfig>,
    additional_fields: &HashMap<String, serde_json::Value>,
) -> Option<aws_smithy_types::Document> {
    use aws_smithy_types::{Document, Number};

    let mut fields = HashMap::new();

    // Start with user-provided additional fields (can be overridden by specific params)
    for (key, value) in additional_fields {
        fields.insert(key.clone(), json_to_document(value));
    }

    // Add top_k if specified (overrides any user-provided top_k)
    if let Some(k) = top_k {
        fields.insert(
            "top_k".to_string(),
            Document::Number(Number::PosInt(k as u64)),
        );
    }

    // Explicitly disabling thinking is different from omitting the field.
    if let Some(config) = thinking_config {
        let value = match config {
            ThinkingConfig::Enabled { budget_tokens } => {
                serde_json::json!({"type": "enabled", "budget_tokens": budget_tokens})
            }
            ThinkingConfig::Disabled => serde_json::json!({"type": "disabled"}),
        };
        fields.insert("thinking".to_owned(), json_to_document(&value));
    }

    if fields.is_empty() {
        None
    } else {
        Some(Document::Object(fields))
    }
}

/// AWS Bedrock model provider
///
/// The provider handles all API interaction with AWS Bedrock.
/// Create one by passing a model that implements `BedrockModel`:
///
/// ```ignore
/// use mixtape_core::{BedrockProvider, ClaudeSonnet4_5};
///
/// let provider = BedrockProvider::new(ClaudeSonnet4_5).await;
/// ```
pub struct BedrockProvider {
    client: Arc<dyn BedrockClient>,
    base_model_id: String,
    inference_target: Option<String>,
    inference_profile: InferenceProfile,
    model_name: String,
    max_context_tokens: usize,
    max_output_tokens: usize,
    max_tokens: i32,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    thinking_config: Option<ThinkingConfig>,
    additional_fields: HashMap<String, serde_json::Value>,
    retry_config: RetryConfig,
    on_retry: Option<RetryCallback>,
    on_invocation: Option<Arc<dyn Fn(BedrockInvocation) + Send + Sync>>,
    prompt_cache: BedrockPromptCache,
    tool_choice: Option<BedrockToolChoice>,
    output_schema: Option<BedrockJsonSchema>,
}

impl BedrockProvider {
    /// Exact identifier dispatched to Bedrock. This is not provider-reported
    /// identity and does not establish the model behind an opaque profile ARN.
    pub fn effective_model_id(&self) -> String {
        self.inference_target
            .clone()
            .unwrap_or_else(|| self.inference_profile.apply_to(&self.base_model_id))
    }
}

impl Clone for BedrockProvider {
    fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            base_model_id: self.base_model_id.clone(),
            inference_target: self.inference_target.clone(),
            inference_profile: self.inference_profile,
            model_name: self.model_name.clone(),
            max_context_tokens: self.max_context_tokens,
            max_output_tokens: self.max_output_tokens,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            thinking_config: self.thinking_config,
            additional_fields: self.additional_fields.clone(),
            retry_config: self.retry_config.clone(),
            on_retry: self.on_retry.clone(),
            on_invocation: self.on_invocation.clone(),
            prompt_cache: self.prompt_cache.clone(),
            tool_choice: self.tool_choice.clone(),
            output_schema: self.output_schema.clone(),
        }
    }
}

impl BedrockProvider {
    /// Create a new Bedrock provider for the specified model
    ///
    /// Uses AWS credentials from the environment (via aws-config).
    ///
    /// Models that require cross-region inference (Claude 4/4.5, Nova 2 Lite)
    /// automatically use `InferenceProfile::Global`. Other models default to
    /// single-region invocation. Use `with_inference_profile()` to override.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::{BedrockProvider, ClaudeSonnet4_5, NovaMicro};
    ///
    /// // Claude 4.5 automatically uses Global inference profile
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5).await?;
    ///
    /// // Nova Micro uses single-region (no inference profile)
    /// let provider = BedrockProvider::new(NovaMicro).await?;
    /// ```
    pub async fn new(model: impl BedrockModel) -> Result<Self, ProviderError> {
        let sdk_config = aws_config::load_from_env().await;
        let client = Client::new(&sdk_config);
        Ok(Self {
            client: Arc::new(SdkBedrockClient::new(client)),
            base_model_id: model.bedrock_id().to_string(),
            inference_target: None,
            inference_profile: model.default_inference_profile(),
            model_name: model.name().to_owned(),
            max_context_tokens: model.max_context_tokens(),
            max_output_tokens: model.max_output_tokens(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking_config: None,
            additional_fields: HashMap::new(),
            retry_config: RetryConfig::default(),
            on_retry: None,
            on_invocation: None,
            prompt_cache: BedrockPromptCache::default(),
            tool_choice: None,
            output_schema: None,
        })
    }

    /// Create a new Bedrock provider with a custom AWS SDK client
    pub fn with_client(client: Client, model: impl BedrockModel) -> Self {
        Self {
            client: Arc::new(SdkBedrockClient::new(client)),
            base_model_id: model.bedrock_id().to_string(),
            inference_target: None,
            inference_profile: model.default_inference_profile(),
            model_name: model.name().to_owned(),
            max_context_tokens: model.max_context_tokens(),
            max_output_tokens: model.max_output_tokens(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking_config: None,
            additional_fields: HashMap::new(),
            retry_config: RetryConfig::default(),
            on_retry: None,
            on_invocation: None,
            prompt_cache: BedrockPromptCache::default(),
            tool_choice: None,
            output_schema: None,
        }
    }

    /// Create a new Bedrock provider with a custom client implementation (for testing)
    #[cfg(test)]
    fn with_bedrock_client(client: Arc<dyn BedrockClient>, model: impl BedrockModel) -> Self {
        Self {
            client,
            base_model_id: model.bedrock_id().to_string(),
            inference_target: None,
            inference_profile: model.default_inference_profile(),
            model_name: model.name().to_owned(),
            max_context_tokens: model.max_context_tokens(),
            max_output_tokens: model.max_output_tokens(),
            max_tokens: DEFAULT_MAX_TOKENS,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking_config: None,
            additional_fields: HashMap::new(),
            retry_config: RetryConfig::default(),
            on_retry: None,
            on_invocation: None,
            prompt_cache: BedrockPromptCache::default(),
            tool_choice: None,
            output_schema: None,
        }
    }

    /// Configure cross-region inference profile for higher throughput and reliability
    ///
    /// Inference profiles enable automatic load balancing across multiple AWS regions.
    /// This overrides the model's default inference profile.
    ///
    /// Note: Models that require inference profiles (Claude 4/4.5, Nova 2 Lite)
    /// automatically default to `InferenceProfile::Global`. Use this method to
    /// change to a regional profile (US, EU, APAC) for data residency requirements.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::{BedrockProvider, ClaudeSonnet4_5, InferenceProfile};
    ///
    /// // Use US inference profile for US data residency
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5).await
    ///     .with_inference_profile(InferenceProfile::US);
    ///
    /// // Or use EU for European data residency
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5).await
    ///     .with_inference_profile(InferenceProfile::EU);
    /// ```
    pub fn with_inference_profile(mut self, profile: InferenceProfile) -> Self {
        self.inference_profile = profile;
        self
    }

    /// Use an exact inference-profile ID or ARN without adding a geographic
    /// prefix. The caller must verify that an opaque ARN routes to this model;
    /// Mixtape does not claim to resolve that mapping. Credentials are unchanged.
    pub fn with_inference_target(
        mut self,
        target: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let target = target.into();
        crate::model::RuntimeBedrockModel::new(
            &self.model_name,
            &target,
            self.max_context_tokens,
            self.max_output_tokens,
        )?;
        let qualified_model = ["us.", "eu.", "apac.", "au.", "in.", "jp.", "global."]
            .iter()
            .find_map(|prefix| target.strip_prefix(prefix))
            .unwrap_or(&target);
        let base = crate::models::bedrock_model_descriptor(&self.base_model_id)
            .map(|model| model.model_id)
            .unwrap_or(&self.base_model_id);
        if !target.starts_with("arn:") && qualified_model != base {
            return Err(ProviderError::Configuration(
                "Inference target names a different model".into(),
            ));
        }
        self.inference_target = Some(target);
        Ok(self)
    }

    /// Place explicit Converse cache checkpoints. Cache eligibility and actual
    /// hits are determined by Bedrock; no cache use is inferred from this setting.
    pub fn with_prompt_cache(mut self, cache: BedrockPromptCache) -> Self {
        self.prompt_cache = cache;
        self
    }

    /// Select automatic, disabled, or forced tools where the model supports it.
    pub fn with_tool_choice(mut self, choice: BedrockToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Request native JSON schema output on a model with a verified contract.
    /// Applications must still validate the response before persisting a summary.
    pub fn with_output_schema(mut self, schema: BedrockJsonSchema) -> Self {
        self.output_schema = Some(schema);
        self
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
    ///
    /// Note: This is passed via `additionalModelRequestFields` as Bedrock's
    /// InferenceConfiguration doesn't natively support top_k.
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
    /// Note: This is passed via `additionalModelRequestFields` for Claude models.
    ///
    /// # Example
    /// ```ignore
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5).await
    ///     .with_thinking(4096);
    /// ```
    pub fn with_thinking(mut self, budget_tokens: u32) -> Self {
        self.thinking_config = Some(ThinkingConfig::Enabled { budget_tokens });
        self
    }

    /// Use Claude's adaptive thinking contract. Effort, if desired, is configured
    /// separately through `with_thinking_effort`; omission keeps model defaults.
    pub fn with_adaptive_thinking(mut self) -> Self {
        self.thinking_config = None;
        self.additional_fields
            .insert("thinking".into(), serde_json::json!({"type": "adaptive"}));
        self
    }

    /// Explicitly disable thinking. Omitting the thinking field does not disable
    /// it on models such as Opus 5 and Sonnet 5.
    pub fn with_disabled_thinking(mut self) -> Self {
        self.thinking_config = Some(ThinkingConfig::Disabled);
        self
    }

    /// Set Claude's output effort. Supported values depend on the model; invalid
    /// or unverified combinations are rejected before making a request.
    pub fn with_thinking_effort(mut self, effort: impl Into<String>) -> Self {
        let config = self
            .additional_fields
            .entry("output_config".into())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(object) = config.as_object_mut() {
            object.insert("effort".into(), serde_json::Value::String(effort.into()));
        } else {
            // Keep invalid custom configuration intact so validation reports it.
            // Never silently replace another caller's output configuration.
        }
        self
    }

    /// Validate local ceilings and known model/API constraints without credentials
    /// or network I/O. Account access and application data eligibility are separate.
    pub fn validate_configuration(&self) -> Result<(), ProviderError> {
        let invalid = |message: &str| ProviderError::Configuration(message.into());
        if self.max_tokens <= 0 || self.max_tokens as usize > self.max_output_tokens {
            return Err(invalid(
                "max_tokens must be positive and within the configured output ceiling",
            ));
        }
        for value in [self.temperature, self.top_p].into_iter().flatten() {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(invalid(
                    "Sampling values must be finite and between 0 and 1",
                ));
            }
        }
        if self.top_k == Some(0) {
            return Err(invalid("top_k must be positive"));
        }
        let target = self.effective_model_id();
        let descriptor = crate::models::bedrock_model_descriptor(&self.base_model_id)
            .or_else(|| crate::models::bedrock_model_descriptor(&target));
        if let Some(model) = descriptor {
            if model.conversation_api != crate::models::BedrockConversationApi::Converse {
                return Err(invalid("This model requires a Bedrock Chat Completions adapter for multi-turn reasoning; Converse is not selected as a fallback"));
            }
            if model.requires_inference_profile && target == model.model_id {
                return Err(invalid("This model requires an explicit inference-profile ID or ARN; choose its routing before calling it"));
            }
        }
        let fields = build_additional_model_fields(
            self.top_k,
            self.thinking_config,
            &self.additional_fields,
        )
        .as_ref()
        .map(conversion::document_to_json)
        .unwrap_or_else(|| serde_json::json!({}));
        let thinking = fields.get("thinking");
        let output_config = fields.get("output_config");
        if output_config.is_some_and(|value| !value.is_object()) {
            return Err(invalid("output_config must be an object"));
        }
        let model_id = descriptor
            .map(|value| value.model_id)
            .unwrap_or(&self.base_model_id);
        controls::validate(
            model_id,
            &self.prompt_cache,
            self.tool_choice.as_ref(),
            self.output_schema.as_ref(),
            thinking,
        )?;
        if model_id == "anthropic.claude-opus-4-7"
            && (self.temperature.is_some() || self.top_p.is_some() || fields.get("top_k").is_some())
        {
            return Err(invalid(
                "Opus 4.7 does not accept temperature, top_p, or top_k",
            ));
        }
        if model_id.starts_with("anthropic.claude-fable-") {
            if fields.get("top_k").is_some() || self.temperature.is_some_and(|value| value != 1.0) {
                return Err(invalid(
                    "Fable requires temperature 1 or omitted and does not accept top_k",
                ));
            }
            if let Some(top_p) = self.top_p {
                let supported = if model_id == "anthropic.claude-fable-5-1" {
                    top_p == 0.99 && self.temperature.is_none()
                } else {
                    (0.99..1.0).contains(&top_p)
                };
                if !supported {
                    return Err(invalid("Unsupported Fable sampling combination"));
                }
            }
        }
        if thinking.is_none()
            && output_config
                .and_then(|value| value.get("effort"))
                .is_none()
        {
            return Ok(());
        }
        if !model_id.starts_with("anthropic.") {
            return Err(invalid("Claude thinking controls require a known Claude model ID; an opaque profile ARN needs a verified model mapping"));
        }
        let mode = match thinking {
            Some(value) => Some(
                value
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| invalid("thinking requires a type"))?,
            ),
            None => None,
        };
        let adaptive = matches!(
            model_id,
            "anthropic.claude-opus-5"
                | "anthropic.claude-sonnet-5"
                | "anthropic.claude-opus-4-7"
                | "anthropic.claude-opus-4-6-v1"
                | "anthropic.claude-sonnet-4-6"
                | "anthropic.claude-fable-5"
                | "anthropic.claude-fable-5-1"
        );
        let adaptive_only = model_id.starts_with("anthropic.claude-fable-");
        let manual = matches!(
            model_id,
            "anthropic.claude-3-7-sonnet-20250219-v1:0"
                | "anthropic.claude-opus-4-20250514-v1:0"
                | "anthropic.claude-opus-4-1-20250805-v1:0"
                | "anthropic.claude-opus-4-5-20251101-v1:0"
                | "anthropic.claude-opus-4-6-v1"
                | "anthropic.claude-sonnet-4-20250514-v1:0"
                | "anthropic.claude-sonnet-4-5-20250929-v1:0"
                | "anthropic.claude-sonnet-4-6"
                | "anthropic.claude-haiku-4-5-20251001-v1:0"
        );
        if mode.is_some() && !adaptive && !manual {
            return Err(invalid("Thinking controls are not verified for this model"));
        }
        match mode {
            Some("adaptive") if !adaptive => {
                return Err(invalid("Adaptive thinking is not verified for this model"))
            }
            Some("disabled") if adaptive_only => {
                return Err(invalid("This model does not support disabling thinking"))
            }
            Some("enabled") => {
                if adaptive
                    && !matches!(
                        model_id,
                        "anthropic.claude-opus-4-6-v1" | "anthropic.claude-sonnet-4-6"
                    )
                {
                    return Err(invalid(
                        "This model does not support manual thinking budgets",
                    ));
                }
                let budget = thinking
                    .and_then(|value| value.get("budget_tokens"))
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| invalid("Manual thinking requires budget_tokens"))?;
                if budget < 1024 || budget >= self.max_tokens as u64 {
                    return Err(invalid(
                        "Thinking budget must be at least 1024 and smaller than max_tokens",
                    ));
                }
            }
            Some("adaptive" | "disabled") | None => {}
            Some(_) => return Err(invalid("Unknown thinking mode")),
        }
        if matches!(mode, Some("adaptive" | "disabled"))
            && thinking.is_some_and(|value| value.get("budget_tokens").is_some())
        {
            return Err(invalid(
                "Adaptive and disabled thinking do not accept budget_tokens",
            ));
        }
        if thinking.is_some_and(|value| value.get("effort").is_some()) {
            return Err(invalid("Effort belongs in output_config, not thinking"));
        }
        if let Some(effort) = output_config.and_then(|value| value.get("effort")) {
            let effort = effort
                .as_str()
                .ok_or_else(|| invalid("Effort must be a string"))?;
            let supported = match effort {
                "low" | "medium" | "high" => {
                    adaptive || model_id == "anthropic.claude-opus-4-5-20251101-v1:0"
                }
                "xhigh" => matches!(
                    model_id,
                    "anthropic.claude-opus-5"
                        | "anthropic.claude-opus-4-6-v1"
                        | "anthropic.claude-fable-5-1"
                ),
                "max" => matches!(
                    model_id,
                    "anthropic.claude-opus-5"
                        | "anthropic.claude-opus-4-6-v1"
                        | "anthropic.claude-sonnet-4-6"
                        | "anthropic.claude-fable-5-1"
                ),
                _ => false,
            };
            if !supported {
                return Err(invalid("Effort level is not verified for this model"));
            }
            if model_id == "anthropic.claude-opus-5"
                && mode == Some("disabled")
                && matches!(effort, "xhigh" | "max")
            {
                return Err(invalid(
                    "Opus 5 effort cannot exceed high when thinking is disabled",
                ));
            }
        }
        Ok(())
    }

    /// Enable 1M token context window for Claude Sonnet 4/4.5 (relies on Anthropic beta feature)
    ///
    /// Expands the context window from 200K to 1 million tokens.
    ///
    /// # Supported models
    ///
    /// - `ClaudeSonnet4` (`anthropic.claude-sonnet-4-20250514-v1:0`)
    /// - `ClaudeSonnet4_5` (`anthropic.claude-sonnet-4-5-20250929-v1:0`)
    ///
    /// # Regional availability
    ///
    /// Available in US West (Oregon), US East (N. Virginia), and US East (Ohio).
    /// Requires cross-region inference profile (`InferenceProfile::US` or `InferenceProfile::Global`).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5)
    ///     .await?
    ///     .with_inference_profile(InferenceProfile::US)
    ///     .with_1m_context();
    /// ```
    pub fn with_1m_context(mut self) -> Self {
        const BETA_KEY: &str = "anthropic_beta";
        const CONTEXT_1M: &str = "context-1m-2025-08-07";

        // Check if already enabled (idempotent)
        if let Some(existing) = self.additional_fields.get(BETA_KEY) {
            if let Some(arr) = existing.as_array() {
                if arr.iter().any(|v| v.as_str() == Some(CONTEXT_1M)) {
                    return self;
                }
            }
        }

        // Add the beta feature
        let betas = self
            .additional_fields
            .entry(BETA_KEY.to_string())
            .or_insert_with(|| serde_json::json!([]));

        if let Some(arr) = betas.as_array_mut() {
            arr.push(serde_json::json!(CONTEXT_1M));
        }

        self.max_context_tokens = 1_000_000;
        self
    }

    /// Add a custom field to `additionalModelRequestFields`
    ///
    /// Use this for model-specific parameters not covered by the standard builder methods.
    /// Fields set here are merged with `top_k` and `thinking` if those are also configured.
    ///
    /// # Example
    /// ```ignore
    /// let provider = BedrockProvider::new(SomeModel).await
    ///     .with_additional_field("custom_param", serde_json::json!(42))
    ///     .with_additional_field("nested", serde_json::json!({"key": "value"}));
    /// ```
    pub fn with_additional_field(
        mut self,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Self {
        self.additional_fields.insert(key.into(), value);
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
    /// let provider = BedrockProvider::new(ClaudeSonnet4_5).await
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

    /// Observe provider calls using metadata-only records. A completed protocol
    /// response still needs application-level summary validation. The callback
    /// should return promptly and must not panic, including on cancellation.
    /// Dropping a request or stream before completion produces one Cancelled
    /// record; SDK-internal retry counts remain unknown.
    pub fn with_invocation_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(BedrockInvocation) + Send + Sync + 'static,
    {
        self.on_invocation = Some(Arc::new(callback));
        self
    }

    fn invocation(&self, api: &'static str) -> BedrockInvocation {
        BedrockInvocation::new(
            &self.base_model_id,
            self.effective_model_id(),
            api,
            self.client.region(),
        )
    }

    fn guard(&self, api: &'static str) -> telemetry::InvocationGuard {
        telemetry::InvocationGuard::new(self.invocation(api), self.on_invocation.clone(), None)
    }

    fn build_request(
        &self,
        messages: Vec<BedrockMessage>,
        tools: Vec<BedrockTool>,
        system_prompt: Option<String>,
    ) -> Result<ConverseRequest, ProviderError> {
        let mut request = ConverseRequest {
            model_id: self.effective_model_id(),
            messages,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            thinking_config: self.thinking_config,
            additional_fields: self.additional_fields.clone(),
            system: system_prompt
                .into_iter()
                .map(SystemContentBlock::Text)
                .collect(),
            tools,
            tool_choice: None,
            output_config: None,
        };
        controls::apply(
            &mut request,
            &self.prompt_cache,
            self.tool_choice.as_ref(),
            self.output_schema.as_ref(),
        )?;
        Ok(request)
    }
}

#[async_trait::async_trait]
impl ModelProvider for BedrockProvider {
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
        self.validate_configuration()?;
        // Convert mixtape types to Bedrock types
        let bedrock_messages: Vec<BedrockMessage> = messages
            .iter()
            .map(to_bedrock_message)
            .collect::<Result<Vec<_>, _>>()?;

        let bedrock_tools: Vec<BedrockTool> = tools
            .iter()
            .map(to_bedrock_tool)
            .collect::<Result<Vec<_>, _>>()?;

        let request = self.build_request(bedrock_messages, bedrock_tools, system_prompt)?;
        let mut guard = self.guard("converse");
        let response = retry_with_backoff(
            || {
                guard.attempts.fetch_add(1, Ordering::Relaxed);
                self.client.converse(request.clone())
            },
            &self.retry_config,
            &self.on_retry,
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                guard.finish(InvocationOutcome::Failed);
                return Err(error);
            }
        };
        guard.record.request_id = response.request_id().map(str::to_owned);
        guard.record.provider_stop_reason = Some(response.stop_reason.as_str().to_owned());
        guard.record.usage = response.usage.as_ref().map(InvocationUsage::from_bedrock);
        guard.record.provider_latency_ms = response
            .metrics
            .as_ref()
            .and_then(|metrics| u64::try_from(metrics.latency_ms).ok());
        let result = (|| {
            let output = response
                .output
                .as_ref()
                .ok_or_else(|| ProviderError::Model("No output from model".into()))?;
            let aws_sdk_bedrockruntime::types::ConverseOutput::Message(message) = output else {
                return Err(ProviderError::Model(
                    "Unexpected output type from model".into(),
                ));
            };
            let usage = response
                .usage
                .as_ref()
                .map(|usage| {
                    Ok::<_, ProviderError>(TokenUsage {
                        input_tokens: usize::try_from(usage.input_tokens).map_err(|_| {
                            ProviderError::Model("Negative input token count".into())
                        })?,
                        output_tokens: usize::try_from(usage.output_tokens).map_err(|_| {
                            ProviderError::Model("Negative output token count".into())
                        })?,
                    })
                })
                .transpose()?;
            Ok(ModelResponse {
                message: from_bedrock_message(message),
                stop_reason: from_bedrock_stop_reason(&response.stop_reason),
                usage,
            })
        })();
        let outcome = if result.is_ok() {
            telemetry::stop_outcome(response.stop_reason.as_str())
        } else {
            InvocationOutcome::Failed
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
        self.validate_configuration()?;
        // Convert mixtape types to Bedrock types
        let bedrock_messages: Vec<BedrockMessage> = messages
            .iter()
            .map(to_bedrock_message)
            .collect::<Result<Vec<_>, _>>()?;

        let bedrock_tools: Vec<BedrockTool> = tools
            .iter()
            .map(to_bedrock_tool)
            .collect::<Result<Vec<_>, _>>()?;

        let request = self.build_request(bedrock_messages, bedrock_tools, system_prompt)?;
        let mut guard = self.guard("converse_stream");
        let output = retry_with_backoff(
            || {
                guard.attempts.fetch_add(1, Ordering::Relaxed);
                self.client.converse_stream(request.clone())
            },
            &self.retry_config,
            &self.on_retry,
        )
        .await;
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                guard.finish(InvocationOutcome::Failed);
                return Err(error);
            }
        };
        guard.record.request_id = output.request_id().map(str::to_owned);
        let mut stream = output.stream;
        let events = async_stream::try_stream! {
            while let Some(event) = stream.recv().await.map_err(classify_aws_error)? {
                yield event;
            }
        };
        Ok(streaming::observe_stream(Box::pin(events), guard))
    }
}

#[cfg(test)]
mod tests {
    #![allow(dead_code)] // Test infrastructure may have unused fields/methods for future use

    use super::*;
    use crate::model::Model;
    use crate::models::{ClaudeSonnet4_5, NovaMicro};
    use std::sync::Mutex;

    /// Test model for unit tests
    struct TestModel {
        name: &'static str,
        bedrock_id: &'static str,
    }

    impl Model for TestModel {
        fn name(&self) -> &'static str {
            self.name
        }
        fn max_context_tokens(&self) -> usize {
            128_000
        }
        fn max_output_tokens(&self) -> usize {
            4_096
        }
        fn estimate_token_count(&self, text: &str) -> usize {
            text.len().div_ceil(4)
        }
    }

    impl BedrockModel for TestModel {
        fn bedrock_id(&self) -> &'static str {
            self.bedrock_id
        }
    }

    const TEST_MODEL: TestModel = TestModel {
        name: "Test Model",
        bedrock_id: "test.model-v1:0",
    };

    /// Test implementation of BedrockClient that returns canned responses
    struct TestBedrockClient {
        converse_responses: Mutex<Vec<Result<ConverseOutput, ProviderError>>>,
        stream_responses: Mutex<Vec<Result<StreamOutputResult, ProviderError>>>,
        converse_call_count: Mutex<usize>,
        stream_call_count: Mutex<usize>,
    }

    impl TestBedrockClient {
        fn new() -> Self {
            Self {
                converse_responses: Mutex::new(Vec::new()),
                stream_responses: Mutex::new(Vec::new()),
                converse_call_count: Mutex::new(0),
                stream_call_count: Mutex::new(0),
            }
        }

        fn with_converse_response(self, response: Result<ConverseOutput, ProviderError>) -> Self {
            self.converse_responses.lock().unwrap().push(response);
            self
        }

        fn with_stream_response(self, response: Result<StreamOutputResult, ProviderError>) -> Self {
            self.stream_responses.lock().unwrap().push(response);
            self
        }
    }

    #[async_trait::async_trait]
    impl BedrockClient for TestBedrockClient {
        async fn converse(&self, _req: ConverseRequest) -> Result<ConverseOutput, ProviderError> {
            *self.converse_call_count.lock().unwrap() += 1;
            self.converse_responses
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| {
                    Err(ProviderError::Other(
                        "No mock response configured".to_string(),
                    ))
                })
        }

        async fn converse_stream(
            &self,
            _req: ConverseRequest,
        ) -> Result<StreamOutputResult, ProviderError> {
            *self.stream_call_count.lock().unwrap() += 1;
            self.stream_responses
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| {
                    Err(ProviderError::Other(
                        "No mock response configured".to_string(),
                    ))
                })
        }
    }

    #[test]
    fn test_builder_max_tokens() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_max_tokens(2048);

        assert_eq!(provider.max_tokens, 2048);
    }

    #[test]
    fn test_builder_temperature() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_temperature(0.7);

        assert_eq!(provider.temperature, Some(0.7));
    }

    #[test]
    fn test_builder_top_p() {
        let client = TestBedrockClient::new();
        let provider =
            BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL).with_top_p(0.9);

        assert_eq!(provider.top_p, Some(0.9));
    }

    #[test]
    fn test_builder_chaining() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_max_tokens(1000)
            .with_temperature(0.5)
            .with_top_p(0.8);

        assert_eq!(provider.base_model_id, "test.model-v1:0");
        assert_eq!(provider.model_name, "Test Model");
        assert_eq!(provider.max_tokens, 1000);
        assert_eq!(provider.temperature, Some(0.5));
        assert_eq!(provider.top_p, Some(0.8));
    }

    #[test]
    fn test_name_from_model() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), ClaudeSonnet4_5);

        assert_eq!(provider.name(), "Claude Sonnet 4.5");
    }

    #[test]
    fn test_name_nova_micro() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), NovaMicro);

        assert_eq!(provider.name(), "Nova Micro");
    }

    #[tokio::test]
    async fn test_generate_provider_error() {
        let client = TestBedrockClient::new()
            .with_converse_response(Err(ProviderError::Other("API Error".to_string())));
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL);

        let result = provider
            .generate(vec![Message::user("Hi")], vec![], None)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API Error"));
    }

    #[tokio::test]
    async fn test_clone_provider() {
        let client = TestBedrockClient::new();
        let provider =
            BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL).with_max_tokens(500);

        let cloned = provider.clone();
        assert_eq!(cloned.base_model_id, "test.model-v1:0");
        assert_eq!(cloned.max_tokens, 500);
    }

    // ===== Error Classification Tests =====
    //
    // These tests verify that AWS Bedrock error messages are correctly classified.
    // Error patterns are based on the AWS SDK for Rust and Bedrock API documentation:
    // - https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
    // - https://docs.rs/aws-sdk-bedrockruntime/latest/aws_sdk_bedrockruntime/operation/converse/enum.ConverseError.html
    // - https://docs.aws.amazon.com/bedrock/latest/userguide/troubleshooting-api-error-codes.html

    #[test]
    fn stream_sdk_errors_keep_service_categories() {
        let unavailable: SdkError<std::io::Error, ()> = SdkError::service_error(
            std::io::Error::other("ServiceUnavailableException: synthetic outage"),
            (),
        );
        assert!(matches!(
            classify_aws_error(unavailable),
            ProviderError::ServiceUnavailable(_)
        ));
        let denied: SdkError<std::io::Error, ()> = SdkError::service_error(
            std::io::Error::other("AccessDeniedException: synthetic permission failure"),
            (),
        );
        assert!(matches!(
            classify_aws_error(denied),
            ProviderError::Authentication(_)
        ));
        let invalid: SdkError<std::io::Error, ()> = SdkError::service_error(
            std::io::Error::other("ValidationException: synthetic schema failure"),
            (),
        );
        assert!(matches!(
            classify_aws_error(invalid),
            ProviderError::Configuration(_)
        ));
    }

    #[test]
    fn test_classify_throttling_exception() {
        // ThrottlingException format from AWS SDK:
        // https://docs.rs/aws-sdk-bedrockruntime/latest/src/aws_sdk_bedrockruntime/types/error/_throttling_exception.rs.html
        let err = classify_error_message(
            "ThrottlingException: Your request was denied due to exceeding the account quotas for Amazon Bedrock",
            "Your request was denied".into(),
        );
        assert!(
            matches!(err, ProviderError::RateLimited(_)),
            "ThrottlingException should map to RateLimited, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_throttling_exception_minimal() {
        // Sometimes the SDK returns just the exception name
        let err = classify_error_message("ThrottlingException", "ThrottlingException".into());
        assert!(matches!(err, ProviderError::RateLimited(_)));
    }

    #[test]
    fn test_classify_throttling_too_many_requests() {
        // Alternative throttling message format
        // https://repost.aws/questions/QUGwlQKp95SiOAEi1D9KTeZA
        let err = classify_error_message(
            "Too many requests, please wait before trying again",
            "Too many requests".into(),
        );
        assert!(matches!(err, ProviderError::RateLimited(_)));
    }

    #[test]
    fn test_classify_service_unavailable_exception() {
        // ServiceUnavailableException (HTTP 503)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
        let err = classify_error_message(
            "ServiceUnavailableException: The service isn't currently available",
            "The service isn't currently available".into(),
        );
        assert!(
            matches!(err, ProviderError::ServiceUnavailable(_)),
            "ServiceUnavailableException should map to ServiceUnavailable, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_internal_server_exception() {
        // InternalServerException (HTTP 500)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
        let err = classify_error_message(
            "InternalServerException: An internal server error occurred",
            "An internal server error occurred".into(),
        );
        assert!(
            matches!(err, ProviderError::ServiceUnavailable(_)),
            "InternalServerException should map to ServiceUnavailable, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_access_denied_exception() {
        // AccessDeniedException (HTTP 403)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/CommonErrors.html
        let err = classify_error_message(
            "AccessDeniedException: You don't have permission to access this resource",
            "You don't have permission".into(),
        );
        assert!(
            matches!(err, ProviderError::Authentication(_)),
            "AccessDeniedException should map to Authentication, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_expired_token() {
        // Expired credentials
        let err = classify_error_message(
            "The security token included in the request is expired",
            "security token expired".into(),
        );
        assert!(matches!(err, ProviderError::Authentication(_)));
    }

    #[test]
    fn test_classify_validation_exception() {
        // ValidationException (HTTP 400)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
        let err = classify_error_message(
            "ValidationException: The input fails to satisfy the constraints specified by Amazon Bedrock",
            "The input fails to satisfy constraints".into(),
        );
        assert!(
            matches!(err, ProviderError::Configuration(_)),
            "ValidationException should map to Configuration, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_resource_not_found_exception() {
        // ResourceNotFoundException (HTTP 404)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
        let err = classify_error_message(
            "ResourceNotFoundException: The specified resource ARN was not found",
            "resource not found".into(),
        );
        assert!(
            matches!(err, ProviderError::Configuration(_)),
            "ResourceNotFoundException should map to Configuration, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_model_timeout_exception() {
        // ModelTimeoutException (HTTP 408)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
        let err = classify_error_message(
            "ModelTimeoutException: The request took too long to process",
            "request took too long".into(),
        );
        assert!(
            matches!(err, ProviderError::Network(_)),
            "ModelTimeoutException should map to Network (retryable), got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_model_not_ready_exception() {
        // ModelNotReadyException (HTTP 429)
        // https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
        let err = classify_error_message(
            "ModelNotReadyException: The model is not ready to serve inference requests",
            "model not ready".into(),
        );
        assert!(
            matches!(err, ProviderError::Configuration(_)),
            "ModelNotReadyException should map to Configuration, got {:?}",
            err
        );
    }

    #[test]
    fn test_classify_connection_error() {
        // Network-level errors from the SDK
        let err = classify_error_message(
            "dispatch failure connector error: connection refused",
            "connection refused".into(),
        );
        assert!(matches!(err, ProviderError::Network(_)));
    }

    #[test]
    fn test_classify_dns_error() {
        // DNS resolution failure
        let err = classify_error_message(
            "error trying to connect: dns error: failed to lookup address",
            "dns error".into(),
        );
        assert!(matches!(err, ProviderError::Network(_)));
    }

    #[test]
    fn test_classify_unknown_error() {
        // Unrecognized errors should fall through to Other
        let err = classify_error_message(
            "SomeNewException: An unexpected error occurred",
            "An unexpected error".into(),
        );
        assert!(
            matches!(err, ProviderError::Other(_)),
            "Unknown errors should map to Other, got {:?}",
            err
        );
    }

    // ===== RetryConfig Builder Tests =====

    #[test]
    fn test_provider_with_retry_config() {
        let client = TestBedrockClient::new();
        let config = RetryConfig {
            max_attempts: 5,
            base_delay_ms: 100,
            max_delay_ms: 5000,
        };

        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_retry_config(config);

        assert_eq!(provider.retry_config.max_attempts, 5);
        assert_eq!(provider.retry_config.base_delay_ms, 100);
        assert_eq!(provider.retry_config.max_delay_ms, 5000);
    }

    #[test]
    fn test_provider_with_max_retries() {
        let client = TestBedrockClient::new();
        let provider =
            BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL).with_max_retries(3);

        assert_eq!(provider.retry_config.max_attempts, 3);
    }

    #[test]
    fn test_provider_with_max_retry_delay() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_max_retry_delay(Duration::from_secs(10));

        assert_eq!(provider.retry_config.max_delay_ms, 10_000);
    }

    #[test]
    fn test_provider_with_base_retry_delay() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_base_retry_delay(Duration::from_millis(200));

        assert_eq!(provider.retry_config.base_delay_ms, 200);
    }

    // ===== Inference Profile Default Tests =====

    #[test]
    fn test_provider_uses_model_default_inference_profile() {
        let client = TestBedrockClient::new();
        // ClaudeSonnet4_5 should get Global profile automatically
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), ClaudeSonnet4_5);
        assert_eq!(provider.inference_profile, InferenceProfile::Global);
    }

    #[test]
    fn test_provider_uses_none_for_older_models() {
        let client = TestBedrockClient::new();
        // NovaMicro should get None profile
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), NovaMicro);
        assert_eq!(provider.inference_profile, InferenceProfile::None);
    }

    // ===== Additional Builder Tests =====

    #[test]
    fn test_builder_top_k() {
        let client = TestBedrockClient::new();
        let provider =
            BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL).with_top_k(50);
        assert_eq!(provider.top_k, Some(50));
    }

    #[test]
    fn test_builder_thinking() {
        let client = TestBedrockClient::new();
        let provider =
            BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL).with_thinking(4096);

        match provider.thinking_config {
            Some(ThinkingConfig::Enabled { budget_tokens }) => {
                assert_eq!(budget_tokens, 4096);
            }
            _ => panic!("Expected Enabled thinking config"),
        }
    }

    #[test]
    fn test_builder_additional_field() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_additional_field("custom_param", serde_json::json!(42))
            .with_additional_field("nested", serde_json::json!({"key": "value"}));

        assert_eq!(provider.additional_fields.len(), 2);
        assert_eq!(provider.additional_fields["custom_param"], 42);
        assert_eq!(
            provider.additional_fields["nested"],
            serde_json::json!({"key": "value"})
        );
    }

    #[test]
    fn test_builder_override_inference_profile() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), ClaudeSonnet4_5)
            .with_inference_profile(InferenceProfile::US);

        assert_eq!(provider.inference_profile, InferenceProfile::US);
    }

    #[test]
    fn test_builder_retry_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let client = TestBedrockClient::new();
        let callback_set = Arc::new(AtomicBool::new(false));
        let callback_clone = callback_set.clone();

        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_retry_callback(move |_| {
                callback_clone.store(true, Ordering::SeqCst);
            });

        assert!(provider.on_retry.is_some());
    }

    #[test]
    fn test_provider_default_values() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL);

        assert_eq!(provider.max_tokens, DEFAULT_MAX_TOKENS);
        assert!(provider.temperature.is_none());
        assert!(provider.top_p.is_none());
        assert!(provider.top_k.is_none());
        assert!(provider.thinking_config.is_none());
        assert!(provider.additional_fields.is_empty());
    }

    #[test]
    fn test_model_provider_trait() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL);

        assert_eq!(provider.name(), "Test Model");
        assert_eq!(provider.max_context_tokens(), 128_000);
        assert_eq!(provider.max_output_tokens(), 4_096);
    }

    #[test]
    fn test_effective_model_id_no_profile() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), NovaMicro);

        // NovaMicro has InferenceProfile::None so model ID should be unchanged
        assert_eq!(provider.effective_model_id(), NovaMicro.bedrock_id());
    }

    #[test]
    fn test_effective_model_id_with_global_profile() {
        let client = TestBedrockClient::new();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), ClaudeSonnet4_5);

        // ClaudeSonnet4_5 uses Global profile, so ID should be wrapped
        let effective_id = provider.effective_model_id();
        assert!(effective_id.contains(ClaudeSonnet4_5.bedrock_id()));
    }

    // ===== Additional Error Classification Tests =====

    #[test]
    fn test_classify_content_filtered() {
        let err = classify_error_message(
            "content filtered by safety mechanism",
            "content filtered".into(),
        );
        assert!(matches!(err, ProviderError::Model(_)));
    }

    #[test]
    fn test_classify_max_tokens_exceeded() {
        let err = classify_error_message(
            "Request exceeds max tokens allowed",
            "max tokens exceeded".into(),
        );
        assert!(matches!(err, ProviderError::Model(_)));
    }

    #[test]
    fn test_classify_context_length_exceeded() {
        let err = classify_error_message(
            "Context length exceeded for this model",
            "context length exceeded".into(),
        );
        assert!(matches!(err, ProviderError::Model(_)));
    }

    #[test]
    fn test_classify_rate_limit_exceeded() {
        let err =
            classify_error_message("Rate limit exceeded for account", "limit exceeded".into());
        assert!(matches!(err, ProviderError::RateLimited(_)));
    }

    #[test]
    fn test_classify_http_503() {
        let err = classify_error_message(
            "HTTP Status Code: 503 Service Temporarily Unavailable",
            "503".into(),
        );
        assert!(matches!(err, ProviderError::ServiceUnavailable(_)));
    }

    #[test]
    fn test_classify_http_500() {
        let err =
            classify_error_message("HTTP Status Code: 500 Internal Server Error", "500".into());
        assert!(matches!(err, ProviderError::ServiceUnavailable(_)));
    }

    #[test]
    fn test_classify_session_token_invalid() {
        let err = classify_error_message(
            "The session token used for this request is invalid",
            "session token invalid".into(),
        );
        assert!(matches!(err, ProviderError::Authentication(_)));
    }

    #[test]
    fn test_classify_credentials_missing() {
        let err = classify_error_message("No credentials configured", "credentials missing".into());
        assert!(matches!(err, ProviderError::Authentication(_)));
    }

    // ===== Runtime configuration regression tests =====

    fn configured(model_id: &str) -> BedrockProvider {
        let model =
            crate::model::RuntimeBedrockModel::new("Configured", model_id, 1_000_000, 16_000)
                .unwrap();
        BedrockProvider::with_bedrock_client(Arc::new(TestBedrockClient::new()), model)
    }

    #[test]
    fn runtime_request_uses_exact_target() {
        let target = "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/example";
        let provider = configured(target).with_inference_profile(InferenceProfile::Global);
        assert_eq!(
            provider
                .build_request(vec![], vec![], None)
                .unwrap()
                .model_id,
            target
        );
    }

    #[test]
    fn adaptive_and_disabled_fields_are_distinct_from_omission() {
        let provider = configured("us.anthropic.claude-opus-5")
            .with_adaptive_thinking()
            .with_thinking_effort("high");
        assert!(provider.validate_configuration().is_ok());
        let fields = build_additional_model_fields(
            provider.top_k,
            provider.thinking_config,
            &provider.additional_fields,
        )
        .unwrap();
        let fields = conversion::document_to_json(&fields);
        assert_eq!(fields["thinking"]["type"], "adaptive");
        assert_eq!(fields["output_config"]["effort"], "high");
        assert!(fields["thinking"].get("budget_tokens").is_none());
        let disabled = provider.with_disabled_thinking();
        assert!(disabled.validate_configuration().is_ok());
        let fields = build_additional_model_fields(
            None,
            disabled.thinking_config,
            &disabled.additional_fields,
        )
        .unwrap();
        assert_eq!(
            conversion::document_to_json(&fields)["thinking"]["type"],
            "disabled"
        );
        assert!(build_additional_model_fields(None, None, &HashMap::new()).is_none());
    }

    #[test]
    fn unsupported_modes_limits_and_api_combinations_are_rejected() {
        assert!(configured("us.anthropic.claude-opus-5")
            .with_thinking(2048)
            .validate_configuration()
            .is_err());
        assert!(configured("us.anthropic.claude-opus-5")
            .with_disabled_thinking()
            .with_thinking_effort("max")
            .validate_configuration()
            .is_err());
        assert!(configured("us.anthropic.claude-haiku-4-5-20251001-v1:0")
            .with_adaptive_thinking()
            .validate_configuration()
            .is_err());
        assert!(configured("us.anthropic.claude-fable-5-1")
            .with_disabled_thinking()
            .validate_configuration()
            .is_err());
        assert!(configured("us.moonshotai.kimi-k3")
            .validate_configuration()
            .is_err());
        assert!(configured("anthropic.claude-opus-5")
            .validate_configuration()
            .is_err());
        assert!(configured("us.anthropic.claude-opus-5")
            .with_max_tokens(0)
            .validate_configuration()
            .is_err());
        assert!(configured("us.anthropic.claude-opus-5")
            .with_max_tokens(16_001)
            .validate_configuration()
            .is_err());
        assert!(configured("test.model")
            .with_temperature(f32::NAN)
            .validate_configuration()
            .is_err());
        assert!(configured("test.model")
            .with_top_p(f32::INFINITY)
            .validate_configuration()
            .is_err());
    }

    #[tokio::test]
    async fn invalid_configuration_never_dispatches_to_either_api() {
        let client = Arc::new(TestBedrockClient::new());
        let provider =
            BedrockProvider::with_bedrock_client(client.clone(), TEST_MODEL).with_max_tokens(-1);
        assert!(provider
            .generate(vec![Message::user("fixture")], vec![], None)
            .await
            .is_err());
        assert!(provider
            .generate_stream(vec![Message::user("fixture")], vec![], None)
            .await
            .is_err());
        assert_eq!(*client.converse_call_count.lock().unwrap(), 0);
        assert_eq!(*client.stream_call_count.lock().unwrap(), 0);
    }

    #[test]
    fn model_and_exact_profile_target_remain_separate() {
        let arn = "arn:aws:bedrock:us-east-1:123456789012:application-inference-profile/example";
        let provider = configured("anthropic.claude-opus-5")
            .with_inference_target(arn)
            .unwrap()
            .with_adaptive_thinking();
        assert!(provider.validate_configuration().is_ok());
        assert_eq!(
            provider
                .build_request(vec![], vec![], None)
                .unwrap()
                .model_id,
            arn
        );
        assert_eq!(
            provider.invocation("converse").requested_model,
            "anthropic.claude-opus-5"
        );
        assert!(configured("anthropic.claude-opus-5")
            .with_inference_target("us.anthropic.claude-sonnet-5")
            .is_err());
    }

    #[tokio::test]
    async fn invocation_records_measure_requests_without_fabricating_identity() {
        let response = ConverseOutput::builder()
            .output(aws_sdk_bedrockruntime::types::ConverseOutput::Message(
                BedrockMessage::builder()
                    .role(aws_sdk_bedrockruntime::types::ConversationRole::Assistant)
                    .content(aws_sdk_bedrockruntime::types::ContentBlock::Text(
                        "fixture answer".into(),
                    ))
                    .build()
                    .unwrap(),
            ))
            .stop_reason(aws_sdk_bedrockruntime::types::StopReason::EndTurn)
            .usage(
                aws_sdk_bedrockruntime::types::TokenUsage::builder()
                    .input_tokens(10)
                    .output_tokens(4)
                    .total_tokens(14)
                    .cache_read_input_tokens(2)
                    .cache_write_input_tokens(0)
                    .build()
                    .unwrap(),
            )
            .metrics(
                aws_sdk_bedrockruntime::types::ConverseMetrics::builder()
                    .latency_ms(12)
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        // This fixture queue pops from the end: one retry, then a good response.
        let client = TestBedrockClient::new()
            .with_converse_response(Ok(response))
            .with_converse_response(Err(ProviderError::RateLimited("fixture only".into())));
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_max_retries(2)
            .with_base_retry_delay(Duration::ZERO)
            .with_invocation_callback(move |record| sink.lock().unwrap().push(record));
        provider
            .generate(
                vec![Message::user("not-for-operational-logs")],
                vec![],
                None,
            )
            .await
            .unwrap();
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.requested_model, TEST_MODEL.bedrock_id());
        assert_eq!(record.dispatched_target, TEST_MODEL.bedrock_id());
        assert_eq!(record.api, "converse");
        assert_eq!(record.attempts, 2);
        assert_eq!(record.outcome, InvocationOutcome::Completed);
        assert_eq!(record.provider_latency_ms, Some(12));
        assert!(record.provider_reported_model.is_none());
        assert!(record.request_id.is_none());
        assert!(record.sdk_retry_count.is_none());
        assert_eq!(
            record.usage.as_ref().unwrap().cache_read_input_tokens,
            Some(2)
        );
        assert_eq!(
            record.usage.as_ref().unwrap().cache_write_input_tokens,
            Some(0)
        );
        let json = serde_json::to_string(record).unwrap();
        assert!(!json.contains("not-for-operational-logs"));
        assert!(!json.contains("fixture answer"));
    }

    #[tokio::test]
    async fn failed_invocations_are_recorded_without_invented_usage() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let sink = records.clone();
        let client = TestBedrockClient::new().with_converse_response(Err(ProviderError::Model(
            "fixture-only-failure-detail".into(),
        )));
        let provider = BedrockProvider::with_bedrock_client(Arc::new(client), TEST_MODEL)
            .with_invocation_callback(move |record| sink.lock().unwrap().push(record));
        assert!(provider
            .generate(vec![Message::user("fixture")], vec![], None)
            .await
            .is_err());
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, InvocationOutcome::Failed);
        assert!(records[0].usage.is_none());
        assert!(!serde_json::to_string(&records[0])
            .unwrap()
            .contains("fixture-only-failure-detail"));
    }

    // ===== build_additional_model_fields Tests =====

    #[test]
    fn test_build_additional_fields_empty() {
        let result = build_additional_model_fields(None, None, &HashMap::new());
        assert!(result.is_none());
    }

    #[test]
    fn test_build_additional_fields_top_k_only() {
        let result = build_additional_model_fields(Some(50), None, &HashMap::new());
        assert!(result.is_some());
        // The result should contain top_k
        if let Some(aws_smithy_types::Document::Object(fields)) = result {
            assert!(fields.contains_key("top_k"));
        }
    }

    #[test]
    fn test_build_additional_fields_thinking_only() {
        let result = build_additional_model_fields(
            None,
            Some(ThinkingConfig::Enabled {
                budget_tokens: 4096,
            }),
            &HashMap::new(),
        );
        assert!(result.is_some());
        if let Some(aws_smithy_types::Document::Object(fields)) = result {
            assert!(fields.contains_key("thinking"));
        }
    }

    #[test]
    fn test_build_additional_fields_custom_only() {
        let mut custom = HashMap::new();
        custom.insert("custom_key".to_string(), serde_json::json!("custom_value"));

        let result = build_additional_model_fields(None, None, &custom);
        assert!(result.is_some());
        if let Some(aws_smithy_types::Document::Object(fields)) = result {
            assert!(fields.contains_key("custom_key"));
        }
    }

    #[test]
    fn test_build_additional_fields_all() {
        let mut custom = HashMap::new();
        custom.insert("extra".to_string(), serde_json::json!(123));

        let result = build_additional_model_fields(
            Some(40),
            Some(ThinkingConfig::Enabled {
                budget_tokens: 2048,
            }),
            &custom,
        );
        assert!(result.is_some());
        if let Some(aws_smithy_types::Document::Object(fields)) = result {
            assert!(fields.contains_key("top_k"));
            assert!(fields.contains_key("thinking"));
            assert!(fields.contains_key("extra"));
        }
    }
}
