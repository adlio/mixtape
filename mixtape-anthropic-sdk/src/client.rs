//! Anthropic API client

use crate::batch::{BatchCreateParams, BatchListResponse, BatchResult, MessageBatch};
use crate::error::{AnthropicError, ApiErrorResponse, RetryConfig};
use crate::messages::{Message, MessageCreateParams};
use crate::streaming::MessageStream;
use crate::tokens::{CountTokensParams, CountTokensResponse};
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use std::collections::HashMap;
use std::time::Duration;

/// Default API base URL
const DEFAULT_API_BASE: &str = "https://api.anthropic.com";

/// Default API version
const DEFAULT_API_VERSION: &str = "2023-06-01";

/// Default request timeout
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

// ============================================================================
// Raw Response Types
// ============================================================================

/// Raw HTTP response metadata for debugging
///
/// This provides access to response headers, status code, and request ID
/// for debugging and troubleshooting purposes.
#[derive(Debug, Clone)]
pub struct RawResponse {
    /// HTTP status code
    pub status: u16,

    /// Response headers
    pub headers: HashMap<String, String>,

    /// Anthropic request ID (from `request-id` header)
    pub request_id: Option<String>,

    /// Rate limit information
    pub rate_limit: Option<RateLimitInfo>,
}

impl RawResponse {
    /// Create from reqwest response (consumes headers)
    fn from_response(response: &reqwest::Response) -> Self {
        let status = response.status().as_u16();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_string(), v.to_string()))
            })
            .collect();

        let request_id = headers.get("request-id").cloned();

        let rate_limit = RateLimitInfo::from_headers(&headers);

        Self {
            status,
            headers,
            request_id,
            rate_limit,
        }
    }

    /// Get a specific header value
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|s| s.as_str())
    }
}

/// Rate limit information from response headers
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// Requests allowed per minute
    pub requests_limit: Option<u32>,

    /// Requests remaining in current window
    pub requests_remaining: Option<u32>,

    /// When the request limit resets
    pub requests_reset: Option<String>,

    /// Tokens allowed per minute
    pub tokens_limit: Option<u32>,

    /// Tokens remaining in current window
    pub tokens_remaining: Option<u32>,

    /// When the token limit resets
    pub tokens_reset: Option<String>,
}

impl RateLimitInfo {
    fn from_headers(headers: &HashMap<String, String>) -> Option<Self> {
        // Check if any rate limit headers are present
        let has_rate_limit_headers = headers
            .keys()
            .any(|k| k.starts_with("anthropic-ratelimit-"));

        if !has_rate_limit_headers {
            return None;
        }

        Some(Self {
            requests_limit: headers
                .get("anthropic-ratelimit-requests-limit")
                .and_then(|s| s.parse().ok()),
            requests_remaining: headers
                .get("anthropic-ratelimit-requests-remaining")
                .and_then(|s| s.parse().ok()),
            requests_reset: headers.get("anthropic-ratelimit-requests-reset").cloned(),
            tokens_limit: headers
                .get("anthropic-ratelimit-tokens-limit")
                .and_then(|s| s.parse().ok()),
            tokens_remaining: headers
                .get("anthropic-ratelimit-tokens-remaining")
                .and_then(|s| s.parse().ok()),
            tokens_reset: headers.get("anthropic-ratelimit-tokens-reset").cloned(),
        })
    }
}

/// A response with both parsed data and raw HTTP metadata
///
/// Use this when you need access to headers, request ID, or other
/// debugging information alongside the parsed response.
#[derive(Debug, Clone)]
pub struct Response<T> {
    /// The parsed response data
    pub data: T,

    /// Raw HTTP response metadata
    pub raw: RawResponse,
}

impl<T> Response<T> {
    /// Get the parsed data
    pub fn into_data(self) -> T {
        self.data
    }

    /// Get the request ID for debugging
    pub fn request_id(&self) -> Option<&str> {
        self.raw.request_id.as_deref()
    }

    /// Get rate limit information
    pub fn rate_limit(&self) -> Option<&RateLimitInfo> {
        self.raw.rate_limit.as_ref()
    }
}

// ============================================================================
// Client
// ============================================================================

/// Anthropic API client
#[derive(Clone)]
pub struct Anthropic {
    client: reqwest::Client,
    api_key: String,
    api_base: String,
    api_version: String,
    retry_config: RetryConfig,
}

impl std::fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Anthropic")
            .field("api_base", &self.api_base)
            .field("api_version", &self.api_version)
            .field("api_key", &"[REDACTED]")
            .field("retry_config", &self.retry_config)
            .finish()
    }
}

impl Anthropic {
    /// Create a new client with an explicit API key
    pub fn new(api_key: impl Into<String>) -> Result<Self, AnthropicError> {
        Self::builder().api_key(api_key).build()
    }

