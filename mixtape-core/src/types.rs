//! Provider-agnostic types for messages and tools
//!
//! These types abstract over provider-specific SDKs (AWS Bedrock, OpenAI, Anthropic API)
//! allowing the Agent and Model trait to work with any backend.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Role of a message in the conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
        }
    }
}

/// A message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Create a new user message with text content
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text(text.into())],
        }
    }

    /// Create a new assistant message with text content
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text(text.into())],
        }
    }

    /// Create a new user message with tool results
    pub fn tool_results(results: Vec<ToolResultBlock>) -> Self {
        Self {
            role: Role::User,
            content: results.into_iter().map(ContentBlock::ToolResult).collect(),
        }
    }

    /// Create an assistant message with text and tool use blocks
    ///
    /// This is useful for constructing multi-turn conversations where the
    /// assistant both provides text and requests tool calls.
    pub fn assistant_with_tool_use(text: impl Into<String>, tool_uses: Vec<ToolUseBlock>) -> Self {
        let mut content = vec![ContentBlock::Text(text.into())];
        content.extend(tool_uses.into_iter().map(ContentBlock::ToolUse));
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// Create an assistant message with arbitrary content blocks
    ///
    /// This provides full control over the message content, useful for
    /// complex scenarios like thinking blocks or mixed content.
    pub fn assistant_with_content(content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// Get all text content concatenated
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Get all tool use blocks
    pub fn tool_uses(&self) -> Vec<&ToolUseBlock> {
        self.content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::ToolUse(t) => Some(t),
                _ => None,
            })
            .collect()
    }
}

/// Content block within a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content
    Text(String),
    /// Tool use request from assistant
    ToolUse(ToolUseBlock),
    /// Tool result from user
    ToolResult(ToolResultBlock),
    /// Thinking block from extended thinking
    Thinking {
        /// The model's thinking content
        thinking: String,
        /// Signature for multi-turn thinking verification
        signature: String,
    },
    /// Server tool use (server-side tools like web search or tool search)
    ///
    /// These are informational blocks showing server-side tool invocations.
    /// Developers don't need to execute these - they're handled by the API.
    ServerToolUse(ServerToolUseBlock),
    /// Tool search result from server
    ///
    /// Contains references to tools discovered via tool search.
    /// The API automatically expands these references.
    ToolSearchResult(ToolSearchResultBlock),
}

/// A tool use request from the model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseBlock {
    /// Unique ID for this tool use (used to match with result)
    pub id: String,
    /// Tool name
    pub name: String,
    /// Tool input parameters as JSON
    pub input: Value,
}

/// A server-side tool use block (informational)
///
/// These blocks are returned when the API invokes server-side tools
/// like web search or tool search. Developers see these for transparency
/// but don't need to execute them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolUseBlock {
    /// Unique ID for this tool use
    pub id: String,
    /// Tool name (e.g., "web_search_tool", "tool_search_tool")
    pub name: String,
    /// Tool input parameters as JSON
    pub input: Value,
}

/// Result from a tool search operation
///
/// Contains references to tools discovered via the tool search API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSearchResultBlock {
    /// ID of the tool search request this result belongs to
    pub tool_use_id: String,
    /// Discovered tool references
    pub tool_references: Vec<ToolReference>,
}

/// A reference to a discovered tool from tool search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReference {
    /// Name of the discovered tool
    pub name: String,
}

impl ToolReference {
    /// Create a new tool reference
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultBlock {
    /// ID of the tool use this is a result for
    pub tool_use_id: String,
    /// Result content (text or structured)
    pub content: crate::tool::ToolResult,
    /// Whether the tool execution succeeded
    pub status: ToolResultStatus,
}

/// Status of a tool result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolResultStatus {
    Success,
    Error,
}

/// Definition of a tool available to the model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name (must match the tool's name() method)
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: Value,
    /// Whether this tool should be deferred until discovered via tool search
    ///
    /// When `true`, the tool's full definition is not loaded into context
    /// until Claude discovers it via the tool search tool. This is useful
    /// for large tool catalogs (30+ tools) to save context tokens.
    #[serde(default)]
    pub defer_loading: bool,
}

/// Why the model stopped generating
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of response
    EndTurn,
    /// Model wants to use a tool
    ToolUse,
    /// Hit max token limit
    MaxTokens,
    /// Content was filtered
    ContentFiltered,
    /// Stop sequence encountered
    StopSequence,
    /// Model paused for extended thinking continuation
    PauseTurn,
    /// Unknown/other reason
    #[default]
    Unknown,
}

