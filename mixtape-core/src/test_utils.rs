//! Test utilities for mixtape-core.
//!
//! This module provides mock implementations for testing agents without
//! requiring real LLM provider credentials.
//!
//! Enable with the `test-utils` feature:
//!
//! ```toml
//! [dev-dependencies]
//! mixtape-core = { version = "...", features = ["test-utils"] }
//! ```
//!
//! # Example
//!
//! ```rust
//! use mixtape_core::{Agent, test_utils::MockProvider};
//!
//! # async fn example() -> mixtape_core::Result<()> {
//! let provider = MockProvider::new()
//!     .with_text("Hello from mock!");
//!
//! let agent = Agent::builder()
//!     .provider(provider)
//!     .build()
//!     .await?;
//!
//! let response = agent.run("Hi").await?;
//! assert_eq!(response.text(), "Hello from mock!");
//! # Ok(())
//! # }
//! ```

use std::sync::{Arc, Mutex};

use crate::events::AgentEvent;
use crate::model::ModelResponse;
use crate::provider::{ModelProvider, ProviderError};
use crate::types::{ContentBlock, Message, Role, StopReason, ToolDefinition, ToolUseBlock};

/// A mock model provider for testing.
///
/// Returns pre-programmed responses in order. Useful for testing agent behavior
/// without making real API calls.
///
/// # Example
///
/// ```rust
/// use mixtape_core::test_utils::MockProvider;
/// use serde_json::json;
///
/// // Simple text response
/// let provider = MockProvider::new()
///     .with_text("Hello!");
///
/// // Tool use followed by final response
/// let provider = MockProvider::new()
///     .with_tool_use("calculator", json!({"expr": "2+2"}))
///     .with_text("The answer is 4");
/// ```
#[derive(Clone)]
pub struct MockProvider {
    responses: Arc<Mutex<Vec<ModelResponse>>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockProvider {
    /// Create a new mock provider with no responses.
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(Vec::new())),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Add a text response to the queue.
    ///
    /// The response will have `StopReason::EndTurn`.
    pub fn with_text(self, text: impl Into<String>) -> Self {
        let message = Message::assistant(text);

        let response = ModelResponse {
            message,
            stop_reason: StopReason::EndTurn,
            usage: None,
        };

        self.responses.lock().unwrap().push(response);
        self
    }

    /// Add a tool use response to the queue.
    ///
    /// The response will have `StopReason::ToolUse`.
    pub fn with_tool_use(
        self,
        tool_name: impl Into<String>,
        tool_input: serde_json::Value,
    ) -> Self {
        let tool_use = ToolUseBlock {
            id: format!("tool_{}", uuid::Uuid::new_v4()),
            name: tool_name.into(),
            input: tool_input,
        };

        let message = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(tool_use)],
        };

        let response = ModelResponse {
            message,
            stop_reason: StopReason::ToolUse,
            usage: None,
        };

        self.responses.lock().unwrap().push(response);
        self
    }

    /// Get the number of times `generate` was called.
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ModelProvider for MockProvider {
    fn name(&self) -> &str {
        "MockProvider"
    }

    fn max_context_tokens(&self) -> usize {
        200_000
    }

    fn max_output_tokens(&self) -> usize {
        8_192
    }

    async fn generate(
        &self,
        _messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
        _system_prompt: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        let mut count = self.call_count.lock().unwrap();
        *count += 1;

        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(ProviderError::Other(
                "MockProvider: No more responses configured".to_string(),
            ));
        }

        Ok(responses.remove(0))
    }
}