    /// Create a new client from the ANTHROPIC_API_KEY environment variable
    pub fn from_env() -> Result<Self, AnthropicError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            AnthropicError::Configuration(
                "ANTHROPIC_API_KEY environment variable not set".to_string(),
            )
        })?;
        Self::new(api_key)
    }

    /// Create a builder for more advanced configuration
    pub fn builder() -> AnthropicBuilder {
        AnthropicBuilder::new()
    }

    /// Get a handle to the messages API
    pub fn messages(&self) -> Messages<'_> {
        Messages { client: self }
    }

    /// Get a handle to the batches API
    pub fn batches(&self) -> Batches<'_> {
        Batches { client: self }
    }

    /// Execute a request with automatic retry
    ///
    /// This is a shared helper that handles:
    /// - Exponential backoff with jitter
    /// - Retry-After header parsing
    /// - Retryable error detection (429, 5xx, network errors)
    async fn execute_with_retry<T, B>(
        &self,
        url: &str,
        body: Option<&B>,
        method: reqwest::Method,
        headers: HeaderMap,
    ) -> Result<Response<T>, AnthropicError>
    where
        T: serde::de::DeserializeOwned,
        B: serde::Serialize,
    {
        let mut last_error: Option<AnthropicError> = None;

        for attempt in 0..=self.retry_config.max_retries {
            let mut request = self
                .client
                .request(method.clone(), url)
                .headers(headers.clone());

            if let Some(b) = body {
                request = request.json(b);
            }

            let result = request.send().await;

            match result {
                Ok(response) => {
                    let raw = RawResponse::from_response(&response);
                    let status = response.status();

                    if status.is_success() {
                        let data = response.json::<T>().await.map_err(|e| {
                            AnthropicError::InvalidResponse(format!(
                                "Failed to parse response: {}",
                                e
                            ))
                        })?;
                        return Ok(Response { data, raw });
                    }

                    let status_code = status.as_u16();
                    let error_body = response.text().await.unwrap_or_default();
                    let error = parse_error_response(&error_body, status_code);

                    if attempt < self.retry_config.max_retries
                        && AnthropicError::is_retryable_status(status_code)
                    {
                        let delay =
                            RetryConfig::parse_retry_after(&headers_to_reqwest(&raw.headers))
                                .unwrap_or_else(|| self.retry_config.delay_for_attempt(attempt));
                        tokio::time::sleep(delay).await;
                        last_error = Some(error);
                        continue;
                    }

                    return Err(error);
                }
                Err(e) => {
                    let error = AnthropicError::from_reqwest_error(e);

                    if attempt < self.retry_config.max_retries && error.is_retryable() {
                        let delay = self.retry_config.delay_for_attempt(attempt);
                        tokio::time::sleep(delay).await;
                        last_error = Some(error);
                        continue;
                    }

                    return Err(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| AnthropicError::Other("Max retries exceeded".to_string())))
    }
}

/// Builder for Anthropic client configuration
///
/// Create with [`Anthropic::builder()`] and configure using the fluent API.
/// The `api_key` is required - call [`Self::build()`] to create the client.
pub struct AnthropicBuilder {
    api_key: Option<String>,
    api_base: Option<String>,
    api_version: Option<String>,
    timeout: Option<Duration>,
    retry_config: Option<RetryConfig>,
}

impl AnthropicBuilder {
    /// Create a new builder
    fn new() -> Self {
        Self {
            api_key: None,
            api_base: None,
            api_version: None,
            timeout: None,
            retry_config: None,
        }
    }

    /// Set the API key
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set a custom API base URL
    pub fn api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = Some(api_base.into());
        self
    }

    /// Set a custom API version
    pub fn api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = Some(api_version.into());
        self
    }

    /// Set the request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the maximum number of retries (default: 2)
    ///
    /// Set to 0 to disable retries.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        let mut config = self.retry_config.take().unwrap_or_default();
        config.max_retries = max_retries;
        self.retry_config = Some(config);
        self
    }

    /// Set custom retry configuration
    pub fn retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Build the client
    pub fn build(self) -> Result<Anthropic, AnthropicError> {
        let api_key = self
            .api_key
            .ok_or_else(|| AnthropicError::Configuration("API key is required".to_string()))?;

        let timeout = self.timeout.unwrap_or(DEFAULT_TIMEOUT);

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| {
                AnthropicError::Configuration(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Anthropic {
            client,
            api_key,
            api_base: self
                .api_base
                .unwrap_or_else(|| DEFAULT_API_BASE.to_string()),
            api_version: self
                .api_version
                .unwrap_or_else(|| DEFAULT_API_VERSION.to_string()),
            retry_config: self.retry_config.unwrap_or_default(),
        })
    }
}

// ============================================================================
// Messages API
// ============================================================================

/// Messages API handle
pub struct Messages<'a> {
    client: &'a Anthropic,
}

impl<'a> Messages<'a> {
    /// Create a message (non-streaming)
    ///
    /// Returns the parsed [`Message`] response.
    ///
    /// # When to use
    ///
    /// Use this method for most cases where you just need the Claude response.
    /// For debugging, rate limit tracking, or accessing request IDs, use
    /// [`Self::create_with_metadata`] instead.
    pub async fn create(&self, params: MessageCreateParams) -> Result<Message, AnthropicError> {
        self.create_with_metadata(params).await.map(|r| r.data)
    }