/// Configuration for extended thinking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingConfig {
    /// Enable extended thinking with a token budget
    Enabled {
        /// Token budget for thinking (must be >= 1024 and < max_tokens)
        budget_tokens: u32,
    },
    /// Disable extended thinking
    Disabled,
}

/// Search algorithm type for tool search
///
/// Claude can use different algorithms to search your tool catalog:
/// - **Regex**: Claude uses regex patterns like `"weather"`, `"get_.*_data"` - more precise
/// - **Bm25**: Claude uses natural language queries - better for semantic matching
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolSearchType {
    /// Regex-based search using patterns like `"weather"`, `"get_.*_data"`
    ///
    /// More precise matching based on tool names and patterns.
    #[default]
    Regex,
    /// BM25-based semantic search using natural language queries
    ///
    /// Better for semantic matching when tool names don't follow patterns.
    Bm25,
}

impl ThinkingConfig {
    /// Create enabled thinking config with specified budget
    pub fn enabled(budget_tokens: u32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Create disabled thinking config
    pub fn disabled() -> Self {
        Self::Disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolResult;

    #[test]
    fn test_role_display() {
        assert_eq!(format!("{}", Role::User), "user");
        assert_eq!(format!("{}", Role::Assistant), "assistant");
    }

    #[test]
    fn test_tool_result_from_string() {
        let result: ToolResult = String::from("hello world").into();
        assert!(matches!(result, ToolResult::Text(s) if s == "hello world"));
    }

    #[test]
    fn test_tool_result_from_str() {
        let result: ToolResult = "hello world".into();
        assert!(matches!(result, ToolResult::Text(s) if s == "hello world"));
    }

    // ===== Message Helper Tests =====

    #[test]
    fn test_message_user_creation() {
        let cases = [
            ("simple text", "simple text"),
            ("", ""),
            ("multi\nline", "multi\nline"),
            ("with unicode: 你好 🦀", "with unicode: 你好 🦀"),
        ];

        for (name, input) in cases {
            let msg = Message::user(input);
            assert_eq!(msg.role, Role::User, "case: {}", name);
            assert_eq!(msg.content.len(), 1, "case: {}", name);
            assert_eq!(msg.text(), input, "case: {}", name);
        }
    }

    #[test]
    fn test_message_user_from_string_type() {
        // Test that impl Into<String> works
        let msg = Message::user(String::from("owned string"));
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "owned string");
    }

    #[test]
    fn test_message_assistant_creation() {
        let msg = Message::assistant("hello");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.text(), "hello");
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn test_message_text_concatenation() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text("Hello ".to_string()),
                ContentBlock::Text("world".to_string()),
            ],
        };
        assert_eq!(msg.text(), "Hello world");
    }

    #[test]
    fn test_message_text_with_mixed_content() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text("before ".to_string()),
                ContentBlock::ToolUse(ToolUseBlock {
                    id: "1".to_string(),
                    name: "tool".to_string(),
                    input: serde_json::json!({}),
                }),
                ContentBlock::Text("after".to_string()),
            ],
        };
        // text() should skip non-text blocks and concatenate
        assert_eq!(msg.text(), "before after");
    }

    #[test]
    fn test_message_text_empty_content() {
        let msg = Message {
            role: Role::User,
            content: vec![],
        };
        assert_eq!(msg.text(), "");
    }

    #[test]
    fn test_message_text_no_text_blocks() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(ToolUseBlock {
                id: "1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"query": "rust"}),
            })],
        };
        assert_eq!(msg.text(), "");
    }

    #[test]
    fn test_message_tool_uses_extraction() {
        let tool_use = ToolUseBlock {
            id: "id1".to_string(),
            name: "search".to_string(),
            input: serde_json::json!({"query": "rust"}),
        };

        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text("I'll search for that".to_string()),
                ContentBlock::ToolUse(tool_use.clone()),
            ],
        };

        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].name, "search");
        assert_eq!(uses[0].id, "id1");
    }

    #[test]
    fn test_message_tool_uses_multiple() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse(ToolUseBlock {
                    id: "1".to_string(),
                    name: "read".to_string(),
                    input: serde_json::json!({}),
                }),
                ContentBlock::Text("reading...".to_string()),
                ContentBlock::ToolUse(ToolUseBlock {
                    id: "2".to_string(),
                    name: "write".to_string(),
                    input: serde_json::json!({}),
                }),
            ],
        };

        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].name, "read");
        assert_eq!(uses[1].name, "write");
    }

    #[test]
    fn test_message_tool_uses_empty() {
        let msg = Message::user("no tools here");
        assert!(msg.tool_uses().is_empty());
    }

    #[test]
    fn test_message_tool_results_creation() {
        let results = vec![
            ToolResultBlock {
                tool_use_id: "1".to_string(),
                content: ToolResult::Text("ok".to_string()),
                status: ToolResultStatus::Success,
            },
            ToolResultBlock {
                tool_use_id: "2".to_string(),
                content: ToolResult::Text("failed".to_string()),
                status: ToolResultStatus::Error,
            },
        ];

        let msg = Message::tool_results(results);
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 2);

        // Verify content blocks are ToolResult variants
        assert!(matches!(&msg.content[0], ContentBlock::ToolResult(r) if r.tool_use_id == "1"));
        assert!(matches!(&msg.content[1], ContentBlock::ToolResult(r) if r.tool_use_id == "2"));
    }

    #[test]
    fn test_message_tool_results_empty() {
        let msg = Message::tool_results(vec![]);
        assert_eq!(msg.role, Role::User);
        assert!(msg.content.is_empty());
    }

    #[test]
    fn test_message_assistant_with_tool_use() {
        let tool_uses = vec![
            ToolUseBlock {
                id: "tool_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"query": "rust"}),
            },
            ToolUseBlock {
                id: "tool_2".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"path": "/tmp/file"}),
            },
        ];

        let msg = Message::assistant_with_tool_use("Let me help you", tool_uses);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 3);

        // First block is text
        assert!(matches!(&msg.content[0], ContentBlock::Text(t) if t == "Let me help you"));
        // Remaining blocks are tool uses
        assert!(matches!(&msg.content[1], ContentBlock::ToolUse(t) if t.name == "search"));
        assert!(matches!(&msg.content[2], ContentBlock::ToolUse(t) if t.name == "read"));

        // Verify text() extraction
        assert_eq!(msg.text(), "Let me help you");

        // Verify tool_uses() extraction
        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].name, "search");
        assert_eq!(uses[1].name, "read");
    }

    #[test]
    fn test_message_assistant_with_content() {
        let content = vec![
            ContentBlock::Text("Thinking about this...".to_string()),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "tool_x".to_string(),
                name: "compute".to_string(),
                input: serde_json::json!({}),
            }),
        ];

        let msg = Message::assistant_with_content(content);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert!(matches!(&msg.content[0], ContentBlock::Text(_)));
        assert!(matches!(&msg.content[1], ContentBlock::ToolUse(_)));
    }

    // ===== Edge Cases for assistant_with_tool_use =====

    #[test]
    fn test_message_assistant_with_tool_use_empty_tools() {
        let msg = Message::assistant_with_tool_use("Just text", vec![]);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(&msg.content[0], ContentBlock::Text(t) if t == "Just text"));
    }

    #[test]
    fn test_message_assistant_with_tool_use_empty_text() {
        let tool_uses = vec![ToolUseBlock {
            id: "tool_1".to_string(),
            name: "search".to_string(),
            input: serde_json::json!({"query": "test"}),
        }];

        let msg = Message::assistant_with_tool_use("", tool_uses);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert!(matches!(&msg.content[0], ContentBlock::Text(t) if t.is_empty()));
        assert!(matches!(&msg.content[1], ContentBlock::ToolUse(_)));
    }

    #[test]
    fn test_message_assistant_with_tool_use_single_tool() {
        let tool_uses = vec![ToolUseBlock {
            id: "single".to_string(),
            name: "compute".to_string(),
            input: serde_json::json!({"value": 42}),
        }];

        let msg = Message::assistant_with_tool_use("Let me compute", tool_uses);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert_eq!(msg.text(), "Let me compute");

        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].name, "compute");
        assert_eq!(uses[0].id, "single");
    }

    #[test]
    fn test_message_assistant_with_tool_use_preserves_order() {
        let tool_uses = vec![
            ToolUseBlock {
                id: "first".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({}),
            },
            ToolUseBlock {
                id: "second".to_string(),
                name: "write".to_string(),
                input: serde_json::json!({}),
            },
            ToolUseBlock {
                id: "third".to_string(),
                name: "delete".to_string(),
                input: serde_json::json!({}),
            },
        ];

        let msg = Message::assistant_with_tool_use("Processing files", tool_uses);

        // First block should be text
        assert!(matches!(&msg.content[0], ContentBlock::Text(_)));

        // Remaining blocks should be tool uses in order
        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].id, "first");
        assert_eq!(uses[1].id, "second");
        assert_eq!(uses[2].id, "third");
    }

    #[test]
    fn test_message_assistant_with_tool_use_string_conversion() {
        let tool_uses = vec![ToolUseBlock {
            id: "1".to_string(),
            name: "tool".to_string(),
            input: serde_json::json!({}),
        }];

        // Test with String (owned)
        let msg1 = Message::assistant_with_tool_use(String::from("owned"), tool_uses.clone());
        assert_eq!(msg1.text(), "owned");

        // Test with &str (borrowed)
        let msg2 = Message::assistant_with_tool_use("borrowed", tool_uses);
        assert_eq!(msg2.text(), "borrowed");
    }

    // ===== Edge Cases for assistant_with_content =====

    #[test]
    fn test_message_assistant_with_content_empty() {
        let msg = Message::assistant_with_content(vec![]);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 0);
        assert_eq!(msg.text(), "");
        assert_eq!(msg.tool_uses().len(), 0);
    }

    #[test]
    fn test_message_assistant_with_content_only_text() {
        let content = vec![ContentBlock::Text("Hello world".to_string())];

        let msg = Message::assistant_with_content(content);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 1);
        assert_eq!(msg.text(), "Hello world");
        assert!(msg.tool_uses().is_empty());
    }

    #[test]
    fn test_message_assistant_with_content_only_tools() {
        let content = vec![
            ContentBlock::ToolUse(ToolUseBlock {
                id: "1".to_string(),
                name: "tool1".to_string(),
                input: serde_json::json!({}),
            }),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "2".to_string(),
                name: "tool2".to_string(),
                input: serde_json::json!({}),
            }),
        ];

        let msg = Message::assistant_with_content(content);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert_eq!(msg.text(), "");
        assert_eq!(msg.tool_uses().len(), 2);
    }

    #[test]
    fn test_message_assistant_with_content_mixed_types() {
        let content = vec![
            ContentBlock::Text("Part 1".to_string()),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "tool_a".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({}),
            }),
            ContentBlock::Text("Part 2".to_string()),
            ContentBlock::Thinking {
                thinking: "Let me think...".to_string(),
                signature: "sig123".to_string(),
            },
            ContentBlock::Text("Part 3".to_string()),
        ];

        let msg = Message::assistant_with_content(content);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 5);

        // text() should concatenate all text blocks
        assert_eq!(msg.text(), "Part 1Part 2Part 3");

        // tool_uses() should extract only tool use blocks
        assert_eq!(msg.tool_uses().len(), 1);
        assert_eq!(msg.tool_uses()[0].id, "tool_a");
    }

    #[test]
    fn test_message_assistant_with_content_with_thinking() {
        let content = vec![
            ContentBlock::Thinking {
                thinking: "Analyzing the problem...".to_string(),
                signature: "abc123".to_string(),
            },
            ContentBlock::Text("Based on my analysis".to_string()),
        ];

        let msg = Message::assistant_with_content(content);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert!(matches!(&msg.content[0], ContentBlock::Thinking { .. }));
        assert_eq!(msg.text(), "Based on my analysis");
    }

    #[test]
    fn test_message_assistant_with_content_complex_scenario() {
        // Real-world scenario: thinking, text, multiple tools, more text
        let content = vec![
            ContentBlock::Thinking {
                thinking: "I need to search and then read files".to_string(),
                signature: "sig1".to_string(),
            },
            ContentBlock::Text("Let me search for relevant files".to_string()),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "search_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"pattern": "*.rs"}),
            }),
            ContentBlock::Text("Now I'll read them".to_string()),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "read_1".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "file.rs"}),
            }),
            ContentBlock::ToolUse(ToolUseBlock {
                id: "read_2".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "main.rs"}),
            }),
        ];

        let msg = Message::assistant_with_content(content);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 6);

        // Verify text extraction works correctly
        assert_eq!(
            msg.text(),
            "Let me search for relevant filesNow I'll read them"
        );

        // Verify tool extraction works correctly
        let uses = msg.tool_uses();
        assert_eq!(uses.len(), 3);
        assert_eq!(uses[0].name, "search");
        assert_eq!(uses[1].name, "read_file");
        assert_eq!(uses[2].name, "read_file");
    }

    // ===== ThinkingConfig Tests =====

    #[test]
    fn test_thinking_config_enabled() {
        let config = ThinkingConfig::enabled(2048);
        match config {
            ThinkingConfig::Enabled { budget_tokens } => {
                assert_eq!(budget_tokens, 2048);
            }
            ThinkingConfig::Disabled => panic!("Expected Enabled variant"),
        }
    }

    #[test]
    fn test_thinking_config_enabled_various_budgets() {
        for budget in [1024, 4096, 8192, 16384] {
            let config = ThinkingConfig::enabled(budget);
            assert!(
                matches!(config, ThinkingConfig::Enabled { budget_tokens } if budget_tokens == budget)
            );
        }
    }

    #[test]
    fn test_thinking_config_disabled() {
        let config = ThinkingConfig::disabled();
        assert!(matches!(config, ThinkingConfig::Disabled));
    }
}
