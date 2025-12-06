//! Token counting types for the Anthropic Messages API
//!
//! This module contains types for the token counting endpoint,
//! which allows you to count tokens before making a request.
//!
//! # Example
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{Anthropic, CountTokensParams, MessageParam};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//!
//! let params = CountTokensParams {
//!     model: "claude-sonnet-4-20250514".to_string(),
//!     messages: vec![MessageParam::user("Hello!")],
//!     system: None,
//!     tools: None,
//! };
//!
//! let response = client.messages().count_tokens(params).await?;
//! println!("Input tokens: {}", response.input_tokens);
//! # Ok(())
//! # }
//! ```

use crate::messages::MessageParam;
use crate::tools::Tool;
use serde::{Deserialize, Serialize};

/// Parameters for counting tokens
#[derive(Debug, Clone, Serialize)]
pub struct CountTokensParams {
    /// The model to use for tokenization
    pub model: String,

    /// The messages to count tokens for
    pub messages: Vec<MessageParam>,

    /// System prompt (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Tools (optional) - included in token count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
}

impl CountTokensParams {
    /// Create a builder for CountTokensParams
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{CountTokensParams, MessageParam};
    ///
    /// let params = CountTokensParams::builder("claude-sonnet-4-20250514")
    ///     .messages(vec![MessageParam::user("Hello!")])
    ///     .system("You are helpful")
    ///     .build();
    /// ```
    pub fn builder(model: impl Into<String>) -> CountTokensParamsBuilder {
        CountTokensParamsBuilder::new(model)
    }
}

/// Builder for CountTokensParams
#[derive(Debug, Clone)]
pub struct CountTokensParamsBuilder {
    model: String,
    messages: Vec<MessageParam>,
    system: Option<String>,
    tools: Option<Vec<Tool>>,
}

impl CountTokensParamsBuilder {
    /// Create a new builder with the required model
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: Vec::new(),
            system: None,
            tools: None,
        }
    }

    /// Append messages to count tokens for
    ///
    /// Uses extend semantics: messages are added to any existing messages,
    /// matching the behavior of [`crate::MessageCreateParamsBuilder::messages`].
    pub fn messages(mut self, messages: impl IntoIterator<Item = MessageParam>) -> Self {
        self.messages.extend(messages);
        self
    }

    /// Add a single message
    pub fn message(mut self, message: MessageParam) -> Self {
        self.messages.push(message);
        self
    }

    /// Add a user message
    pub fn user(mut self, content: impl Into<crate::messages::MessageContent>) -> Self {
        self.messages.push(MessageParam::user(content));
        self
    }

    /// Add an assistant message
    pub fn assistant(mut self, content: impl Into<crate::messages::MessageContent>) -> Self {
        self.messages.push(MessageParam::assistant(content));
        self
    }

    /// Set the system prompt
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the tools
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Build the CountTokensParams
    pub fn build(self) -> CountTokensParams {
        CountTokensParams {
            model: self.model,
            messages: self.messages,
            system: self.system,
            tools: self.tools,
        }
    }
}

