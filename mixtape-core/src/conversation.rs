//! Conversation management for context window handling
//!
//! This module provides the `ConversationManager` trait and implementations
//! for managing conversation history and context window limits.
//!
//! # Overview
//!
//! The `ConversationManager` is responsible for:
//! - Tracking all messages in a conversation
//! - Deciding which messages to include when calling the model (context selection)
//! - Preventing context window overflow
//!
//! # Implementations
//!
//! - [`SlidingWindowConversationManager`] - Default. Token-aware, keeps recent
//!   messages that fit within the context window. Never fails due to context overflow.
//! - [`SimpleConversationManager`] - Keeps last N messages. May fail if N is too large.
//! - [`NoOpConversationManager`] - Pass-through, no truncation. Fails on overflow.

use crate::types::Message;

/// Context limits for message selection
///
/// This struct provides the information needed by ConversationManager
/// to select which messages fit within the context window.
#[derive(Debug, Clone, Copy)]
pub struct ContextLimits {
    /// Maximum tokens available for context
    pub max_context_tokens: usize,
}

impl ContextLimits {
    /// Create new context limits
    pub fn new(max_context_tokens: usize) -> Self {
        Self { max_context_tokens }
    }
}

/// Information about context usage
#[derive(Debug, Clone)]
pub struct ContextUsage {
    /// Estimated token count for messages that will be sent
    pub context_tokens: usize,
    /// Total messages in full history
    pub total_messages: usize,
    /// Messages that will be sent to the model
    pub context_messages: usize,
    /// Maximum context tokens for the model
    pub max_context_tokens: usize,
    /// Percentage of context used (0.0 - 1.0)
    pub usage_percentage: f32,
}

/// Token estimator function type
///
/// Takes a slice of messages and returns the estimated token count.
pub type TokenEstimator<'a> = &'a dyn Fn(&[Message]) -> usize;

/// Trait for managing conversation context
///
/// A `ConversationManager` owns the full message history and decides
/// which messages to include when calling the model. This allows for
/// different strategies like sliding window, summarization, etc.
pub trait ConversationManager: Send + Sync {
    /// Add a message to the conversation history
    fn add_message(&mut self, message: Message);

    /// Get messages to send to the model (may be a subset of all messages)
    ///
    /// This method returns the messages that should be included in the next
    /// model call, respecting context window limits.
    ///
    /// # Arguments
    /// * `limits` - Context window limits
    /// * `estimate_tokens` - Function to estimate token count for messages
    fn messages_for_context(
        &self,
        limits: ContextLimits,
        estimate_tokens: TokenEstimator<'_>,
    ) -> Vec<Message>;

    /// Get all messages in the conversation (full history)
    fn all_messages(&self) -> &[Message];

    /// Restore conversation state from persisted messages
    fn hydrate(&mut self, messages: Vec<Message>);

    /// Clear all messages from the conversation
    fn clear(&mut self);

    /// Get context usage statistics
    fn context_usage(
        &self,
        limits: ContextLimits,
        estimate_tokens: TokenEstimator<'_>,
    ) -> ContextUsage {
        let context_messages = self.messages_for_context(limits, estimate_tokens);
        let context_tokens = estimate_tokens(&context_messages);
        let max_context_tokens = limits.max_context_tokens;

        ContextUsage {
            context_tokens,
            total_messages: self.all_messages().len(),
            context_messages: context_messages.len(),
            max_context_tokens,
            usage_percentage: if max_context_tokens > 0 {
                context_tokens as f32 / max_context_tokens as f32
            } else {
                0.0
            },
        }
    }
}

/// Sliding window conversation manager (default)
///
/// Keeps as many recent messages as will fit within the context window.
/// This implementation **never fails** due to context overflow - it will
/// always truncate old messages to fit.
///
/// The manager reserves space for the system prompt and leaves headroom
/// for the model's response.
///
/// # Example
/// ```
/// use mixtape_core::conversation::SlidingWindowConversationManager;
///
/// // Use defaults (10% reserved for system prompt, 20% for response)
/// let manager = SlidingWindowConversationManager::new();
///
/// // Or customize the reserved percentages
/// let manager = SlidingWindowConversationManager::with_reserve(0.15, 0.25);
/// ```
#[derive(Debug, Clone)]
pub struct SlidingWindowConversationManager {
    messages: Vec<Message>,
    /// Fraction of context to reserve for system prompt (0.0 - 1.0)
    system_prompt_reserve: f32,
    /// Fraction of context to reserve for model response (0.0 - 1.0)
    response_reserve: f32,
}