/// Collects agent events for verification in tests.
///
/// Stores full [`AgentEvent`] objects and provides convenience methods
/// for inspecting event types.
///
/// # Example
///
/// ```rust
/// use mixtape_core::{Agent, test_utils::{MockProvider, EventCollector}};
///
/// # async fn example() -> mixtape_core::Result<()> {
/// let provider = MockProvider::new().with_text("Hello!");
/// let collector = EventCollector::new();
///
/// let agent = Agent::builder()
///     .provider(provider)
///     .build()
///     .await?;
///
/// agent.add_hook(collector.clone());
/// agent.run("Hi").await?;
///
/// assert!(collector.has_event("run_started"));
/// assert!(collector.has_event("run_completed"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct EventCollector {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl EventCollector {
    /// Create a new event collector.
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get all collected events.
    pub fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Get all collected event type names.
    pub fn event_types(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|e| Self::event_type_name(e).to_string())
            .collect()
    }

    /// Clear all collected events.
    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }

    /// Check if a specific event type was collected.
    pub fn has_event(&self, event_type: &str) -> bool {
        self.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| Self::event_type_name(e) == event_type)
    }

    /// Count occurrences of a specific event type.
    pub fn count_event(&self, event_type: &str) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| Self::event_type_name(e) == event_type)
            .count()
    }

    /// Get the number of collected events.
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Check if no events have been collected.
    pub fn is_empty(&self) -> bool {
        self.events.lock().unwrap().is_empty()
    }

    fn event_type_name(event: &AgentEvent) -> &'static str {
        match event {
            AgentEvent::RunStarted { .. } => "run_started",
            AgentEvent::RunCompleted { .. } => "run_completed",
            AgentEvent::RunFailed { .. } => "run_failed",
            AgentEvent::ModelCallStarted { .. } => "model_call_started",
            AgentEvent::ModelCallStreaming { .. } => "model_streaming",
            AgentEvent::ModelCallCompleted { .. } => "model_call_completed",
            AgentEvent::ToolRequested { .. } => "tool_requested",
            AgentEvent::ToolExecuting { .. } => "tool_executing",
            AgentEvent::ToolCompleted { .. } => "tool_completed",
            AgentEvent::ToolFailed { .. } => "tool_failed",
            AgentEvent::PermissionRequired { .. } => "permission_required",
            AgentEvent::PermissionGranted { .. } => "permission_granted",
            AgentEvent::PermissionDenied { .. } => "permission_denied",
            #[cfg(feature = "session")]
            AgentEvent::SessionResumed { .. } => "session_resumed",
            #[cfg(feature = "session")]
            AgentEvent::SessionSaved { .. } => "session_saved",
        }
    }
}

impl Default for EventCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::events::AgentHook for EventCollector {
    fn on_event(&self, event: &AgentEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_provider_text_response() {
        let provider = MockProvider::new().with_text("Hello!");
        assert_eq!(provider.call_count(), 0);
    }

    #[test]
    fn test_mock_provider_chained_responses() {
        let provider = MockProvider::new()
            .with_tool_use("calculator", serde_json::json!({"expr": "2+2"}))
            .with_text("The answer is 4");

        // Verify both responses were queued
        assert_eq!(provider.call_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_provider_generate() {
        let provider = MockProvider::new()
            .with_text("Response 1")
            .with_text("Response 2");

        let response1 = provider.generate(vec![], vec![], None).await.unwrap();
        assert_eq!(provider.call_count(), 1);
        assert!(response1.message.text().contains("Response 1"));

        let response2 = provider.generate(vec![], vec![], None).await.unwrap();
        assert_eq!(provider.call_count(), 2);
        assert!(response2.message.text().contains("Response 2"));

        // Should error when exhausted
        let result = provider.generate(vec![], vec![], None).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_event_collector() {
        let collector = EventCollector::new();
        assert!(collector.is_empty());

        // Simulate adding events directly for testing
        collector
            .events
            .lock()
            .unwrap()
            .push(AgentEvent::RunStarted {
                input: "test".to_string(),
                timestamp: std::time::Instant::now(),
            });
        collector
            .events
            .lock()
            .unwrap()
            .push(AgentEvent::RunCompleted {
                output: "done".to_string(),
                duration: std::time::Duration::from_secs(1),
            });

        assert_eq!(collector.len(), 2);
        assert!(collector.has_event("run_started"));
        assert!(collector.has_event("run_completed"));
        assert!(!collector.has_event("run_failed"));
        assert_eq!(collector.count_event("run_started"), 1);

        let types = collector.event_types();
        assert_eq!(types, vec!["run_started", "run_completed"]);

        collector.clear();
        assert!(collector.is_empty());
    }
}
