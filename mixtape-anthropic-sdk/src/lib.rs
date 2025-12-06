//! Minimal Anthropic API client for mixtape
//!
//! This crate provides a lightweight, focused client for the Anthropic Messages API.
//! It supports both regular and streaming message creation with tool use.
//!
//! # Quick Start
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//!
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!     .user("Hello, Claude!")
//!     .build();
//!
//! let response = client.messages().create(params).await?;
//! println!("{:?}", response);
//! # Ok(())
//! # }
//! ```
//!
//! # Streaming Responses
//!
//! For long responses, streaming provides a better user experience:
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!     .user("Tell me a story")
//!     .build();
//!
//! // Collect all text in one call
//! let stream = client.messages().stream(params.clone()).await?;
//! let text = stream.collect_text().await?;
//!
//! // Or reconstruct the full message
//! let stream = client.messages().stream(params).await?;
//! let message = stream.collect_message().await?;
//! println!("Stop reason: {:?}", message.stop_reason);
//! # Ok(())
//! # }
//! ```
//!
//! # Tool Use
//!
//! Define tools for the model to use:
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{
//!     Anthropic, MessageCreateParams, Tool, ToolInputSchema, ToolChoice, ContentBlock,
//! };
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//!
//! let tool = Tool {
//!     name: "get_weather".to_string(),
//!     description: Some("Get the current weather for a location".to_string()),
//!     input_schema: ToolInputSchema::new(),
//!     cache_control: None,
//!     tool_type: None,
//! };
//!
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!     .user("What's the weather in San Francisco?")
//!     .tools(vec![tool])
//!     .tool_choice(ToolChoice::auto())
//!     .build();
//!
//! let response = client.messages().create(params).await?;
//!
//! for block in &response.content {
//!     if let ContentBlock::ToolUse { id, name, input } = block {
//!         println!("Tool {} called with input: {}", name, input);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Rate Limits and Raw Response
//!
//! Access rate limit information and request IDs for debugging:
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!     .user("Hello!")
//!     .build();
//!
//! let response = client.messages().create_with_metadata(params).await?;
//!
//! println!("Response: {:?}", response.data);
//!
//! if let Some(request_id) = response.request_id() {
//!     println!("Request ID: {}", request_id);
//! }
//! if let Some(rate_limit) = response.rate_limit() {
//!     println!("Requests remaining: {:?}", rate_limit.requests_remaining);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Retry Configuration
//!
//! Configure automatic retry behavior:
//!
//! ```
//! use mixtape_anthropic_sdk::{Anthropic, RetryConfig};
//! use std::time::Duration;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::builder()
//!     .api_key("your-api-key")
//!     .max_retries(5)
//!     .build()?;
//!
//! // Or with full control
//! let client = Anthropic::builder()
//!     .api_key("your-api-key")
//!     .retry_config(RetryConfig {
//!         max_retries: 3,
//!         base_delay: Duration::from_millis(500),
//!         max_delay: Duration::from_secs(10),
//!         jitter: 0.25,
//!     })
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Extended Thinking
//!
//! Enable extended thinking for complex reasoning tasks:
//!
//! ```
//! use mixtape_anthropic_sdk::MessageCreateParams;
//!
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 16000)
//!     .user("Solve this complex math problem...")
//!     .thinking(4096)  // Allow 4096 tokens for thinking
//!     .build();
//! ```

// Domain modules
pub mod batch;
mod client;
mod error;
pub mod messages;
pub mod streaming;
pub mod tokens;
pub mod tools;

// Client types
pub use client::{
    Anthropic, AnthropicBuilder, BatchListOptions, Batches, Messages, RateLimitInfo, RawResponse,
    Response,
};

// Error types
pub use error::{AnthropicError, ApiError, ApiErrorResponse, RetryConfig};

// Streaming
pub use streaming::{
    ContentBlockDelta, DeltaUsage, MessageDeltaData, MessageStream, MessageStreamEvent,
};

// Messages - request types
pub use messages::{
    CacheControl, CacheTtl, CitationsConfig, ContentBlockParam, DocumentSource, ImageSource,
    MessageContent, MessageCreateParams, MessageCreateParamsBuilder, MessageParam, Metadata, Role,
    ServiceTier, ThinkingConfig, ToolResultContent, ToolResultContentBlock, WebSearchErrorCode,
    WebSearchResult, WebSearchToolResultContent, WebSearchToolResultError,
};

// Messages - response types
pub use messages::{ContentBlock, Message, StopReason, Usage};

// Tools
pub use tools::{Tool, ToolChoice, ToolInputSchema};

// Batch API
pub use batch::{
    BatchCreateParams, BatchError, BatchListResponse, BatchRequest, BatchRequestCounts,
    BatchResult, BatchResultType, BatchStatus, MessageBatch,
};

// Token counting
pub use tokens::{CountTokensParams, CountTokensParamsBuilder, CountTokensResponse};
