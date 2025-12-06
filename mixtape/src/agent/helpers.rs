//! Helper functions for the Agent module

use crate::types::{ContentBlock, Message};

/// Extract the first text content from a message
///
/// Returns the first text block found in the message content,
/// or None if no text content exists.
pub fn extract_text_response(message: &Message) -> Option<String> {
    message.content.iter().find_map(|c| {
        if let ContentBlock::Text(t) = c {
            Some(t.clone())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Role, ToolUseBlock};

    #[test]
    fn test_extract_text_response_with_text() {
        let message = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text("Hello, world!".to_string())],
        };
        assert_eq!(
            extract_text_response(&message),
            Some("Hello, world!".to_string())
        );
    }

    #[test]
    fn test_extract_text_response_empty() {
        let message = Message {
            role: Role::Assistant,
            content: vec![],
        };
        assert_eq!(extract_text_response(&message), None);
    }

    #[test]
    fn test_extract_text_response_multiple_text_blocks() {
        // Should return only the first text block
        let message = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text("First".to_string()),
                ContentBlock::Text("Second".to_string()),
            ],
        };
        assert_eq!(extract_text_response(&message), Some("First".to_string()));
    }

    #[test]
    fn test_extract_text_response_tool_use_before_text() {
        // Text comes after tool use - should still find it
        let message = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse(ToolUseBlock {
                    id: "1".to_string(),
                    name: "tool".to_string(),
                    input: serde_json::json!({}),
                }),
                ContentBlock::Text("after tool".to_string()),
            ],
        };
        assert_eq!(
            extract_text_response(&message),
            Some("after tool".to_string())
        );
    }

    #[test]
    fn test_extract_text_response_only_tool_use() {
        // No text blocks at all
        let message = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse(ToolUseBlock {
                id: "1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"query": "rust"}),
            })],
        };
        assert_eq!(extract_text_response(&message), None);
    }

    #[test]
    fn test_extract_text_response_thinking_then_text() {
        // Thinking block before text - should skip thinking and find text
        let message = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "reasoning about the problem".to_string(),
                    signature: "sig123".to_string(),
                },
                ContentBlock::Text("The answer is 42".to_string()),
            ],
        };
        assert_eq!(
            extract_text_response(&message),
            Some("The answer is 42".to_string())
        );
    }

    #[test]
    fn test_extract_text_response_mixed_content_text_first() {
        // Text comes first among mixed content
        let message = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text("I'll help you".to_string()),
                ContentBlock::ToolUse(ToolUseBlock {
                    id: "1".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "/tmp/test"}),
                }),
                ContentBlock::Text("Here's what I found".to_string()),
            ],
        };
        // Should return the first text block
        assert_eq!(
            extract_text_response(&message),
            Some("I'll help you".to_string())
        );
    }
}