impl Default for SlidingWindowConversationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SlidingWindowConversationManager {
    /// Create a new sliding window manager with default reserves
    ///
    /// Defaults:
    /// - 10% reserved for system prompt
    /// - 20% reserved for model response
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            system_prompt_reserve: 0.10,
            response_reserve: 0.20,
        }
    }

    /// Create a manager with custom reserve percentages
    ///
    /// # Arguments
    /// * `system_prompt_reserve` - Fraction of context for system prompt (0.0 - 1.0)
    /// * `response_reserve` - Fraction of context for model response (0.0 - 1.0)
    pub fn with_reserve(system_prompt_reserve: f32, response_reserve: f32) -> Self {
        Self {
            messages: Vec::new(),
            system_prompt_reserve: system_prompt_reserve.clamp(0.0, 0.5),
            response_reserve: response_reserve.clamp(0.0, 0.5),
        }
    }

    /// Calculate available tokens for messages
    fn available_tokens(&self, limits: ContextLimits) -> usize {
        let max = limits.max_context_tokens;
        let reserved = (max as f32 * (self.system_prompt_reserve + self.response_reserve)) as usize;
        max.saturating_sub(reserved)
    }
}

impl ConversationManager for SlidingWindowConversationManager {
    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    fn messages_for_context(
        &self,
        limits: ContextLimits,
        estimate_tokens: TokenEstimator<'_>,
    ) -> Vec<Message> {
        let available = self.available_tokens(limits);

        // Start from the end and work backwards, keeping messages that fit
        let mut result = Vec::new();
        let mut total_tokens = 0;

        for message in self.messages.iter().rev() {
            let msg_tokens = estimate_tokens(std::slice::from_ref(message));

            if total_tokens + msg_tokens <= available {
                result.push(message.clone());
                total_tokens += msg_tokens;
            } else {
                // Can't fit any more messages
                break;
            }
        }

        // Reverse to restore chronological order
        result.reverse();
        result
    }

    fn all_messages(&self) -> &[Message] {
        &self.messages
    }

    fn hydrate(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }

    fn clear(&mut self) {
        self.messages.clear();
    }
}

/// Simple count-based conversation manager
///
/// Keeps the last N messages in the context. May fail if N messages
/// exceed the context window or if individual messages are very large.
///
/// Use this when you want predictable message counts rather than
/// token-based management.
///
/// # Example
/// ```
/// use mixtape_core::conversation::SimpleConversationManager;
///
/// // Keep last 50 messages
/// let manager = SimpleConversationManager::new(50);
/// ```
#[derive(Debug, Clone)]
pub struct SimpleConversationManager {
    messages: Vec<Message>,
    max_messages: usize,
}

impl SimpleConversationManager {
    /// Create a manager that keeps the last `max_messages` messages
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_messages,
        }
    }
}

impl ConversationManager for SimpleConversationManager {
    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    fn messages_for_context(
        &self,
        _limits: ContextLimits,
        _estimate_tokens: TokenEstimator<'_>,
    ) -> Vec<Message> {
        let start = self.messages.len().saturating_sub(self.max_messages);
        self.messages[start..].to_vec()
    }

    fn all_messages(&self) -> &[Message] {
        &self.messages
    }

    fn hydrate(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }

    fn clear(&mut self) {
        self.messages.clear();
    }
}

/// No-op conversation manager
///
/// Passes all messages through without any truncation.
/// **Will fail** if messages exceed the context window.
///
/// Use this for short, controlled conversations where you're confident
/// the context will never overflow.
///
/// # Example
/// ```
/// use mixtape_core::conversation::NoOpConversationManager;
///
/// let manager = NoOpConversationManager::new();
/// ```
#[derive(Debug, Clone, Default)]
pub struct NoOpConversationManager {
    messages: Vec<Message>,
}

