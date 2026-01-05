//! Common test utilities shared across test files.
//!
//! This module provides mock implementations and test helpers.
//! Items here may not be used by all test files, hence the module-level allow.
#![allow(dead_code)]

use async_trait::async_trait;
use mixtape_core::{
    permission::{Grant, GrantStore, GrantStoreError},
    AgentEvent, AgentHook, ContentBlock, Message, ModelProvider, ModelResponse, ProviderError,
    Role, StopReason, Tool, ToolDefinition, ToolError, ToolResult, ToolUseBlock,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// ===== Auto-Approve Grant Store for Testing =====

/// A grant store that auto-approves all tools (for testing).
pub struct AutoApproveGrantStore;

#[async_trait]
impl GrantStore for AutoApproveGrantStore {
    async fn save(&self, _grant: Grant) -> Result<(), GrantStoreError> {
        Ok(())
    }

    async fn load(&self, tool: &str) -> Result<Vec<Grant>, GrantStoreError> {
        // Return a tool-wide grant for any tool
        Ok(vec![Grant::tool(tool)])
    }

    async fn load_all(&self) -> Result<Vec<Grant>, GrantStoreError> {
        Ok(vec![])
    }

    async fn delete(
        &self,
        _tool: &str,
        _params_hash: Option<&str>,
    ) -> Result<bool, GrantStoreError> {
        Ok(true)
    }

    async fn clear(&self) -> Result<(), GrantStoreError> {
        Ok(())
    }
}

#[cfg(feature = "mcp")]
pub mod mock_mcp_server;

// ===== Test Tools =====

/// Input for the Calculator test tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CalculateInput {
    pub expression: String,
}

/// A simple calculator tool for testing
pub struct Calculator;

impl Tool for Calculator {
    type Input = CalculateInput;

    fn name(&self) -> &str {
        "calculate"
    }

    fn description(&self) -> &str {
        "Evaluate a mathematical expression"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Simple eval for testing - just handle "2+2"
        let result = if input.expression == "2+2" {
            "4"
        } else {
            "42" // Default for other expressions
        };

        Ok(ToolResult::Text(result.to_string()))
    }
}

/// Input for the DataTool test tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataInput {
    pub key: String,
}

/// A tool that returns structured JSON data for testing
pub struct DataTool;

impl Tool for DataTool {
    type Input = DataInput;

    fn name(&self) -> &str {
        "get_data"
    }

    fn description(&self) -> &str {
        "Get structured data"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let data = serde_json::json!({
            "key": input.key,
            "value": 42,
            "nested": {
                "field": "test"
            }
        });
        Ok(ToolResult::Json(data))
    }
}

/// A tool that always errors for testing error handling
pub struct ErrorTool;

impl Tool for ErrorTool {
    type Input = CalculateInput;

    fn name(&self) -> &str {
        "error_tool"
    }

    fn description(&self) -> &str {
        "A tool that errors"
    }

    async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        Err(ToolError::Custom("Intentional error".to_string()))
    }
}

// ===== Event Collectors for Hook Testing =====

/// Collects event types as strings for simple verification
#[derive(Clone)]
pub struct EventCollector {
    events: Arc<Mutex<Vec<String>>>,
}

impl EventCollector {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

impl AgentHook for EventCollector {
    fn on_event(&self, event: &AgentEvent) {
        let event_type = match event {
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
        };
        self.events.lock().unwrap().push(event_type.to_string());
    }
}

/// Collects full AgentEvent objects for detailed verification
#[derive(Clone)]
pub struct DetailedEventCollector {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl DetailedEventCollector {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AgentHook for DetailedEventCollector {
    fn on_event(&self, event: &AgentEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

/// A mock provider for testing that returns pre-programmed responses
#[derive(Clone)]
pub struct MockProvider {
    name: &'static str,
    responses: Arc<Mutex<Vec<ModelResponse>>>,
    call_count: Arc<Mutex<usize>>,
}

impl MockProvider {
    /// Create a new mock provider with no responses
    pub fn new() -> Self {
        Self {
            name: "MockProvider",
            responses: Arc::new(Mutex::new(Vec::new())),
            call_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Add a text response
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

    /// Add a tool use response
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

    /// Get the number of times converse was called
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

#[async_trait::async_trait]
impl ModelProvider for MockProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn max_context_tokens(&self) -> usize {
        200_000 // Same as Claude
    }

    fn max_output_tokens(&self) -> usize {
        8_192 // Same as Claude Sonnet
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
                "No more responses configured".to_string(),
            ));
        }

        Ok(responses.remove(0))
    }
}

// ===== Mock Session Store (for session feature tests) =====

#[cfg(feature = "session")]
use mixtape_core::{Session, SessionError, SessionStore, SessionSummary};

#[cfg(feature = "session")]
#[derive(Clone)]
pub struct MockSessionStore {
    sessions: Arc<Mutex<std::collections::HashMap<String, Session>>>,
    current_directory: String,
}

#[cfg(feature = "session")]
impl MockSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            current_directory: "/test/dir".to_string(),
        }
    }

    pub fn with_directory(mut self, dir: impl Into<String>) -> Self {
        self.current_directory = dir.into();
        self
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

#[cfg(feature = "session")]
#[async_trait::async_trait]
impl SessionStore for MockSessionStore {
    async fn get_or_create_session(&self) -> Result<Session, SessionError> {
        let mut sessions = self.sessions.lock().unwrap();

        // Find existing session for this directory
        if let Some(session) = sessions
            .values()
            .find(|s| s.directory == self.current_directory)
        {
            return Ok(session.clone());
        }

        // Create new session
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            directory: self.current_directory.clone(),
            messages: Vec::new(),
        };

        sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }

    async fn get_session(&self, id: &str) -> Result<Option<Session>, SessionError> {
        Ok(self.sessions.lock().unwrap().get(id).cloned())
    }

    async fn save_session(&self, session: &Session) -> Result<(), SessionError> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| SessionSummary {
                id: s.id.clone(),
                directory: s.directory.clone(),
                message_count: s.messages.len(),
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
            .collect())
    }

    async fn delete_session(&self, id: &str) -> Result<(), SessionError> {
        self.sessions.lock().unwrap().remove(id);
        Ok(())
    }
}
