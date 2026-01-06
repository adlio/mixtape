//! Message types for the Anthropic Messages API
//!
//! This module contains all types related to creating and receiving messages,
//! including request parameters, content blocks, and response structures.
//!
//! # Request vs Response Types
//!
//! Types follow a naming convention:
//! - Request types use a `Param` suffix (e.g., `MessageParam`, `ContentBlockParam`)
//! - Response types have no suffix (e.g., `Message`, `ContentBlock`)
//!
//! # Example
//!
//! ```
//! use mixtape_anthropic_sdk::{MessageCreateParams, MessageParam};
//!
//! let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!     .user("Hello, Claude!")
//!     .build();
//! ```

use crate::tools::{Tool, ToolChoice};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Beta Features
// ============================================================================

/// Beta features that can be enabled via the `anthropic-beta` header
///
/// Beta features are experimental capabilities that require explicit opt-in.
/// Use these with the `.betas()` builder method or convenience methods like
/// `.with_1m_context()`.
///
/// # Example
///
/// ```
/// use mixtape_anthropic_sdk::{MessageCreateParams, BetaFeature};
///
/// let params = MessageCreateParams::builder("claude-sonnet-4-5-20250929", 8192)
///     .betas(vec![BetaFeature::Context1M])
///     .user("Analyze this large document...")
///     .build();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BetaFeature {
    /// Extended 1M token context window for Claude Sonnet 4/4.5
    ///
    /// Expands the context window from 200K to 1 million tokens.
    /// Only supported on Claude Sonnet 4 and Claude Sonnet 4.5 models.
    ///
    /// **Pricing**: ~2x input, ~1.5x output when prompts exceed 200K tokens.
    Context1M,

    /// A custom beta feature identifier for forward compatibility
    ///
    /// Use this for beta features not yet added to this enum.
    Custom(String),
}

impl BetaFeature {
    /// Get the API identifier string for this beta feature
    pub fn as_str(&self) -> &str {
        match self {
            BetaFeature::Context1M => "context-1m-2025-08-07",
            BetaFeature::Custom(s) => s,
        }
    }
}

impl std::fmt::Display for BetaFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Request Types
// ============================================================================

/// Parameters for creating a message
#[derive(Debug, Clone, Serialize)]
pub struct MessageCreateParams {
    /// The model to use (e.g., "claude-sonnet-4-20250514")
    pub model: String,

    /// The messages in the conversation
    pub messages: Vec<MessageParam>,

    /// Maximum tokens to generate
    pub max_tokens: u32,

    /// System prompt (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Sampling temperature (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Top-p sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Top-k sampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    /// Tools available to the model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// How the model should use tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,

    /// Stop sequences
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to stream the response (set internally by SDK)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream: Option<bool>,

    /// Request metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Service tier selection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,

    /// Extended thinking configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,

    /// Beta features to enable via the `anthropic-beta` header
    ///
    /// Prefer using convenience methods like `.with_1m_context()` instead of
    /// setting this directly.
    #[serde(skip)]
    pub betas: Option<Vec<BetaFeature>>,
}

impl MessageCreateParams {
    /// Create a builder for MessageCreateParams
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageCreateParams, MessageParam};
    ///
    /// // Using convenience methods
    /// let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    ///     .user("Hello!")
    ///     .temperature(0.7)
    ///     .build();
    ///
    /// // Or with explicit messages
    /// let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    ///     .messages(vec![MessageParam::user("Hello!")])
    ///     .build();
    /// ```
    pub fn builder(model: impl Into<String>, max_tokens: u32) -> MessageCreateParamsBuilder {
        MessageCreateParamsBuilder::new(model, max_tokens)
    }
}