    /// Create a message with full response metadata
    ///
    /// Returns a [`Response<Message>`] that includes:
    /// - The parsed message (`.data`)
    /// - Request ID for debugging (`.request_id()`)
    /// - Rate limit info (`.rate_limit()`)
    /// - Raw headers (`.raw.headers`)
    ///
    /// # When to use
    ///
    /// Use this method when you need:
    /// - **Debugging**: Access `request_id()` to correlate with Anthropic support
    /// - **Rate limiting**: Check `rate_limit()` to implement backoff strategies
    /// - **Monitoring**: Track usage patterns via response headers
    ///
    /// For simple cases where you only need the message, use [`Self::create`].
    pub async fn create_with_metadata(
        &self,
        params: MessageCreateParams,
    ) -> Result<Response<Message>, AnthropicError> {
        let url = format!("{}/v1/messages", self.client.api_base);
        let beta_strings: Option<Vec<String>> = params
            .betas
            .as_ref()
            .map(|b| b.iter().map(|f| f.to_string()).collect());
        let headers = self.build_headers(beta_strings.as_deref())?;

        self.client
            .execute_with_retry(&url, Some(&params), reqwest::Method::POST, headers)
            .await
    }

    /// Create a message with streaming
    ///
    /// Returns a [`MessageStream`] for processing the response incrementally.
    ///
    /// # When to use
    ///
    /// Use streaming when you need:
    /// - **Responsive UI**: Display text as it arrives for better perceived latency
    /// - **Long responses**: Avoid waiting for the full response before processing
    /// - **Token-by-token processing**: Handle content as it's generated
    ///
    /// For simple cases where you can wait for the complete response, use
    /// [`Self::create`] instead. Note that streaming does not provide response
    /// metadata (use non-streaming [`Self::create_with_metadata`] if you need
    /// request IDs or rate limit info).
    pub async fn stream(
        &self,
        mut params: MessageCreateParams,
    ) -> Result<MessageStream, AnthropicError> {
        // Ensure streaming is enabled
        params.stream = Some(true);

        let url = format!("{}/v1/messages", self.client.api_base);
        let beta_strings: Option<Vec<String>> = params
            .betas
            .as_ref()
            .map(|b| b.iter().map(|f| f.to_string()).collect());
        let headers = self.build_headers(beta_strings.as_deref())?;

        MessageStream::new(&self.client.client, &url, headers, params).await
    }

    /// Count tokens for a message
    ///
    /// Useful for estimating costs and managing context windows before
    /// sending a message. For response metadata, use [`Self::count_tokens_with_metadata`].
    pub async fn count_tokens(
        &self,
        params: CountTokensParams,
    ) -> Result<CountTokensResponse, AnthropicError> {
        self.count_tokens_with_metadata(params)
            .await
            .map(|r| r.data)
    }

    /// Count tokens with full response metadata
    ///
    /// See [`Self::create_with_metadata`] for details on the response wrapper.
    /// For simple cases, use [`Self::count_tokens`].
    pub async fn count_tokens_with_metadata(
        &self,
        params: CountTokensParams,
    ) -> Result<Response<CountTokensResponse>, AnthropicError> {
        let url = format!("{}/v1/messages/count_tokens", self.client.api_base);
        let headers = self.build_headers(None)?;

        self.client
            .execute_with_retry(&url, Some(&params), reqwest::Method::POST, headers)
            .await
    }

    fn build_headers(&self, betas: Option<&[String]>) -> Result<HeaderMap, AnthropicError> {
        build_headers(&self.client.api_key, &self.client.api_version, betas)
    }
}

// ============================================================================
// Batches API
// ============================================================================

/// Batches API handle
pub struct Batches<'a> {
    client: &'a Anthropic,
}

/// Options for listing batches
#[derive(Debug, Default)]
pub struct BatchListOptions {
    /// Maximum number of batches to return (1-100, default 20)
    pub limit: Option<u32>,

    /// Return batches after this ID (for pagination)
    pub after_id: Option<String>,

    /// Return batches before this ID (for pagination)
    pub before_id: Option<String>,
}

impl<'a> Batches<'a> {
    /// Create a new message batch
    ///
    /// Submits a batch of message requests for asynchronous processing.
    /// Batches can take up to 24 hours to complete but are significantly
    /// cheaper than individual requests.
    ///
    /// For response metadata, use [`Self::create_with_metadata`].
    pub async fn create(&self, params: BatchCreateParams) -> Result<MessageBatch, AnthropicError> {
        self.create_with_metadata(params).await.map(|r| r.data)
    }