/// Response from the token counting API
#[derive(Debug, Clone, Deserialize)]
pub struct CountTokensResponse {
    /// Number of input tokens
    pub input_tokens: u32,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens_params_serialization() {
        let params = CountTokensParams {
            model: "claude-sonnet-4-20250514".to_string(),
            messages: vec![MessageParam::user("Hello")],
            system: Some("Be helpful".to_string()),
            tools: None,
        };

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"model\":\"claude-sonnet-4-20250514\""));
        assert!(json.contains("\"system\":\"Be helpful\""));
    }

    #[test]
    fn test_count_tokens_response_deserialization() {
        let json = r#"{"input_tokens": 42}"#;
        let response: CountTokensResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.input_tokens, 42);
    }

    #[test]
    fn test_builder_basic() {
        let params = CountTokensParams::builder("claude-sonnet-4-20250514")
            .messages(vec![MessageParam::user("Hello")])
            .build();

        assert_eq!(params.model, "claude-sonnet-4-20250514");
        assert_eq!(params.messages.len(), 1);
        assert!(params.system.is_none());
        assert!(params.tools.is_none());
    }

    #[test]
    fn test_builder_all_methods() {
        // Table-based test for all builder methods
        let tool = crate::tools::Tool {
            name: "test_tool".to_string(),
            description: Some("Test".to_string()),
            input_schema: crate::tools::ToolInputSchema::new(),
            cache_control: None,
            tool_type: None,
        };

        let params = CountTokensParams::builder("test-model")
            .messages(vec![MessageParam::user("msg1")])
            .system("test system")
            .tools(vec![tool])
            .build();

        assert_eq!(params.model, "test-model");
        assert_eq!(params.messages.len(), 1);
        assert_eq!(params.system, Some("test system".to_string()));
        assert_eq!(params.tools.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_builder_message_methods() {
        // Test all ways to add messages
        let cases = [
            (
                "single message method",
                CountTokensParams::builder("model")
                    .message(MessageParam::user("msg1"))
                    .build(),
                1,
            ),
            (
                "user convenience method",
                CountTokensParams::builder("model").user("msg1").build(),
                1,
            ),
            (
                "assistant convenience method",
                CountTokensParams::builder("model")
                    .assistant("msg1")
                    .build(),
                1,
            ),
            (
                "messages batch method",
                CountTokensParams::builder("model")
                    .messages(vec![MessageParam::user("msg1"), MessageParam::user("msg2")])
                    .build(),
                2,
            ),
            (
                "message then user",
                CountTokensParams::builder("model")
                    .message(MessageParam::user("msg1"))
                    .user("msg2")
                    .build(),
                2,
            ),
            (
                "user then assistant chain",
                CountTokensParams::builder("model")
                    .user("msg1")
                    .assistant("msg2")
                    .build(),
                2,
            ),
        ];

        for (name, params, expected_count) in cases {
            assert_eq!(params.messages.len(), expected_count, "case: {}", name);
        }
    }

    #[test]
    fn test_builder_assistant_role() {
        // Verify .assistant() creates correct role
        let params = CountTokensParams::builder("model")
            .user("user message")
            .assistant("assistant response")
            .build();

        assert_eq!(params.messages.len(), 2);
        assert!(matches!(
            params.messages[0].role,
            crate::messages::Role::User
        ));
        assert!(matches!(
            params.messages[1].role,
            crate::messages::Role::Assistant
        ));
    }

    #[test]
    fn test_builder_messages_extends() {
        // Test that .messages() extends (matching MessageCreateParamsBuilder behavior)
        let params = CountTokensParams::builder("model")
            .user("msg1")
            .messages(vec![MessageParam::user("msg2")])
            .build();

        // Should have both messages since .messages() extends
        assert_eq!(params.messages.len(), 2);
    }

    #[test]
    fn test_builder_string_conversions() {
        // Test that Into<String> conversions work correctly
        let str_ref: &str = "model-from-str";
        let string_owned: String = "system-from-string".to_string();

        let params = CountTokensParams::builder(str_ref)
            .system(string_owned)
            .user("user message")
            .build();

        assert_eq!(params.model, "model-from-str");
        assert_eq!(params.system, Some("system-from-string".to_string()));
    }

    #[test]
    fn test_builder_empty_messages() {
        // Edge case: builder with no messages
        let params = CountTokensParams::builder("model").build();

        assert_eq!(params.messages.len(), 0);
        assert!(params.system.is_none());
    }

    #[test]
    fn test_builder_serialization() {
        // Test that builder-created params serialize correctly
        let params = CountTokensParams::builder("test-model")
            .user("Hello")
            .system("Be helpful")
            .build();

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"model\":\"test-model\""));
        assert!(json.contains("\"system\":\"Be helpful\""));

        // Optional fields should be skipped when None
        let minimal = CountTokensParams::builder("model").user("hi").build();
        let json = serde_json::to_string(&minimal).unwrap();
        assert!(!json.contains("\"system\""));
        assert!(!json.contains("\"tools\""));
    }

    #[test]
    fn test_builder_edge_cases() {
        // Edge case: empty string inputs
        let params = CountTokensParams::builder("model")
            .user("")
            .assistant("")
            .system("")
            .build();

        assert_eq!(params.messages.len(), 2);
        assert_eq!(params.system, Some("".to_string()));

        // Edge case: multiple .assistant() calls in sequence
        let params = CountTokensParams::builder("model")
            .assistant("first")
            .assistant("second")
            .assistant("third")
            .build();

        assert_eq!(params.messages.len(), 3);

        // Edge case: interleaved user/assistant (typical conversation)
        let params = CountTokensParams::builder("model")
            .user("Hello")
            .assistant("Hi there!")
            .user("How are you?")
            .assistant("I'm doing well!")
            .build();

        assert_eq!(params.messages.len(), 4);
    }
}