/// Builder for MessageCreateParams
///
/// Provides a fluent API for constructing message creation parameters.
/// The builder starts with an empty message list; use `.message()`, `.messages()`,
/// `.user()`, or `.assistant()` to add messages.
///
/// # Example
///
/// ```
/// use mixtape_anthropic_sdk::{MessageCreateParams, MessageParam};
///
/// // Using convenience methods for simple cases
/// let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
///     .user("Hello!")
///     .system("You are a helpful assistant.")
///     .temperature(0.7)
///     .build();
///
/// // Using .messages() for multiple messages
/// let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
///     .messages(vec![
///         MessageParam::user("Hello!"),
///         MessageParam::assistant("Hi there!"),
///         MessageParam::user("How are you?"),
///     ])
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct MessageCreateParamsBuilder {
    model: String,
    max_tokens: u32,
    messages: Vec<MessageParam>,
    system: Option<String>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    top_k: Option<u32>,
    tools: Option<Vec<Tool>>,
    tool_choice: Option<ToolChoice>,
    stop_sequences: Option<Vec<String>>,
    stream: Option<bool>,
    metadata: Option<Metadata>,
    service_tier: Option<ServiceTier>,
    thinking: Option<ThinkingConfig>,
    betas: Option<Vec<BetaFeature>>,
}

impl MessageCreateParamsBuilder {
    /// Create a new builder with required parameters
    pub fn new(model: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            messages: Vec::new(),
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
        }
    }

    /// Append messages to the conversation
    ///
    /// Uses extend semantics: messages are added to any existing messages.
    /// Since the builder starts with an empty message list, the first call
    /// effectively sets the initial messages.
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageCreateParams, MessageParam};
    ///
    /// // Append multiple messages at once
    /// let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    ///     .messages(vec![
    ///         MessageParam::user("Hello!"),
    ///         MessageParam::assistant("Hi there!"),
    ///     ])
    ///     .messages(vec![MessageParam::user("How are you?")])
    ///     .build();
    ///
    /// assert_eq!(params.messages.len(), 3);
    /// ```
    pub fn messages(mut self, messages: impl IntoIterator<Item = MessageParam>) -> Self {
        self.messages.extend(messages);
        self
    }

    /// Append a single message to the conversation
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageCreateParams, MessageParam};
    ///
    /// let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    ///     .message(MessageParam::user("Hello!"))
    ///     .message(MessageParam::assistant("Hi!"))
    ///     .build();
    ///
    /// assert_eq!(params.messages.len(), 2);
    /// ```
    pub fn message(mut self, message: MessageParam) -> Self {
        self.messages.push(message);
        self
    }

    /// Add a user message with text content
    pub fn user(mut self, content: impl Into<MessageContent>) -> Self {
        self.messages.push(MessageParam::user(content));
        self
    }

    /// Add an assistant message with text content
    pub fn assistant(mut self, content: impl Into<MessageContent>) -> Self {
        self.messages.push(MessageParam::assistant(content));
        self
    }

    /// Set the system prompt
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the sampling temperature (0.0 to 1.0)
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set top-p sampling
    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Set top-k sampling
    pub fn top_k(mut self, top_k: u32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Set the available tools
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set how the model should use tools
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Set stop sequences
    pub fn stop_sequences(mut self, stop_sequences: Vec<String>) -> Self {
        self.stop_sequences = Some(stop_sequences);
        self
    }

    /// Set request metadata
    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set service tier selection
    pub fn service_tier(mut self, service_tier: ServiceTier) -> Self {
        self.service_tier = Some(service_tier);
        self
    }

    /// Enable extended thinking with a budget
    pub fn thinking(mut self, budget_tokens: u32) -> Self {
        self.thinking = Some(ThinkingConfig::enabled(budget_tokens));
        self
    }

    /// Set extended thinking configuration
    pub fn thinking_config(mut self, config: ThinkingConfig) -> Self {
        self.thinking = Some(config);
        self
    }

    /// Enable beta features
    ///
    /// For common beta features, prefer convenience methods like `.with_1m_context()`.
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageCreateParams, BetaFeature};
    ///
    /// let params = MessageCreateParams::builder("claude-sonnet-4-5-20250929", 8192)
    ///     .betas(vec![BetaFeature::Context1M])
    ///     .user("Analyze this large document...")
    ///     .build();
    /// ```
    pub fn betas(mut self, betas: Vec<BetaFeature>) -> Self {
        self.betas = Some(betas);
        self
    }

    /// Enable 1M token context window (beta)
    ///
    /// Expands the context window from 200K to 1 million tokens.
    /// Only supported on Claude Sonnet 4 and Claude Sonnet 4.5 models.
    ///
    /// **Pricing**: When prompts exceed 200K tokens, input costs ~2x and
    /// output costs ~1.5x standard rates.
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::MessageCreateParams;
    ///
    /// let params = MessageCreateParams::builder("claude-sonnet-4-5-20250929", 8192)
    ///     .with_1m_context()
    ///     .user("Analyze this large document...")
    ///     .build();
    /// ```
    pub fn with_1m_context(mut self) -> Self {
        let betas = self.betas.get_or_insert_with(Vec::new);
        if !betas.contains(&BetaFeature::Context1M) {
            betas.push(BetaFeature::Context1M);
        }
        self
    }

    /// Build the MessageCreateParams
    pub fn build(self) -> MessageCreateParams {
        MessageCreateParams {
            model: self.model,
            messages: self.messages,
            max_tokens: self.max_tokens,
            system: self.system,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            tools: self.tools,
            tool_choice: self.tool_choice,
            stop_sequences: self.stop_sequences,
            stream: self.stream,
            metadata: self.metadata,
            service_tier: self.service_tier,
            thinking: self.thinking,
            betas: self.betas,
        }
    }
}