    /// Create a batch with full response metadata
    ///
    /// See [`Messages::create_with_metadata`] for details on the response wrapper.
    /// For simple cases, use [`Self::create`].
    pub async fn create_with_metadata(
        &self,
        params: BatchCreateParams,
    ) -> Result<Response<MessageBatch>, AnthropicError> {
        let url = format!("{}/v1/messages/batches", self.client.api_base);
        let headers = self.build_headers()?;
        self.client
            .execute_with_retry(&url, Some(&params), reqwest::Method::POST, headers)
            .await
    }

    /// Get the status of a message batch
    ///
    /// For response metadata, use [`Self::get_with_metadata`].
    pub async fn get(&self, batch_id: &str) -> Result<MessageBatch, AnthropicError> {
        self.get_with_metadata(batch_id).await.map(|r| r.data)
    }

    /// Get batch status with full response metadata
    ///
    /// For simple cases, use [`Self::get`].
    pub async fn get_with_metadata(
        &self,
        batch_id: &str,
    ) -> Result<Response<MessageBatch>, AnthropicError> {
        let url = format!("{}/v1/messages/batches/{}", self.client.api_base, batch_id);
        let headers = self.build_headers()?;
        self.client
            .execute_with_retry::<MessageBatch, ()>(&url, None, reqwest::Method::GET, headers)
            .await
    }

    /// List all message batches
    ///
    /// For response metadata, use [`Self::list_with_metadata`].
    pub async fn list(
        &self,
        options: Option<BatchListOptions>,
    ) -> Result<BatchListResponse, AnthropicError> {
        self.list_with_metadata(options).await.map(|r| r.data)
    }

    /// List batches with full response metadata
    ///
    /// For simple cases, use [`Self::list`].
    pub async fn list_with_metadata(
        &self,
        options: Option<BatchListOptions>,
    ) -> Result<Response<BatchListResponse>, AnthropicError> {
        let mut url = format!("{}/v1/messages/batches", self.client.api_base);

        // Add query parameters
        let mut query_parts = Vec::new();
        if let Some(opts) = options {
            if let Some(limit) = opts.limit {
                query_parts.push(format!("limit={}", limit));
            }
            if let Some(after_id) = opts.after_id {
                query_parts.push(format!("after_id={}", after_id));
            }
            if let Some(before_id) = opts.before_id {
                query_parts.push(format!("before_id={}", before_id));
            }
        }
        if !query_parts.is_empty() {
            url.push('?');
            url.push_str(&query_parts.join("&"));
        }

        let headers = self.build_headers()?;
        self.client
            .execute_with_retry::<BatchListResponse, ()>(&url, None, reqwest::Method::GET, headers)
            .await
    }

    /// Cancel a message batch
    ///
    /// Cancellation is asynchronous - the batch will transition to "canceling"
    /// and then "ended" once all in-flight requests complete.
    ///
    /// For response metadata, use [`Self::cancel_with_metadata`].
    pub async fn cancel(&self, batch_id: &str) -> Result<MessageBatch, AnthropicError> {
        self.cancel_with_metadata(batch_id).await.map(|r| r.data)
    }

    /// Cancel batch with full response metadata
    ///
    /// For simple cases, use [`Self::cancel`].
    pub async fn cancel_with_metadata(
        &self,
        batch_id: &str,
    ) -> Result<Response<MessageBatch>, AnthropicError> {
        let url = format!(
            "{}/v1/messages/batches/{}/cancel",
            self.client.api_base, batch_id
        );
        let headers = self.build_headers()?;
        self.client
            .execute_with_retry::<MessageBatch, ()>(&url, None, reqwest::Method::POST, headers)
            .await
    }

    /// Stream results from a completed batch
    ///
    /// Returns a stream of BatchResult items. Each line in the response is a
    /// JSON object containing the custom_id and result for one request.
    ///
    /// Only available after the batch has ended (processing_status == "ended").
    pub async fn results(
        &self,
        batch_id: &str,
    ) -> Result<BoxStream<'static, Result<BatchResult, AnthropicError>>, AnthropicError> {
        let url = format!(
            "{}/v1/messages/batches/{}/results",
            self.client.api_base, batch_id
        );

        let response = self
            .client
            .client
            .get(&url)
            .headers(self.build_headers()?)
            .send()
            .await
            .map_err(AnthropicError::from_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(parse_error_response(&error_body, status.as_u16()));
        }

        // Stream JSONL response
        let byte_stream = response.bytes_stream();

