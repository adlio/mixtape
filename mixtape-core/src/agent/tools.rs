//! Tool management and execution for Agent

use std::time::Instant;

use futures::stream::{self, StreamExt};
use serde_json::Value;

use crate::events::AgentEvent;
use crate::permission::{Authorization, AuthorizationResponse};
use crate::tool::{box_tool, ToolResult};
use crate::types::{Message, ToolResultBlock, ToolResultStatus, ToolUseBlock};

use super::types::{AgentError, ToolCallInfo, ToolInfo};
use super::Agent;

#[cfg(feature = "session")]
use crate::session::ToolCall;

impl Agent {
    /// Add a tool to the agent's toolbox
    pub fn add_tool<T: crate::tool::Tool + 'static>(&mut self, tool: T)
    where
        T::Input: serde::Serialize,
    {
        let tool_name = tool.name().to_string();

        // Check for duplicate tool names
        if self.tools.iter().any(|t| t.name() == tool_name) {
            eprintln!(
                "Warning: Tool '{}' is already registered. This will cause errors when calling the model.",
                tool_name
            );
            eprintln!("   Consider using .with_namespace() on MCP servers to avoid conflicts.");
        }

        self.tools.push(box_tool(tool));
    }

    /// List all configured tools
    pub fn list_tools(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|t| ToolInfo {
                name: t.name().to_string(),
                description: t.description().to_string(),
            })
            .collect()
    }

    /// Format tool input parameters for presentation
    ///
    /// Returns formatted string if the tool has a custom presenter,
    /// None otherwise (caller should fall back to JSON).
    pub fn format_tool_input(
        &self,
        tool_name: &str,
        params: &Value,
        context: crate::presentation::Display,
    ) -> Option<String> {
        let tool = self.tools.iter().find(|t| t.name() == tool_name)?;

        Some(match context {
            crate::presentation::Display::Cli => tool.format_input_ansi(params),
        })
    }

    /// Format tool output for presentation
    ///
    /// Returns formatted string for the tool output.
    pub fn format_tool_output(
        &self,
        tool_name: &str,
        result: &crate::tool::ToolResult,
        context: crate::presentation::Display,
    ) -> Option<String> {
        let tool = self.tools.iter().find(|t| t.name() == tool_name)?;

        Some(match context {
            crate::presentation::Display::Cli => tool.format_output_ansi(result),
        })
    }

    /// Execute a tool with approval checking
    pub(super) async fn execute_tool(
        &self,
        tool_use: &ToolUseBlock,
    ) -> Result<ToolResult, AgentError> {
        let tool_start = Instant::now();
        let tool_id = tool_use.id.clone();
        let tool_name = tool_use.name.clone();
        let input = tool_use.input.clone();

        // Emit ToolRequested (always fires exactly once)
        self.emit_event(AgentEvent::ToolRequested {
            tool_use_id: tool_id.clone(),
            name: tool_name.clone(),
            input: input.clone(),
        });

        // Validate that input is a JSON object (per Anthropic/Bedrock spec)
        if !input.is_object() {
            let type_name = match &input {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object", // Won't reach here
            };
            let error_msg = format!("Tool input must be a JSON object, got: {}", type_name);
            self.emit_event(AgentEvent::ToolFailed {
                tool_use_id: tool_id,
                name: tool_name,
                error: error_msg.clone(),
                duration: tool_start.elapsed(),
            });
            return Err(AgentError::InvalidToolInput(error_msg));
        }

        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_use.name)
            .ok_or_else(|| {
                self.emit_event(AgentEvent::ToolFailed {
                    tool_use_id: tool_id.clone(),
                    name: tool_name.clone(),
                    error: format!("Tool not found: {}", tool_name),
                    duration: tool_start.elapsed(),
                });
                AgentError::ToolNotFound(tool_name.clone())
            })?;

        // Check approval (emits permission events as needed)
        self.check_tool_approval(&tool_id, &tool_name, &input, tool_start)
            .await?;

        // Emit ToolExecuting (after permission granted)
        self.emit_event(AgentEvent::ToolExecuting {
            tool_use_id: tool_id.clone(),
            name: tool_name.clone(),
        });

        // Execute the tool
        match tool.execute_raw(input).await {
            Ok(result) => {
                self.emit_event(AgentEvent::ToolCompleted {
                    tool_use_id: tool_id,
                    name: tool_name,
                    output: result.clone(),
                    duration: tool_start.elapsed(),
                });
                Ok(result)
            }
            Err(e) => {
                let error_msg = e.to_string();
                self.emit_event(AgentEvent::ToolFailed {
                    tool_use_id: tool_id,
                    name: tool_name,
                    error: error_msg,
                    duration: tool_start.elapsed(),
                });
                Err(AgentError::Tool(e))
            }
        }
    }

    /// Check if a tool is authorized for execution
    async fn check_tool_approval(
        &self,
        tool_id: &str,
        tool_name: &str,
        input: &Value,
        tool_start: Instant,
    ) -> Result<(), AgentError> {
        let authorizer = self.authorizer.read().await;

        match authorizer.check(tool_name, input).await {
            Authorization::Granted { grant } => {
                self.emit_event(AgentEvent::PermissionGranted {
                    tool_use_id: tool_id.to_string(),
                    tool_name: tool_name.to_string(),
                    scope: Some(grant.scope),
                });
                Ok(())
            }
            Authorization::Denied { reason } => {
                self.emit_event(AgentEvent::PermissionDenied {
                    tool_use_id: tool_id.to_string(),
                    tool_name: tool_name.to_string(),
                    reason: reason.clone(),
                });
                self.emit_event(AgentEvent::ToolFailed {
                    tool_use_id: tool_id.to_string(),
                    name: tool_name.to_string(),
                    error: reason,
                    duration: tool_start.elapsed(),
                });
                Err(AgentError::ToolDenied(tool_name.to_string()))
            }
            Authorization::PendingApproval { params_hash } => {
                // Need to drop the lock before requesting authorization
                drop(authorizer);

                self.request_authorization(tool_id, tool_name, input, params_hash, tool_start)
                    .await
            }
        }
    }

    /// Request authorization for a tool
    async fn request_authorization(
        &self,
        tool_id: &str,
        tool_name: &str,
        input: &Value,
        params_hash: String,
        tool_start: Instant,
    ) -> Result<(), AgentError> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AuthorizationResponse>(1);

        // Use tool_id as proposal_id for consistency
        let proposal_id = tool_id.to_string();

        // Register pending authorization
        {
            let mut pending = self.pending_authorizations.write().await;
            pending.insert(proposal_id.clone(), tx);
        }

        // Emit permission required event
        self.emit_event(AgentEvent::PermissionRequired {
            proposal_id: proposal_id.clone(),
            tool_name: tool_name.to_string(),
            params: input.clone(),
            params_hash: params_hash.clone(),
        });

        // Wait for response with timeout
        let response = match tokio::time::timeout(self.authorization_timeout, rx.recv()).await {
            Ok(Some(response)) => response,
            Ok(None) => AuthorizationResponse::Deny {
                reason: Some("Channel closed".to_string()),
            },
            Err(_) => {
                self.emit_event(AgentEvent::PermissionDenied {
                    tool_use_id: tool_id.to_string(),
                    tool_name: tool_name.to_string(),
                    reason: "Authorization request timed out".to_string(),
                });
                AuthorizationResponse::Deny {
                    reason: Some("Timeout".to_string()),
                }
            }
        };

        // Clean up pending authorization
        {
            let mut pending = self.pending_authorizations.write().await;
            pending.remove(&proposal_id);
        }

        match response {
            AuthorizationResponse::Once => {
                self.emit_event(AgentEvent::PermissionGranted {
                    tool_use_id: tool_id.to_string(),
                    tool_name: tool_name.to_string(),
                    scope: None,
                });
                Ok(())
            }
            AuthorizationResponse::Trust { grant } => {
                // Save the grant to the authorizer
                let authorizer = self.authorizer.read().await;
                let result = if grant.is_tool_wide() {
                    authorizer.grant_tool(&grant.tool).await
                } else if let Some(ref hash) = grant.params_hash {
                    authorizer.grant_params_hash(&grant.tool, hash).await
                } else {
                    authorizer.grant_tool(&grant.tool).await
                };
                if let Err(e) = result {
                    eprintln!("Warning: Failed to save grant: {}", e);
                }
                self.emit_event(AgentEvent::PermissionGranted {
                    tool_use_id: tool_id.to_string(),
                    tool_name: tool_name.to_string(),
                    scope: Some(grant.scope),
                });
                Ok(())
            }
            AuthorizationResponse::Deny { reason } => {
                let reason_str =
                    reason.unwrap_or_else(|| "Authorization denied by user".to_string());
                self.emit_event(AgentEvent::PermissionDenied {
                    tool_use_id: tool_id.to_string(),
                    tool_name: tool_name.to_string(),
                    reason: reason_str,
                });
                self.emit_event(AgentEvent::ToolFailed {
                    tool_use_id: tool_id.to_string(),
                    name: tool_name.to_string(),
                    error: "Tool execution denied by user".to_string(),
                    duration: tool_start.elapsed(),
                });
                Err(AgentError::ToolDenied(tool_name.to_string()))
            }
        }
    }

    /// Process tool calls from a model response
    ///
    /// Executes all tool calls in parallel (up to max_concurrent_tools),
    /// collecting results and recording statistics.
    pub(super) async fn process_tool_calls(
        &self,
        message: &Message,
        tool_call_infos: &mut Vec<ToolCallInfo>,
        #[cfg(feature = "session")] session_tool_calls: &mut Vec<ToolCall>,
        #[cfg(feature = "session")] session_tool_results: &mut Vec<crate::session::ToolResult>,
    ) -> Vec<ToolResultBlock> {
        let tool_uses = message.tool_uses();
        let tool_use_blocks: Vec<_> = tool_uses.into_iter().cloned().collect();

        // Execute tools in parallel with concurrency limit
        let futures: Vec<_> = tool_use_blocks
            .iter()
            .map(|tool_use| {
                let tool_use = tool_use.clone();
                async move {
                    let start = Instant::now();
                    let result = self.execute_tool(&tool_use).await;
                    let duration = start.elapsed();
                    (tool_use, result, duration)
                }
            })
            .collect();

        let results: Vec<_> = stream::iter(futures)
            .buffer_unordered(self.max_concurrent_tools)
            .collect()
            .await;

        results
            .into_iter()
            .map(|(tool_use, result, duration)| {
                // Record tool call for session
                #[cfg(feature = "session")]
                {
                    session_tool_calls.push(ToolCall {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input: tool_use.input.to_string(),
                    });
                }

                match result {
                    Ok(ref tool_result) => {
                        // Record tool call info for response
                        tool_call_infos.push(ToolCallInfo {
                            name: tool_use.name.clone(),
                            input: tool_use.input.clone(),
                            output: tool_result.as_text(),
                            success: true,
                            duration,
                        });

                        // Record tool result for session
                        #[cfg(feature = "session")]
                        {
                            session_tool_results.push(crate::session::ToolResult {
                                tool_use_id: tool_use.id.clone(),
                                success: true,
                                content: tool_result.as_text(),
                            });
                        }

                        ToolResultBlock {
                            tool_use_id: tool_use.id,
                            content: tool_result.clone(),
                            status: ToolResultStatus::Success,
                        }
                    }
                    Err(ref e) => {
                        let error_msg = format!("Error: {}", e);

                        // Record tool call info for response
                        tool_call_infos.push(ToolCallInfo {
                            name: tool_use.name.clone(),
                            input: tool_use.input.clone(),
                            output: error_msg.clone(),
                            success: false,
                            duration,
                        });

                        // Record tool error for session
                        #[cfg(feature = "session")]
                        {
                            session_tool_results.push(crate::session::ToolResult {
                                tool_use_id: tool_use.id.clone(),
                                success: false,
                                content: error_msg.clone(),
                            });
                        }

                        ToolResultBlock {
                            tool_use_id: tool_use.id,
                            content: ToolResult::Text(error_msg),
                            status: ToolResultStatus::Error,
                        }
                    }
                }
            })
            .collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ModelProvider, ProviderError};
    use crate::tool::{Tool, ToolError, ToolResult as MxToolResult};
    use crate::types::{ContentBlock, Message, Role, StopReason, ToolDefinition, ToolUseBlock};
    use crate::{Agent, ModelResponse};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    /// Mock provider for testing
    #[derive(Clone)]
    struct MockProvider {
        responses: Arc<parking_lot::Mutex<Vec<ModelResponse>>>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                responses: Arc::new(parking_lot::Mutex::new(Vec::new())),
            }
        }

        fn with_text(self, text: impl Into<String>) -> Self {
            let message = Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text(text.into())],
            };
            let response = ModelResponse {
                message,
                stop_reason: StopReason::EndTurn,
                usage: None,
            };
            self.responses.lock().push(response);
            self
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
            let mut responses = self.responses.lock();
            if responses.is_empty() {
                return Err(ProviderError::Other("No more responses".to_string()));
            }
            Ok(responses.remove(0))
        }
    }

    /// Input for the Echo test tool
    #[derive(Debug, Deserialize, Serialize, JsonSchema)]
    struct EchoInput {
        message: String,
    }

    /// Simple test tool that echoes input
    struct EchoTool;

    impl Tool for EchoTool {
        type Input = EchoInput;

        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echoes the input back"
        }

        async fn execute(&self, input: Self::Input) -> Result<MxToolResult, ToolError> {
            Ok(MxToolResult::text(input.message))
        }
    }

    /// Input for the Add test tool
    #[derive(Debug, Deserialize, Serialize, JsonSchema)]
    struct AddInput {
        a: f64,
        b: f64,
    }

    /// Simple test tool that adds two numbers
    struct AddTool;

    impl Tool for AddTool {
        type Input = AddInput;

        fn name(&self) -> &str {
            "add"
        }

        fn description(&self) -> &str {
            "Adds two numbers"
        }

        async fn execute(&self, input: Self::Input) -> Result<MxToolResult, ToolError> {
            Ok(MxToolResult::text(format!("{}", input.a + input.b)))
        }
    }

    /// Input for the FailingTool test tool
    #[derive(Debug, Deserialize, Serialize, JsonSchema)]
    struct EmptyInput {}

    /// Tool that always fails
    struct FailingTool;

    impl Tool for FailingTool {
        type Input = EmptyInput;

        fn name(&self) -> &str {
            "failing_tool"
        }

        fn description(&self) -> &str {
            "A tool that always fails"
        }

        async fn execute(&self, _input: Self::Input) -> Result<MxToolResult, ToolError> {
            Err(ToolError::Custom("Tool execution failed".to_string()))
        }
    }

    // ===== add_tool Tests =====

    #[tokio::test]
    async fn test_add_tool() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        // Initially no tools
        assert_eq!(agent.list_tools().len(), 0);

        // Add a tool
        agent.add_tool(EchoTool);

        // Should have one tool
        let tools = agent.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "Echoes the input back");
    }

    #[tokio::test]
    async fn test_add_multiple_tools() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);
        agent.add_tool(AddTool);

        let tools = agent.list_tools();
        assert_eq!(tools.len(), 2);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"add"));
    }

    #[tokio::test]
    async fn test_add_tool_with_builder() {
        let provider = MockProvider::new().with_text("ok");
        let agent = Agent::builder()
            .provider(provider)
            .add_tool(EchoTool)
            .add_tool(AddTool)
            .build()
            .await
            .unwrap();

        let tools = agent.list_tools();
        assert_eq!(tools.len(), 2);
    }

    // ===== list_tools Tests =====

    #[tokio::test]
    async fn test_list_tools_empty() {
        let provider = MockProvider::new().with_text("ok");
        let agent = Agent::builder().provider(provider).build().await.unwrap();

        let tools = agent.list_tools();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_list_tools_preserves_order() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);
        agent.add_tool(AddTool);
        agent.add_tool(FailingTool);

        let tools = agent.list_tools();
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[1].name, "add");
        assert_eq!(tools[2].name, "failing_tool");
    }

    // ===== execute_tool Tests =====

    #[tokio::test]
    async fn test_execute_tool_success() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);

        // Grant permission to the echo tool
        agent
            .authorizer()
            .write()
            .await
            .grant_tool("echo")
            .await
            .unwrap();

        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "echo".to_string(),
            input: serde_json::json!({"message": "Hello, world!"}),
        };

        let result = agent.execute_tool(&tool_use).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_text(), "Hello, world!");
    }

    #[tokio::test]
    async fn test_execute_tool_not_found() {
        let provider = MockProvider::new().with_text("ok");
        let agent = Agent::builder().provider(provider).build().await.unwrap();

        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "nonexistent_tool".to_string(),
            input: serde_json::json!({}),
        };

        let result = agent.execute_tool(&tool_use).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AgentError::ToolNotFound(_)));
    }

    #[tokio::test]
    async fn test_execute_tool_invalid_input_not_object() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);

        // Test with string input (not an object)
        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "echo".to_string(),
            input: serde_json::json!("not an object"),
        };

        let result = agent.execute_tool(&tool_use).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AgentError::InvalidToolInput(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_tool_invalid_input_array() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);

        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "echo".to_string(),
            input: serde_json::json!([1, 2, 3]),
        };

        let result = agent.execute_tool(&tool_use).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        if let AgentError::InvalidToolInput(msg) = &err {
            assert!(msg.contains("array"));
        }
    }

    #[tokio::test]
    async fn test_execute_tool_invalid_input_null() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);

        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "echo".to_string(),
            input: serde_json::Value::Null,
        };

        let result = agent.execute_tool(&tool_use).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        if let AgentError::InvalidToolInput(msg) = &err {
            assert!(msg.contains("null"));
        }
    }

    #[tokio::test]
    async fn test_execute_tool_execution_failure() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(FailingTool);

        // Grant permission to the failing tool
        agent
            .authorizer()
            .write()
            .await
            .grant_tool("failing_tool")
            .await
            .unwrap();

        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "failing_tool".to_string(),
            input: serde_json::json!({}),
        };

        let result = agent.execute_tool(&tool_use).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AgentError::Tool(_)));
    }

    // ===== format_tool_input/output Tests =====

    #[tokio::test]
    async fn test_format_tool_input_existing_tool() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);

        let params = serde_json::json!({"message": "test"});
        let formatted = agent.format_tool_input("echo", &params, crate::presentation::Display::Cli);

        // Should return some formatted output
        assert!(formatted.is_some());
    }

    #[tokio::test]
    async fn test_format_tool_input_nonexistent_tool() {
        let provider = MockProvider::new().with_text("ok");
        let agent = Agent::builder().provider(provider).build().await.unwrap();

        let params = serde_json::json!({"message": "test"});
        let formatted =
            agent.format_tool_input("nonexistent", &params, crate::presentation::Display::Cli);

        assert!(formatted.is_none());
    }

    #[tokio::test]
    async fn test_format_tool_output_existing_tool() {
        let provider = MockProvider::new().with_text("ok");
        let mut agent = Agent::builder().provider(provider).build().await.unwrap();

        agent.add_tool(EchoTool);

        let result = crate::tool::ToolResult::text("output");
        let formatted =
            agent.format_tool_output("echo", &result, crate::presentation::Display::Cli);

        assert!(formatted.is_some());
    }

    #[tokio::test]
    async fn test_format_tool_output_nonexistent_tool() {
        let provider = MockProvider::new().with_text("ok");
        let agent = Agent::builder().provider(provider).build().await.unwrap();

        let result = crate::tool::ToolResult::text("output");
        let formatted =
            agent.format_tool_output("nonexistent", &result, crate::presentation::Display::Cli);

        assert!(formatted.is_none());
    }
}