// ============================================================================
// Message Content Types
// ============================================================================

/// Role in a conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A message in the conversation (request format)
#[derive(Debug, Clone, Serialize)]
pub struct MessageParam {
    /// The role of the message author
    pub role: Role,

    /// The content of the message
    pub content: MessageContent,
}

impl MessageParam {
    /// Create a user message with text content
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::MessageParam;
    ///
    /// let msg = MessageParam::user("Hello, Claude!");
    /// ```
    pub fn user(content: impl Into<MessageContent>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    /// Create an assistant message with text content
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::MessageParam;
    ///
    /// let msg = MessageParam::assistant("Hello! How can I help?");
    /// ```
    pub fn assistant(content: impl Into<MessageContent>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }

    /// Create a user message with content blocks (for multi-modal content)
    ///
    /// Use this for messages that include images, documents, or mixed content types.
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageParam, ContentBlockParam};
    ///
    /// let msg = MessageParam::user_blocks(vec![
    ///     ContentBlockParam::Text {
    ///         text: "What's in this image?".to_string(),
    ///         cache_control: None,
    ///     },
    ///     // ContentBlockParam::Image { ... }
    /// ]);
    /// ```
    pub fn user_blocks(blocks: Vec<ContentBlockParam>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Blocks(blocks),
        }
    }

    /// Create an assistant message with content blocks
    ///
    /// Use this for assistant messages that include tool use results or mixed content.
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageParam, ContentBlockParam};
    ///
    /// let msg = MessageParam::assistant_blocks(vec![
    ///     ContentBlockParam::Text {
    ///         text: "Here's my response".to_string(),
    ///         cache_control: None,
    ///     },
    /// ]);
    /// ```
    pub fn assistant_blocks(blocks: Vec<ContentBlockParam>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Blocks(blocks),
        }
    }
}

/// Content of a message - can be simple text or structured blocks
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content
    Text(String),

    /// Structured content blocks
    Blocks(Vec<ContentBlockParam>),
}

impl MessageContent {
    /// Create text content
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::MessageContent;
    ///
    /// let content = MessageContent::text("Hello!");
    /// ```
    pub fn text(s: impl Into<String>) -> Self {
        MessageContent::Text(s.into())
    }

    /// Create content from blocks
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{MessageContent, ContentBlockParam};
    ///
    /// let blocks = vec![
    ///     ContentBlockParam::Text { text: "Hello".to_string(), cache_control: None },
    /// ];
    /// let content = MessageContent::blocks(blocks);
    /// ```
    pub fn blocks(blocks: Vec<ContentBlockParam>) -> Self {
        MessageContent::Blocks(blocks)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        MessageContent::Text(s.to_string())
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::Text(s)
    }
}