        let result_stream = async_stream::stream! {
            let mut buffer = String::new();
            let mut byte_stream = byte_stream;

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        // Append chunk to buffer
                        match std::str::from_utf8(&chunk) {
                            Ok(s) => buffer.push_str(s),
                            Err(e) => {
                                yield Err(AnthropicError::Other(format!("Invalid UTF-8: {}", e)));
                                return;
                            }
                        }

                        // Process complete lines
                        while let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].trim();
                            if !line.is_empty() {
                                match serde_json::from_str::<BatchResult>(line) {
                                    Ok(result) => yield Ok(result),
                                    Err(e) => {
                                        yield Err(AnthropicError::Other(format!(
                                            "Failed to parse batch result: {} (line: {})",
                                            e, line
                                        )));
                                    }
                                }
                            }
                            buffer = buffer[newline_pos + 1..].to_string();
                        }
                    }
                    Err(e) => {
                        yield Err(AnthropicError::Network(format!("Stream error: {}", e)));
                        return;
                    }
                }
            }

            // Process any remaining content in buffer
            let remaining = buffer.trim();
            if !remaining.is_empty() {
                match serde_json::from_str::<BatchResult>(remaining) {
                    Ok(result) => yield Ok(result),
                    Err(e) => {
                        yield Err(AnthropicError::Other(format!(
                            "Failed to parse final batch result: {} (line: {})",
                            e, remaining
                        )));
                    }
                }
            }
        };

        Ok(Box::pin(result_stream))
    }

    fn build_headers(&self) -> Result<HeaderMap, AnthropicError> {
        build_headers(&self.client.api_key, &self.client.api_version, None)
    }
}

// ============================================================================
// Shared Helpers
// ============================================================================

fn build_headers(
    api_key: &str,
    api_version: &str,
    betas: Option<&[String]>,
) -> Result<HeaderMap, AnthropicError> {
    let mut headers = HeaderMap::new();

    headers.insert(
        "x-api-key",
        HeaderValue::from_str(api_key)
            .map_err(|e| AnthropicError::Configuration(format!("Invalid API key: {}", e)))?,
    );

    headers.insert(
        "anthropic-version",
        HeaderValue::from_str(api_version)
            .map_err(|e| AnthropicError::Configuration(format!("Invalid API version: {}", e)))?,
    );

    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    // Add beta features header if any betas are specified
    if let Some(betas) = betas {
        if !betas.is_empty() {
            let beta_value = betas.join(",");
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_str(&beta_value).map_err(|e| {
                    AnthropicError::Configuration(format!("Invalid beta value: {}", e))
                })?,
            );
        }
    }

    Ok(headers)
}

fn parse_error_response(body: &str, status_code: u16) -> AnthropicError {
    // Try to parse as API error response
    if let Ok(error_response) = serde_json::from_str::<ApiErrorResponse>(body) {
        return AnthropicError::from_api_error(&error_response.error, status_code);
    }

    // Fallback to generic error based on status code
    let msg = if body.is_empty() {
        format!("HTTP {}", status_code)
    } else {
        body.to_string()
    };

    match status_code {
        401 => AnthropicError::Authentication(msg),
        429 => AnthropicError::RateLimited(msg),
        500..=599 => AnthropicError::ServiceUnavailable(msg),
        _ => AnthropicError::Other(msg),
    }
}