impl NoOpConversationManager {
    /// Create a new no-op manager
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

impl ConversationManager for NoOpConversationManager {
    fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    fn messages_for_context(
        &self,
        _limits: ContextLimits,
        _estimate_tokens: TokenEstimator<'_>,
    ) -> Vec<Message> {
        self.messages.clone()
    }

    fn all_messages(&self) -> &[Message] {
        &self.messages
    }

    fn hydrate(&mut self, messages: Vec<Message>) {
        self.messages = messages;
    }

    fn clear(&mut self) {
        self.messages.clear();
    }
}

/// Boxed conversation manager for type erasure
pub type BoxedConversationManager = Box<dyn ConversationManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentBlock, Role};

    fn make_message(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text(text.to_string())],
        }
    }

    /// Simple token estimator: 1 token per character + 4 for message overhead
    fn estimate_tokens(messages: &[Message]) -> usize {
        messages.iter().map(|m| m.text().len() + 4).sum()
    }

    #[test]
    fn test_sliding_window_basic() {
        let mut manager = SlidingWindowConversationManager::new();
        let limits = ContextLimits::new(1000);

        manager.add_message(make_message("Hello"));
        manager.add_message(make_message("World"));

        let context = manager.messages_for_context(limits, &estimate_tokens);
        assert_eq!(context.len(), 2);
    }

    #[test]
    fn test_sliding_window_truncates() {
        let mut manager = SlidingWindowConversationManager::with_reserve(0.0, 0.0);
        // Very small context window
        let limits = ContextLimits::new(50);

        // Add messages that exceed context
        manager.add_message(make_message("This is a long message one"));
        manager.add_message(make_message("This is a long message two"));
        manager.add_message(make_message("Short"));

        let context = manager.messages_for_context(limits, &estimate_tokens);
        // Should only keep messages that fit, starting from most recent
        assert!(context.len() < 3);
        // Most recent message should be included
        assert_eq!(context.last().unwrap().text(), "Short");
    }

    #[test]
    fn test_sliding_window_hydrate() {
        let mut manager = SlidingWindowConversationManager::new();

        let messages = vec![
            make_message("One"),
            make_message("Two"),
            make_message("Three"),
        ];

        manager.hydrate(messages);
        assert_eq!(manager.all_messages().len(), 3);
    }

    #[test]
    fn test_simple_manager_limits() {
        let mut manager = SimpleConversationManager::new(2);
        let limits = ContextLimits::new(10000);

        manager.add_message(make_message("One"));
        manager.add_message(make_message("Two"));
        manager.add_message(make_message("Three"));
        manager.add_message(make_message("Four"));

        // All messages stored
        assert_eq!(manager.all_messages().len(), 4);

        // Only last 2 in context
        let context = manager.messages_for_context(limits, &estimate_tokens);
        assert_eq!(context.len(), 2);
        assert_eq!(context[0].text(), "Three");
        assert_eq!(context[1].text(), "Four");
    }

    #[test]
    fn test_noop_manager() {
        let mut manager = NoOpConversationManager::new();
        let limits = ContextLimits::new(10000);

        manager.add_message(make_message("One"));
        manager.add_message(make_message("Two"));
        manager.add_message(make_message("Three"));

        let context = manager.messages_for_context(limits, &estimate_tokens);
        assert_eq!(context.len(), 3);
    }

    #[test]
    fn test_context_usage() {
        let mut manager = SlidingWindowConversationManager::new();
        let limits = ContextLimits::new(1000);

        manager.add_message(make_message("Hello"));
        manager.add_message(make_message("World"));

        let usage = manager.context_usage(limits, &estimate_tokens);
        assert_eq!(usage.total_messages, 2);
        assert_eq!(usage.context_messages, 2);
        assert!(usage.usage_percentage > 0.0);
        assert!(usage.usage_percentage < 1.0);
    }

    #[test]
    fn test_clear() {
        let mut manager = SlidingWindowConversationManager::new();

        manager.add_message(make_message("Hello"));
        manager.add_message(make_message("World"));
        assert_eq!(manager.all_messages().len(), 2);

        manager.clear();
        assert_eq!(manager.all_messages().len(), 0);
    }
}