impl From<Vec<ContentBlockParam>> for MessageContent {
    fn from(blocks: Vec<ContentBlockParam>) -> Self {
        MessageContent::Blocks(blocks)
    }
}

// ============================================================================
// Content Blocks (Request)
// ============================================================================

/// A content block in a request
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockParam {
    /// Text content
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },

    /// Tool use request (from assistant)
    ToolUse {
        id: String,
        name: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },

    /// Tool result (from user, in response to tool use)
    ToolResult {
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<ToolResultContent>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },

    /// Image content
    Image {
        source: ImageSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },

    /// Document content (PDF, plain text)
    Document {
        source: DocumentSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        citations: Option<CitationsConfig>,
    },

    /// Thinking block (for multi-turn with extended thinking)
    Thinking { thinking: String, signature: String },

    /// Redacted thinking block
    RedactedThinking { data: String },

    /// Server tool use block (for server-side tools like web search)
    ServerToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Web search tool result
    WebSearchToolResult {
        tool_use_id: String,
        content: WebSearchToolResultContent,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

/// Tool result content - can be text or structured
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// Simple text result
    Text(String),
    /// Structured content blocks
    Blocks(Vec<ToolResultContentBlock>),
}

/// Content block within a tool result
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContentBlock {
    /// Text content
    Text { text: String },
    /// Image content
    Image { source: ImageSource },
    /// Document content
    Document {
        source: DocumentSource,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
}

/// Source of an image
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64-encoded image data
    Base64 { media_type: String, data: String },
    /// URL reference to an image
    Url { url: String },
}

/// Source of a document
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentSource {
    /// Base64-encoded document data
    Base64 { media_type: String, data: String },
    /// Plain text document
    Text { data: String, media_type: String },
    /// URL reference to a document
    Url { url: String },
    /// Content blocks as document
    Content { content: Vec<ContentBlockParam> },
}

/// Web search tool result content
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum WebSearchToolResultContent {
    /// List of search results
    Results(Vec<WebSearchResult>),
    /// Error from web search
    Error(WebSearchToolResultError),
}

/// A single web search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    /// Title of the result
    pub title: String,
    /// URL of the result
    pub url: String,
    /// Encrypted content
    pub encrypted_content: String,
    /// Age of the page (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_age: Option<String>,
}

/// Error from web search tool
#[derive(Debug, Clone, Serialize)]
pub struct WebSearchToolResultError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub error_code: WebSearchErrorCode,
}

/// Web search error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchErrorCode {
    InvalidToolInput,
    Unavailable,
    MaxUsesExceeded,
    TooManyRequests,
    QueryTooLong,
}

/// Citations configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Response from the Messages API
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    /// Unique identifier for the message
    pub id: String,

    /// Object type (always "message")
    #[serde(rename = "type")]
    pub message_type: String,

    /// Role of the message author (always "assistant" for responses)
    pub role: Role,

    /// Content blocks in the response
    pub content: Vec<ContentBlock>,

    /// Model that generated the response
    pub model: String,

    /// Reason the model stopped generating
    pub stop_reason: Option<StopReason>,

    /// Stop sequence that triggered completion (if any)
    pub stop_sequence: Option<String>,

    /// Token usage statistics
    pub usage: Usage,
}

/// A content block in a response
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content
    Text { text: String },

    /// Tool use request
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Thinking block (extended thinking output)
    Thinking { thinking: String, signature: String },

    /// Redacted thinking block
    RedactedThinking { data: String },

    /// Server tool use (server-side tools like web search)
    ServerToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Web search tool result
    WebSearchToolResult {
        tool_use_id: String,
        content: Value, // Can be results array or error
    },
}

/// Reason the model stopped generating
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of turn
    EndTurn,

    /// Model wants to use a tool
    ToolUse,

    /// Hit max_tokens limit
    MaxTokens,

    /// Hit a stop sequence
    StopSequence,

    /// Model paused (for multi-turn with thinking)
    PauseTurn,

    /// Model refused to respond
    Refusal,
}