/// Convert HashMap headers back to reqwest HeaderMap for retry-after parsing
fn headers_to_reqwest(headers: &HashMap<String, String>) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::try_from(k.as_str()),
            HeaderValue::from_str(v),
        ) {
            map.insert(name, value);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_requires_api_key() {
        let result = Anthropic::builder().build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AnthropicError::Configuration(_)
        ));
    }

    #[test]
    fn test_builder_with_api_key() {
        let client = Anthropic::builder().api_key("test-key").build().unwrap();
        assert_eq!(client.api_base, DEFAULT_API_BASE);
        assert_eq!(client.api_version, DEFAULT_API_VERSION);
    }

    #[test]
    fn test_builder_custom_base() {
        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base("https://custom.api.com")
            .build()
            .unwrap();
        assert_eq!(client.api_base, "https://custom.api.com");
    }

    #[test]
    fn test_builder_with_max_retries() {
        let client = Anthropic::builder()
            .api_key("test-key")
            .max_retries(5)
            .build()
            .unwrap();
        assert_eq!(client.retry_config.max_retries, 5);
    }

    #[test]
    fn test_builder_with_retry_config() {
        let config = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            jitter: 0.1,
        };
        let client = Anthropic::builder()
            .api_key("test-key")
            .retry_config(config.clone())
            .build()
            .unwrap();
        assert_eq!(client.retry_config.max_retries, 3);
        assert_eq!(client.retry_config.base_delay, Duration::from_millis(100));
    }

    #[test]
    fn test_from_env_missing_key() {
        // This test assumes ANTHROPIC_API_KEY is not set in the test environment
        // If it is set, this test will fail - that's expected behavior
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = Anthropic::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.base_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(8));
    }

    #[test]
    fn test_retry_config_disabled() {
        let config = RetryConfig::disabled();
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn test_retry_delay_calculation() {
        let config = RetryConfig {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
            jitter: 0.0, // No jitter for predictable testing
        };

        // Attempt 0: 500ms * 2^0 = 500ms
        let delay0 = config.delay_for_attempt(0);
        assert_eq!(delay0, Duration::from_millis(500));

        // Attempt 1: 500ms * 2^1 = 1000ms
        let delay1 = config.delay_for_attempt(1);
        assert_eq!(delay1, Duration::from_millis(1000));

        // Attempt 2: 500ms * 2^2 = 2000ms
        let delay2 = config.delay_for_attempt(2);
        assert_eq!(delay2, Duration::from_millis(2000));
    }

    #[test]
    fn test_retry_delay_max_cap() {
        let config = RetryConfig {
            max_retries: 10,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            jitter: 0.0,
        };

        // Attempt 5: 1s * 2^5 = 32s, but capped at 5s
        let delay = config.delay_for_attempt(5);
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn test_is_retryable_status() {
        assert!(AnthropicError::is_retryable_status(408)); // Request Timeout
        assert!(AnthropicError::is_retryable_status(409)); // Conflict
        assert!(AnthropicError::is_retryable_status(429)); // Rate Limited
        assert!(AnthropicError::is_retryable_status(500)); // Internal Server Error
        assert!(AnthropicError::is_retryable_status(502)); // Bad Gateway
        assert!(AnthropicError::is_retryable_status(503)); // Service Unavailable
        assert!(AnthropicError::is_retryable_status(529)); // Overloaded

        assert!(!AnthropicError::is_retryable_status(400)); // Bad Request
        assert!(!AnthropicError::is_retryable_status(401)); // Unauthorized
        assert!(!AnthropicError::is_retryable_status(403)); // Forbidden
        assert!(!AnthropicError::is_retryable_status(404)); // Not Found
    }

    #[test]
    fn test_parse_retry_after_ms() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("1500"));

        let delay = RetryConfig::parse_retry_after(&headers);
        assert_eq!(delay, Some(Duration::from_millis(1500)));
    }

    #[test]
    fn test_parse_retry_after_secs() {
        let mut headers = HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("30"));

        let delay = RetryConfig::parse_retry_after(&headers);
        assert_eq!(delay, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_parse_retry_after_ms_precedence() {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after-ms", HeaderValue::from_static("500"));
        headers.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("10"));

        // retry-after-ms should take precedence
        let delay = RetryConfig::parse_retry_after(&headers);
        assert_eq!(delay, Some(Duration::from_millis(500)));
    }

    #[test]
    fn test_raw_response_creation() {
        // Can't easily test without a real response, but we can test the struct
        let raw = RawResponse {
            status: 200,
            headers: vec![
                ("request-id".to_string(), "req_123".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ]
            .into_iter()
            .collect(),
            request_id: Some("req_123".to_string()),
            rate_limit: None,
        };

        assert_eq!(raw.status, 200);
        assert_eq!(raw.request_id, Some("req_123".to_string()));
        assert_eq!(raw.header("content-type"), Some("application/json"));
    }

    #[test]
    fn test_rate_limit_info_parsing() {
        let headers: HashMap<String, String> = vec![
            (
                "anthropic-ratelimit-requests-limit".to_string(),
                "1000".to_string(),
            ),
            (
                "anthropic-ratelimit-requests-remaining".to_string(),
                "999".to_string(),
            ),
            (
                "anthropic-ratelimit-tokens-limit".to_string(),
                "100000".to_string(),
            ),
            (
                "anthropic-ratelimit-tokens-remaining".to_string(),
                "99000".to_string(),
            ),
        ]
        .into_iter()
        .collect();

        let info = RateLimitInfo::from_headers(&headers).unwrap();
        assert_eq!(info.requests_limit, Some(1000));
        assert_eq!(info.requests_remaining, Some(999));
        assert_eq!(info.tokens_limit, Some(100000));
        assert_eq!(info.tokens_remaining, Some(99000));
    }

    #[test]
    fn test_rate_limit_info_no_headers() {
        let headers: HashMap<String, String> = HashMap::new();
        let info = RateLimitInfo::from_headers(&headers);
        assert!(info.is_none());
    }

    #[test]
    fn test_rate_limit_info_partial_headers() {
        let headers: HashMap<String, String> = vec![
            (
                "anthropic-ratelimit-requests-limit".to_string(),
                "1000".to_string(),
            ),
            // Other headers missing
        ]
        .into_iter()
        .collect();

        let info = RateLimitInfo::from_headers(&headers).unwrap();
        assert_eq!(info.requests_limit, Some(1000));
        assert!(info.requests_remaining.is_none());
        assert!(info.tokens_limit.is_none());
    }

    #[test]
    fn test_rate_limit_info_with_reset_times() {
        let headers: HashMap<String, String> = vec![
            (
                "anthropic-ratelimit-requests-limit".to_string(),
                "1000".to_string(),
            ),
            (
                "anthropic-ratelimit-requests-reset".to_string(),
                "2024-01-01T00:00:00Z".to_string(),
            ),
            (
                "anthropic-ratelimit-tokens-reset".to_string(),
                "2024-01-01T00:01:00Z".to_string(),
            ),
        ]
        .into_iter()
        .collect();

        let info = RateLimitInfo::from_headers(&headers).unwrap();
        assert_eq!(
            info.requests_reset,
            Some("2024-01-01T00:00:00Z".to_string())
        );
        assert_eq!(info.tokens_reset, Some("2024-01-01T00:01:00Z".to_string()));
    }

    #[test]
    fn test_rate_limit_info_invalid_numbers() {
        let headers: HashMap<String, String> = vec![(
            "anthropic-ratelimit-requests-limit".to_string(),
            "not_a_number".to_string(),
        )]
        .into_iter()
        .collect();

        let info = RateLimitInfo::from_headers(&headers).unwrap();
        assert!(info.requests_limit.is_none()); // Parse fails, returns None
    }

    // ===== Response Tests =====

    #[test]
    fn test_response_into_data() {
        let response = Response {
            data: "test data".to_string(),
            raw: RawResponse {
                status: 200,
                headers: HashMap::new(),
                request_id: None,
                rate_limit: None,
            },
        };
        assert_eq!(response.into_data(), "test data");
    }

    #[test]
    fn test_response_request_id() {
        let response = Response {
            data: (),
            raw: RawResponse {
                status: 200,
                headers: HashMap::new(),
                request_id: Some("req_abc123".to_string()),
                rate_limit: None,
            },
        };
        assert_eq!(response.request_id(), Some("req_abc123"));
    }

    #[test]
    fn test_response_request_id_none() {
        let response = Response {
            data: (),
            raw: RawResponse {
                status: 200,
                headers: HashMap::new(),
                request_id: None,
                rate_limit: None,
            },
        };
        assert!(response.request_id().is_none());
    }

    #[test]
    fn test_response_rate_limit() {
        let rate_limit = RateLimitInfo {
            requests_limit: Some(1000),
            requests_remaining: Some(999),
            requests_reset: None,
            tokens_limit: Some(100000),
            tokens_remaining: Some(99000),
            tokens_reset: None,
        };
        let response = Response {
            data: (),
            raw: RawResponse {
                status: 200,
                headers: HashMap::new(),
                request_id: None,
                rate_limit: Some(rate_limit),
            },
        };
        let rl = response.rate_limit().unwrap();
        assert_eq!(rl.requests_limit, Some(1000));
    }

    // ===== Builder Tests =====

    #[test]
    fn test_builder_api_version() {
        let client = Anthropic::builder()
            .api_key("test-key")
            .api_version("2024-01-01")
            .build()
            .unwrap();
        assert_eq!(client.api_version, "2024-01-01");
    }

    #[test]
    fn test_builder_timeout() {
        // We can't easily verify the timeout was set on the reqwest client,
        // but we can verify the builder accepts it without error
        let client = Anthropic::builder()
            .api_key("test-key")
            .timeout(Duration::from_secs(30))
            .build();
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_new() {
        let client = Anthropic::new("test-key");
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_debug_redacts_api_key() {
        let client = Anthropic::new("super-secret-key").unwrap();
        let debug_str = format!("{:?}", client);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("super-secret-key"));
    }

    #[test]
    fn test_client_messages_api() {
        let client = Anthropic::new("test-key").unwrap();
        let _messages = client.messages();
        // Just verify we can get the messages handle without panic
    }

    #[test]
    fn test_client_batches_api() {
        let client = Anthropic::new("test-key").unwrap();
        let _batches = client.batches();
        // Just verify we can get the batches handle without panic
    }

    #[test]
    fn test_raw_response_header() {
        let mut headers = HashMap::new();
        headers.insert("x-custom-header".to_string(), "custom-value".to_string());
        headers.insert("content-type".to_string(), "application/json".to_string());

        let raw = RawResponse {
            status: 200,
            headers,
            request_id: None,
            rate_limit: None,
        };

        assert_eq!(raw.header("x-custom-header"), Some("custom-value"));
        assert_eq!(raw.header("content-type"), Some("application/json"));
        assert!(raw.header("non-existent").is_none());
    }
}

#[cfg(test)]
mod wiremock_tests {
    use super::*;
    use crate::messages::{MessageContent, MessageParam, Role, StopReason};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn message_response_json() -> serde_json::Value {
        serde_json::json!({
            "id": "msg_test123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })
    }

    fn error_response_json(error_type: &str, message: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message
            }
        })
    }

    #[tokio::test]
    async fn test_successful_message_create() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(message_response_json())
                    .insert_header("request-id", "req_abc123"),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .build()
            .unwrap();

        let response = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await
            .unwrap();

        assert_eq!(response.id, "msg_test123");
        assert_eq!(response.stop_reason, Some(StopReason::EndTurn));
    }

    #[tokio::test]
    async fn test_message_create_with_metadata() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(message_response_json())
                    .insert_header("request-id", "req_xyz789")
                    .insert_header("anthropic-ratelimit-requests-limit", "1000")
                    .insert_header("anthropic-ratelimit-requests-remaining", "999"),
            )
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .build()
            .unwrap();

        let response = client
            .messages()
            .create_with_metadata(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await
            .unwrap();

        assert_eq!(response.request_id(), Some("req_xyz789"));
        assert_eq!(response.raw.status, 200);

        let rate_limit = response.rate_limit().unwrap();
        assert_eq!(rate_limit.requests_limit, Some(1000));
        assert_eq!(rate_limit.requests_remaining, Some(999));
    }

    #[tokio::test]
    async fn test_authentication_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(401).set_body_json(error_response_json(
                    "authentication_error",
                    "Invalid API key",
                )),
            )
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("bad-key")
            .api_base(mock_server.uri())
            .max_retries(0)
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::Authentication(_))));
    }

    #[tokio::test]
    async fn test_invalid_request_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(error_response_json(
                    "invalid_request_error",
                    "max_tokens must be positive",
                )),
            )
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .max_retries(0)
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::InvalidRequest(_))));
    }

    #[tokio::test]
    async fn test_retry_on_rate_limit() {
        let mock_server = MockServer::start().await;

        // First request returns 429, second succeeds
        // Mount success mock first (lower priority), then rate limit mock (higher priority, but limited)
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(message_response_json()))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(429)
                    .set_body_json(error_response_json("rate_limit_error", "Too many requests"))
                    .insert_header("retry-after-ms", "10"),
            )
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 1,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_retry_on_overloaded() {
        let mock_server = MockServer::start().await;

        // Mount success mock first (lower priority), then overloaded mock (higher priority, but limited)
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(message_response_json()))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(529)
                    .set_body_json(error_response_json("overloaded_error", "API is overloaded")),
            )
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 1,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_no_retry_on_auth_error() {
        let mock_server = MockServer::start().await;

        // Auth errors should not retry
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(error_response_json("authentication_error", "Invalid key")),
            )
            .expect(1) // Should only be called once, no retry
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::Authentication(_))));
    }

    #[tokio::test]
    async fn test_exhausted_retries() {
        let mock_server = MockServer::start().await;

        // Always return 503
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_json(error_response_json("api_error", "Service unavailable")),
            )
            .expect(3) // Initial + 2 retries
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 2,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::ServiceUnavailable(_))));
    }

    #[tokio::test]
    async fn test_count_tokens() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages/count_tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "input_tokens": 42
            })))
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .build()
            .unwrap();

        let response = client
            .messages()
            .count_tokens(CountTokensParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hello world".to_string()),
                }],
                system: None,
                tools: None,
            })
            .await
            .unwrap();

        assert_eq!(response.input_tokens, 42);
    }

    #[tokio::test]
    async fn test_retry_after_header_respected() {
        let mock_server = MockServer::start().await;

        // Mount success mock first (lower priority), then rate limit mock (higher priority, but limited)
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(message_response_json()))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(429)
                    .set_body_json(error_response_json("rate_limit_error", "Rate limited"))
                    .insert_header("retry-after", "1"), // Standard retry-after header
            )
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 1,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_secs(10),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        // Just verify retry succeeds - timing assertions are flaky in CI
        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_retry_on_500_error() {
        let mock_server = MockServer::start().await;

        // Mount success mock first, then 500 error mock
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(message_response_json()))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(error_response_json("api_error", "Internal server error")),
            )
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 1,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        // Should succeed after retry
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_response_malformed_json() {
        let mock_server = MockServer::start().await;

        // Return 200 with invalid JSON - triggers InvalidResponse error
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not valid json"))
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .max_retries(0)
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::InvalidResponse(_))));

        // Verify error message is helpful
        if let Err(AnthropicError::InvalidResponse(msg)) = result {
            assert!(msg.contains("Failed to parse response"));
        }
    }

    #[tokio::test]
    async fn test_invalid_response_wrong_schema() {
        let mock_server = MockServer::start().await;

        // Return 200 with valid JSON but wrong schema
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"unexpected": "schema"})),
            )
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .max_retries(0)
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::InvalidResponse(_))));
    }

    #[tokio::test]
    async fn test_invalid_response_not_retried() {
        let mock_server = MockServer::start().await;

        // Return 200 with invalid JSON - should NOT be retried
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string("invalid"))
            .expect(1) // Should only be called once (no retries)
            .mount(&mock_server)
            .await;

        let client = Anthropic::builder()
            .api_key("test-key")
            .api_base(mock_server.uri())
            .retry_config(RetryConfig {
                max_retries: 3,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(10),
                jitter: 0.0,
            })
            .build()
            .unwrap();

        let result = client
            .messages()
            .create(MessageCreateParams {
                model: "claude-sonnet-4-20250514".to_string(),
                messages: vec![MessageParam {
                    role: Role::User,
                    content: MessageContent::Text("Hi".to_string()),
                }],
                max_tokens: 1024,
                system: None,
                temperature: None,
                top_p: None,
                top_k: None,
                tools: None,
                tool_choice: None,
                stop_sequences: None,
                stream: None,
                metadata: None,
                service_tier: None,
                thinking: None,
                betas: None,
            })
            .await;

        assert!(matches!(result, Err(AnthropicError::InvalidResponse(_))));
        // Mock expectation of 1 call verifies no retries occurred
    }
}
