//! Model providers for LLM interactions
//!
//! This module contains the `ModelProvider` trait and implementations for
//! different LLM backends (Bedrock, Anthropic, etc.)

#[cfg(feature = "anthropic")]
pub mod anthropic;
#[cfg(feature = "bedrock")]
pub mod bedrock;
pub mod retry;

use crate::events::TokenUsage;
use crate::types::{Message, StopReason, ToolDefinition, ToolUseBlock};
use futures::stream::BoxStream;
use std::error::Error;

// Re-export provider types at provider level
#[cfg(feature = "anthropic")]
pub use anthropic::AnthropicProvider;
#[cfg(feature = "bedrock")]
pub use bedrock::{BedrockProvider, InferenceProfile};
pub use retry::{RetryCallback, RetryConfig, RetryInfo};

// Re-export ModelResponse from model module
pub use crate::model::ModelResponse;

/// Events from streaming model responses
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Incremental text delta
    TextDelta(String),
    /// Tool use detected
    ToolUse(ToolUseBlock),
    /// Incremental thinking delta (extended thinking)
    ThinkingDelta(String),
    /// Streaming stopped
    Stop {
        /// Why the model stopped
        stop_reason: StopReason,
        /// Token usage for this response (if available)
        usage: Option<TokenUsage>,
    },
}

/// Error types for model providers
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Authentication or authorization failed (expired tokens, invalid credentials, etc.)
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Rate limiting or throttling
    #[error("Rate limited: {0}")]
    RateLimited(String),

    /// Network or connectivity issues
    #[error("Network error: {0}")]
    Network(String),

    /// Model-specific errors (content filtered, context too long, etc.)
    #[error("Model error: {0}")]
    Model(String),

    /// Service unavailable or temporary issues
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Invalid configuration (bad model ID, missing parameters, etc.)
    #[error("Invalid configuration: {0}")]
    Configuration(String),

    /// Other provider-specific errors that don't fit above categories
    #[error("{0}")]
    Other(String),

    /// Communication error (legacy, kept for compatibility)
    #[error("Communication error: {0}")]
    Communication(#[from] Box<dyn Error + Send + Sync>),
}

/// Trait for model providers
///
/// This trait abstracts over different LLM providers (Bedrock, Anthropic, etc.)
/// allowing the Agent to work with any provider implementation.
///
/// A provider combines model metadata (name, token limits) with API interaction
/// (generate, streaming). Use the builder to create agents:
///
/// ```ignore
/// let agent = Agent::builder()
///     .bedrock(ClaudeSonnet4_5)
///     .build()
///     .await?;
/// ```
#[async_trait::async_trait]
pub trait ModelProvider: Send + Sync {
    /// Get the model name for display (e.g., "Claude Sonnet 4.5")
    fn name(&self) -> &str;

    /// Maximum input context tokens for this model
    fn max_context_tokens(&self) -> usize;

    /// Maximum output tokens this model can generate
    fn max_output_tokens(&self) -> usize;

    /// Estimate token count for text
    ///
    /// Providers should implement this to match their model's tokenization.
    /// Default implementation uses ~4 characters per token heuristic.
    fn estimate_token_count(&self, text: &str) -> usize {
        text.len().div_ceil(4)
    }

    /// Estimate token count for a conversation
    fn estimate_message_tokens(&self, messages: &[Message]) -> usize {
        let mut total = 0;
        for message in messages {
            total += 4; // Role overhead
            for block in &message.content {
                total += self.estimate_token_count(&format!("{:?}", block));
            }
        }
        total
    }

    /// Send a request to the model and get a response
    ///
    /// # Arguments
    /// * `messages` - The conversation history
    /// * `tools` - Available tools for the model to use
    /// * `system_prompt` - Optional system prompt
    async fn generate(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<ModelResponse, ProviderError>;

    /// Send a request and stream the response token-by-token (optional)
    ///
    /// # Arguments
    /// * `messages` - The conversation history
    /// * `tools` - Available tools for the model to use
    /// * `system_prompt` - Optional system prompt
    async fn generate_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        // Default implementation: call generate and return complete response
        let response = self.generate(messages, tools, system_prompt).await?;

        // Extract text content and tool uses from response message
        let mut text_content = String::new();
        let mut tool_uses = Vec::new();

        for content in &response.message.content {
            match content {
                crate::types::ContentBlock::Text(text) => {
                    text_content.push_str(text);
                }
                crate::types::ContentBlock::ToolUse(tool_use) => {
                    tool_uses.push(tool_use.clone());
                }
                _ => {}
            }
        }

        // Create a stream with the complete response
        let mut events = Vec::new();
        if !text_content.is_empty() {
            events.push(Ok(StreamEvent::TextDelta(text_content)));
        }
        for tool_use in tool_uses {
            events.push(Ok(StreamEvent::ToolUse(tool_use)));
        }
        events.push(Ok(StreamEvent::Stop {
            stop_reason: response.stop_reason,
            usage: response.usage,
        }));

        Ok(Box::pin(futures::stream::iter(events)))
    }
}

// Implement ModelProvider for Arc<dyn ModelProvider> to support dynamic dispatch
#[async_trait::async_trait]
impl ModelProvider for std::sync::Arc<dyn ModelProvider> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn max_context_tokens(&self) -> usize {
        (**self).max_context_tokens()
    }

    fn max_output_tokens(&self) -> usize {
        (**self).max_output_tokens()
    }

    fn estimate_token_count(&self, text: &str) -> usize {
        (**self).estimate_token_count(text)
    }

    fn estimate_message_tokens(&self, messages: &[Message]) -> usize {
        (**self).estimate_message_tokens(messages)
    }

    async fn generate(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        (**self).generate(messages, tools, system_prompt).await
    }

    async fn generate_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system_prompt: Option<String>,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        (**self)
            .generate_stream(messages, tools, system_prompt)
            .await
    }
}