/// Token usage statistics
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Usage {
    /// Tokens in the input (prompt)
    pub input_tokens: u32,

    /// Tokens in the output (completion)
    pub output_tokens: u32,

    /// Tokens written to cache
    #[serde(default)]
    pub cache_creation_input_tokens: u32,

    /// Tokens read from cache
    #[serde(default)]
    pub cache_read_input_tokens: u32,
}

// ============================================================================
// Shared Types
// ============================================================================

/// Cache control configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub cache_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<CacheTtl>,
}

impl CacheControl {
    /// Create an ephemeral cache control with default TTL
    pub fn ephemeral() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: None,
        }
    }

    /// Create an ephemeral cache control with 5 minute TTL
    pub fn ephemeral_5m() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: Some(CacheTtl::FiveMinutes),
        }
    }

    /// Create an ephemeral cache control with 1 hour TTL
    pub fn ephemeral_1h() -> Self {
        Self {
            cache_type: "ephemeral".to_string(),
            ttl: Some(CacheTtl::OneHour),
        }
    }
}

/// Cache TTL options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

/// Request metadata
#[derive(Debug, Clone, Serialize)]
pub struct Metadata {
    /// External user identifier (should be opaque, like a UUID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// Service tier selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    /// Automatically select tier
    Auto,
    /// Only use standard capacity
    StandardOnly,
}

/// Extended thinking configuration
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    /// Enable extended thinking
    Enabled {
        /// Token budget for thinking (must be >= 1024 and < max_tokens)
        budget_tokens: u32,
    },
    /// Disable extended thinking
    Disabled,
}

impl ThinkingConfig {
    /// Enable thinking with the specified budget
    pub fn enabled(budget_tokens: u32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Disable thinking
    pub fn disabled() -> Self {
        Self::Disabled
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_param_serialization() {
        let msg = MessageParam {
            role: Role::User,
            content: MessageContent::Text("Hello".to_string()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
    }

    #[test]
    fn test_content_block_param_serialization() {
        let block = ContentBlockParam::Text {
            text: "Hello".to_string(),
            cache_control: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_message_content_from_str() {
        let content: MessageContent = "Hello".into();
        match content {
            MessageContent::Text(s) => assert_eq!(s, "Hello"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_message_content_from_blocks() {
        let blocks = vec![ContentBlockParam::Text {
            text: "Hello".to_string(),
            cache_control: None,
        }];
        let content: MessageContent = blocks.into();
        match content {
            MessageContent::Blocks(b) => assert_eq!(b.len(), 1),
            _ => panic!("Expected Blocks variant"),
        }
    }

    #[test]
    fn test_builder_pattern() {
        let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
            .user("Hello")
            .system("Be helpful")
            .temperature(0.7)
            .build();

        assert_eq!(params.model, "claude-sonnet-4-20250514");
        assert_eq!(params.max_tokens, 1024);
        assert_eq!(params.messages.len(), 1);
        assert_eq!(params.system, Some("Be helpful".to_string()));
        assert_eq!(params.temperature, Some(0.7));
    }

    #[test]
    fn test_cache_control() {
        let cc = CacheControl::ephemeral();
        assert_eq!(cc.cache_type, "ephemeral");
        assert!(cc.ttl.is_none());

        assert_eq!(
            CacheControl::ephemeral_5m().ttl,
            Some(CacheTtl::FiveMinutes)
        );
        assert_eq!(CacheControl::ephemeral_1h().ttl, Some(CacheTtl::OneHour));
    }

    #[test]
    fn test_thinking_config() {
        let enabled = ThinkingConfig::enabled(4096);
        match enabled {
            ThinkingConfig::Enabled { budget_tokens } => assert_eq!(budget_tokens, 4096),
            _ => panic!("Expected Enabled"),
        }

        let disabled = ThinkingConfig::disabled();
        assert!(matches!(disabled, ThinkingConfig::Disabled));
    }

    #[test]
    fn test_messages_extend_semantics() {
        // Test that .messages() extends rather than replaces
        let cases = [
            (
                "single call with multiple messages",
                vec![vec![
                    MessageParam::user("msg1"),
                    MessageParam::assistant("msg2"),
                ]],
                2,
            ),
            (
                "multiple calls accumulate",
                vec![
                    vec![MessageParam::user("msg1")],
                    vec![MessageParam::assistant("msg2")],
                    vec![MessageParam::user("msg3")],
                ],
                3,
            ),
            ("empty vec does nothing", vec![vec![]], 0),
            (
                "mix of empty and non-empty",
                vec![vec![], vec![MessageParam::user("msg1")], vec![]],
                1,
            ),
        ];

        for (name, message_vecs, expected_count) in cases {
            let mut builder = MessageCreateParams::builder("test-model", 100);
            for messages in message_vecs {
                builder = builder.messages(messages);
            }
            let params = builder.build();
            assert_eq!(params.messages.len(), expected_count, "case: {}", name);
        }
    }

    #[test]
    fn test_message_method() {
        // Test single message append
        let params = MessageCreateParams::builder("test-model", 100)
            .message(MessageParam::user("msg1"))
            .message(MessageParam::assistant("msg2"))
            .message(MessageParam::user("msg3"))
            .build();

        assert_eq!(params.messages.len(), 3);
        assert!(matches!(params.messages[0].role, Role::User));
        assert!(matches!(params.messages[1].role, Role::Assistant));
        assert!(matches!(params.messages[2].role, Role::User));
    }

    #[test]
    fn test_message_and_messages_together() {
        // Test that .message() and .messages() can be mixed
        let params = MessageCreateParams::builder("test-model", 100)
            .message(MessageParam::user("msg1"))
            .messages(vec![
                MessageParam::assistant("msg2"),
                MessageParam::user("msg3"),
            ])
            .message(MessageParam::assistant("msg4"))
            .build();

        assert_eq!(params.messages.len(), 4);
    }

    #[test]
    fn test_builder_convenience_methods() {
        // Test all builder convenience methods in one test
        let params = MessageCreateParams::builder("test-model", 1024)
            .user("user message")
            .assistant("assistant message")
            .build();

        assert_eq!(params.messages.len(), 2);
        assert!(matches!(params.messages[0].role, Role::User));
        assert!(matches!(params.messages[1].role, Role::Assistant));
    }

    #[test]
    fn test_all_builder_methods() {
        // Table-based test for all builder setter methods
        let stop_seqs = vec!["STOP".to_string()];
        let metadata = Metadata {
            user_id: Some("user123".to_string()),
        };

        let params = MessageCreateParams::builder("test-model", 2048)
            .user("hello")
            .system("test system")
            .temperature(0.8)
            .top_p(0.9)
            .top_k(40)
            .stop_sequences(stop_seqs.clone())
            .metadata(metadata)
            .service_tier(ServiceTier::StandardOnly)
            .thinking(4096)
            .build();

        assert_eq!(params.model, "test-model");
        assert_eq!(params.max_tokens, 2048);
        assert_eq!(params.messages.len(), 1);
        assert_eq!(params.system, Some("test system".to_string()));
        assert_eq!(params.temperature, Some(0.8));
        assert_eq!(params.top_p, Some(0.9));
        assert_eq!(params.top_k, Some(40));
        assert_eq!(params.stop_sequences, Some(stop_seqs));
        assert!(params.metadata.is_some());
        assert_eq!(params.service_tier, Some(ServiceTier::StandardOnly));
        assert!(matches!(
            params.thinking,
            Some(ThinkingConfig::Enabled {
                budget_tokens: 4096
            })
        ));
    }

    #[test]
    fn test_thinking_config_builder_method() {
        // Test both .thinking() and .thinking_config()
        let params1 = MessageCreateParams::builder("test", 100)
            .thinking(2048)
            .build();

        assert!(matches!(
            params1.thinking,
            Some(ThinkingConfig::Enabled {
                budget_tokens: 2048
            })
        ));

        let params2 = MessageCreateParams::builder("test", 100)
            .thinking_config(ThinkingConfig::disabled())
            .build();

        assert!(matches!(params2.thinking, Some(ThinkingConfig::Disabled)));
    }

    #[test]
    fn test_message_create_params_serialization() {
        // Test that optional fields are properly skipped when None
        let minimal = MessageCreateParams::builder("test-model", 100)
            .user("hello")
            .build();

        let json = serde_json::to_string(&minimal).unwrap();

        // Required fields should be present
        assert!(json.contains("\"model\":\"test-model\""));
        assert!(json.contains("\"max_tokens\":100"));
        assert!(json.contains("\"messages\""));

        // Optional fields should not be present when None
        let should_not_contain = [
            "\"system\"",
            "\"temperature\"",
            "\"top_p\"",
            "\"top_k\"",
            "\"tools\"",
            "\"tool_choice\"",
            "\"stop_sequences\"",
            "\"stream\"",
            "\"metadata\"",
            "\"service_tier\"",
            "\"thinking\"",
        ];

        for field in should_not_contain {
            assert!(
                !json.contains(field),
                "Optional field {} should not be serialized",
                field
            );
        }
    }

    #[test]
    fn test_user_blocks_and_assistant_blocks() {
        // Test user_blocks creates correct role and content type
        let text_block = ContentBlockParam::Text {
            text: "Hello".to_string(),
            cache_control: None,
        };

        let user_msg = MessageParam::user_blocks(vec![text_block.clone()]);
        assert!(matches!(user_msg.role, Role::User));
        assert!(matches!(user_msg.content, MessageContent::Blocks(_)));

        // Test assistant_blocks creates correct role and content type
        let assistant_msg = MessageParam::assistant_blocks(vec![text_block]);
        assert!(matches!(assistant_msg.role, Role::Assistant));
        assert!(matches!(assistant_msg.content, MessageContent::Blocks(_)));
    }

    #[test]
    fn test_blocks_methods_with_empty_vec() {
        // Edge case: empty blocks vector
        let user_msg = MessageParam::user_blocks(vec![]);
        assert!(matches!(user_msg.role, Role::User));
        if let MessageContent::Blocks(blocks) = user_msg.content {
            assert!(blocks.is_empty());
        } else {
            panic!("Expected Blocks variant");
        }

        let assistant_msg = MessageParam::assistant_blocks(vec![]);
        assert!(matches!(assistant_msg.role, Role::Assistant));
        if let MessageContent::Blocks(blocks) = assistant_msg.content {
            assert!(blocks.is_empty());
        } else {
            panic!("Expected Blocks variant");
        }
    }

    #[test]
    fn test_blocks_methods_with_multiple_blocks() {
        // Test with multiple content blocks
        let blocks = vec![
            ContentBlockParam::Text {
                text: "First".to_string(),
                cache_control: None,
            },
            ContentBlockParam::Text {
                text: "Second".to_string(),
                cache_control: None,
            },
        ];

        let msg = MessageParam::user_blocks(blocks);
        if let MessageContent::Blocks(content_blocks) = msg.content {
            assert_eq!(content_blocks.len(), 2);
        } else {
            panic!("Expected Blocks variant");
        }
    }

    #[test]
    fn test_blocks_methods_serialization() {
        // Verify blocks methods produce correctly serializable messages
        let blocks = vec![ContentBlockParam::Text {
            text: "Test content".to_string(),
            cache_control: None,
        }];

        let user_msg = MessageParam::user_blocks(blocks.clone());
        let json = serde_json::to_string(&user_msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Test content\""));

        let assistant_msg = MessageParam::assistant_blocks(blocks);
        let json = serde_json::to_string(&assistant_msg).unwrap();
        assert!(json.contains("\"role\":\"assistant\""));
        assert!(json.contains("\"type\":\"text\""));
    }

    #[test]
    fn test_builder_assistant_explicit_verification() {
        // Explicit verification that builder .assistant() works correctly
        // (complementing test_builder_convenience_methods)
        let params = MessageCreateParams::builder("test-model", 1024)
            .assistant("response text")
            .build();

        assert_eq!(params.messages.len(), 1);
        let msg = &params.messages[0];
        assert!(matches!(msg.role, Role::Assistant));

        // Verify content is text, not blocks
        assert!(matches!(msg.content, MessageContent::Text(_)));
        if let MessageContent::Text(text) = &msg.content {
            assert_eq!(text, "response text");
        }
    }
}
