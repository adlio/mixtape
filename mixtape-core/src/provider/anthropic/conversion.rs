//! Type conversions between Mixtape and Anthropic SDK types

use super::ProviderError;
use crate::tool::{DocumentFormat, ImageFormat, ToolResult};
use crate::types::{
    ContentBlock, Message, Role, ServerToolUseBlock, StopReason, ToolDefinition, ToolReference,
    ToolResultStatus, ToolSearchResultBlock, ToolUseBlock,
};
use base64::Engine;
use mixtape_anthropic_sdk::{
    ContentBlock as AnthropicContentBlock, ContentBlockParam, DocumentSource, ImageSource,
    Message as AnthropicMessage, MessageContent, MessageParam, Role as AnthropicRole,
    StopReason as AnthropicStopReason, Tool as AnthropicTool, ToolInputSchema,
    ToolResultContent as AnthropicToolResultContent, ToolResultContentBlock,
    ToolSearchResultContent,
};

// ===== Type Conversion: Mixtape -> Anthropic =====

pub fn to_anthropic_message(msg: &Message) -> Result<MessageParam, ProviderError> {
    let role = match msg.role {
        Role::User => AnthropicRole::User,
        Role::Assistant => AnthropicRole::Assistant,
    };

    let content_blocks: Vec<ContentBlockParam> = msg
        .content
        .iter()
        .filter_map(|block| to_anthropic_content_block(block).transpose())
        .collect::<Result<Vec<_>, _>>()?;

    Ok(MessageParam {
        role,
        content: MessageContent::Blocks(content_blocks),
    })
}

fn to_anthropic_content_block(
    block: &ContentBlock,
) -> Result<Option<ContentBlockParam>, ProviderError> {
    match block {
        ContentBlock::Text(text) => Ok(Some(ContentBlockParam::Text {
            text: text.clone(),
            cache_control: None,
        })),
        ContentBlock::ToolUse(tool_use) => Ok(Some(ContentBlockParam::ToolUse {
            id: tool_use.id.clone(),
            name: tool_use.name.clone(),
            input: tool_use.input.clone(),
            cache_control: None,
        })),
        ContentBlock::ToolResult(result) => {
            // Convert content to proper Anthropic types
            let content_block = match &result.content {
                ToolResult::Text(text) => ToolResultContentBlock::Text { text: text.clone() },
                ToolResult::Json(json) => ToolResultContentBlock::Text {
                    text: json.to_string(),
                },
                ToolResult::Image { format, data } => {
                    let media_type = image_format_to_media_type(*format);
                    let base64_data = base64::engine::general_purpose::STANDARD.encode(data);
                    ToolResultContentBlock::Image {
                        source: ImageSource::Base64 {
                            media_type,
                            data: base64_data,
                        },
                    }
                }
                ToolResult::Document { format, data, name } => {
                    let media_type = document_format_to_media_type(*format);
                    let base64_data = base64::engine::general_purpose::STANDARD.encode(data);
                    ToolResultContentBlock::Document {
                        source: DocumentSource::Base64 {
                            media_type,
                            data: base64_data,
                        },
                        title: name.clone(),
                    }
                }
            };
            let is_error = matches!(result.status, ToolResultStatus::Error);
            Ok(Some(ContentBlockParam::ToolResult {
                tool_use_id: result.tool_use_id.clone(),
                content: Some(AnthropicToolResultContent::Blocks(vec![content_block])),
                is_error: Some(is_error),
                cache_control: None,
            }))
        }
        ContentBlock::Thinking {
            thinking,
            signature,
        } => {
            // Pass thinking blocks back to the API for multi-turn conversations
            Ok(Some(ContentBlockParam::Thinking {
                thinking: thinking.clone(),
                signature: signature.clone(),
            }))
        }
        // Server-side blocks are informational only - don't send back to API
        ContentBlock::ServerToolUse(_) => Ok(None),
        ContentBlock::ToolSearchResult(_) => Ok(None),
    }
}

pub fn to_anthropic_tool(tool: &ToolDefinition) -> Result<AnthropicTool, ProviderError> {
    // Convert serde_json::Value to ToolInputSchema
    let input_schema = convert_json_to_tool_schema(&tool.input_schema)?;

    Ok(AnthropicTool {
        name: tool.name.clone(),
        description: Some(tool.description.clone()),
        input_schema,
        cache_control: None,
        tool_type: None,
        defer_loading: if tool.defer_loading { Some(true) } else { None },
    })
}

fn convert_json_to_tool_schema(
    value: &serde_json::Value,
) -> Result<ToolInputSchema, ProviderError> {
    // Extract properties, required, and any additional fields from the JSON schema
    let obj = value.as_object().ok_or_else(|| {
        ProviderError::Configuration("Tool input_schema must be an object".to_string())
    })?;

    let schema_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("object")
        .to_string();

    let properties = obj.get("properties").and_then(|v| v.as_object()).cloned();

    let required = obj.get("required").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });

    // Collect any additional schema properties (like additionalProperties, etc.)
    let mut additional = serde_json::Map::new();
    for (key, val) in obj {
        if key != "type" && key != "properties" && key != "required" {
            additional.insert(key.clone(), val.clone());
        }
    }

    Ok(ToolInputSchema {
        schema_type,
        properties,
        required,
        additional,
    })
}

/// Convert ImageFormat to MIME type string
fn image_format_to_media_type(format: ImageFormat) -> String {
    match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Webp => "image/webp",
    }
    .to_string()
}

/// Convert DocumentFormat to MIME type string
fn document_format_to_media_type(format: DocumentFormat) -> String {
    match format {
        DocumentFormat::Pdf => "application/pdf",
        DocumentFormat::Csv => "text/csv",
        DocumentFormat::Doc => "application/msword",
        DocumentFormat::Docx => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        DocumentFormat::Html => "text/html",
        DocumentFormat::Md => "text/markdown",
        DocumentFormat::Txt => "text/plain",
        DocumentFormat::Xls => "application/vnd.ms-excel",
        DocumentFormat::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    }
    .to_string()
}

// ===== Type Conversion: Anthropic -> Mixtape =====

pub fn from_anthropic_message(msg: &AnthropicMessage) -> Message {
    let role = match msg.role {
        AnthropicRole::User => Role::User,
        AnthropicRole::Assistant => Role::Assistant,
    };

    let content: Vec<ContentBlock> = msg
        .content
        .iter()
        .filter_map(from_anthropic_content_block)
        .collect();

    Message { role, content }
}

fn from_anthropic_content_block(block: &AnthropicContentBlock) -> Option<ContentBlock> {
    match block {
        AnthropicContentBlock::Text { text } => Some(ContentBlock::Text(text.clone())),
        AnthropicContentBlock::ToolUse { id, name, input } => {
            Some(ContentBlock::ToolUse(ToolUseBlock {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }))
        }
        AnthropicContentBlock::Thinking {
            thinking,
            signature,
        } => Some(ContentBlock::Thinking {
            thinking: thinking.clone(),
            signature: signature.clone(),
        }),
        // Redacted thinking - we preserve it as thinking with empty content
        AnthropicContentBlock::RedactedThinking { data } => Some(ContentBlock::Thinking {
            thinking: String::new(),
            signature: data.clone(),
        }),
        // Server tool use blocks - expose for transparency
        AnthropicContentBlock::ServerToolUse { id, name, input } => {
            Some(ContentBlock::ServerToolUse(ServerToolUseBlock {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }))
        }
        // Web search results - skip for now (could be added as a new block type)
        AnthropicContentBlock::WebSearchToolResult { .. } => None,
        // Tool search results - convert to core type
        AnthropicContentBlock::ToolSearchToolResult {
            tool_use_id,
            content,
        } => {
            let tool_references = match content {
                ToolSearchResultContent::ToolReferences { tool_references } => tool_references
                    .iter()
                    .map(|r| ToolReference::new(&r.name))
                    .collect(),
                ToolSearchResultContent::Error { .. } => Vec::new(),
            };
            Some(ContentBlock::ToolSearchResult(ToolSearchResultBlock {
                tool_use_id: tool_use_id.clone(),
                tool_references,
            }))
        }
    }
}

pub fn from_anthropic_stop_reason(reason: &AnthropicStopReason) -> StopReason {
    match reason {
        AnthropicStopReason::EndTurn => StopReason::EndTurn,
        AnthropicStopReason::ToolUse => StopReason::ToolUse,
        AnthropicStopReason::MaxTokens => StopReason::MaxTokens,
        AnthropicStopReason::StopSequence => StopReason::StopSequence,
        AnthropicStopReason::PauseTurn => StopReason::PauseTurn,
        AnthropicStopReason::Refusal => StopReason::ContentFiltered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolResultBlock;

    #[test]
    fn test_message_conversion_user() {
        let msg = Message::user("Hello, world!");
        let anthropic_msg = to_anthropic_message(&msg).unwrap();

        assert_eq!(anthropic_msg.role, AnthropicRole::User);
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlockParam::Text { text, .. } => assert_eq!(text, "Hello, world!"),
                    _ => panic!("Expected text block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_message_conversion_assistant() {
        let msg = Message::assistant("I can help with that.");
        let anthropic_msg = to_anthropic_message(&msg).unwrap();

        assert_eq!(anthropic_msg.role, AnthropicRole::Assistant);
    }

    #[test]
    fn test_tool_use_conversion() {
        let tool_use = ToolUseBlock {
            id: "tool_123".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let block = ContentBlock::ToolUse(tool_use);
        let msg = Message {
            role: Role::Assistant,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlockParam::ToolUse {
                        id, name, input, ..
                    } => {
                        assert_eq!(id, "tool_123");
                        assert_eq!(name, "read_file");
                        assert_eq!(input["path"], "/tmp/test.txt");
                    }
                    _ => panic!("Expected tool use block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_tool_result_text_conversion() {
        let result = ToolResultBlock {
            tool_use_id: "tool_123".to_string(),
            content: ToolResult::Text("File contents here".to_string()),
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);
        let msg = Message {
            role: Role::User,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlockParam::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } => {
                        assert_eq!(tool_use_id, "tool_123");
                        match content {
                            Some(AnthropicToolResultContent::Blocks(result_blocks)) => {
                                assert_eq!(result_blocks.len(), 1);
                                match &result_blocks[0] {
                                    ToolResultContentBlock::Text { text } => {
                                        assert_eq!(text, "File contents here");
                                    }
                                    _ => panic!("Expected text block"),
                                }
                            }
                            _ => panic!("Expected blocks content in tool result"),
                        }
                        assert_eq!(*is_error, Some(false));
                    }
                    _ => panic!("Expected tool result block"),
                }
            }
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_tool_result_error_status() {
        let result = ToolResultBlock {
            tool_use_id: "tool_456".to_string(),
            content: ToolResult::Text("Error: file not found".to_string()),
            status: ToolResultStatus::Error,
        };
        let block = ContentBlock::ToolResult(result);
        let msg = Message {
            role: Role::User,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlockParam::ToolResult { is_error, .. } => {
                    assert_eq!(*is_error, Some(true));
                }
                _ => panic!("Expected tool result block"),
            },
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_tool_result_json_conversion() {
        let result = ToolResultBlock {
            tool_use_id: "tool_789".to_string(),
            content: ToolResult::Json(serde_json::json!({"count": 42})),
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);
        let msg = Message {
            role: Role::User,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlockParam::ToolResult { content, .. } => match content {
                    Some(AnthropicToolResultContent::Blocks(result_blocks)) => {
                        assert_eq!(result_blocks.len(), 1);
                        match &result_blocks[0] {
                            ToolResultContentBlock::Text { text } => {
                                assert!(text.contains("42"));
                            }
                            _ => panic!("Expected text block"),
                        }
                    }
                    _ => panic!("Expected blocks content in tool result"),
                },
                _ => panic!("Expected tool result block"),
            },
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_tool_definition_conversion() {
        let tool_def = ToolDefinition {
            name: "search".to_string(),
            description: "Search for files".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
            defer_loading: false,
        };

        let anthropic_tool = to_anthropic_tool(&tool_def).unwrap();

        assert_eq!(anthropic_tool.name, "search");
        assert_eq!(
            anthropic_tool.description,
            Some("Search for files".to_string())
        );
        assert_eq!(anthropic_tool.input_schema.schema_type, "object");
        assert!(anthropic_tool
            .input_schema
            .properties
            .as_ref()
            .unwrap()
            .contains_key("query"));
        assert_eq!(
            anthropic_tool.input_schema.required,
            Some(vec!["query".to_string()])
        );
        assert!(anthropic_tool.defer_loading.is_none());
    }

    #[test]
    fn test_stop_reason_conversion() {
        assert_eq!(
            from_anthropic_stop_reason(&AnthropicStopReason::EndTurn),
            StopReason::EndTurn
        );
        assert_eq!(
            from_anthropic_stop_reason(&AnthropicStopReason::ToolUse),
            StopReason::ToolUse
        );
        assert_eq!(
            from_anthropic_stop_reason(&AnthropicStopReason::MaxTokens),
            StopReason::MaxTokens
        );
        assert_eq!(
            from_anthropic_stop_reason(&AnthropicStopReason::StopSequence),
            StopReason::StopSequence
        );
    }

    #[test]
    fn test_stop_reason_pause_turn() {
        assert_eq!(
            from_anthropic_stop_reason(&AnthropicStopReason::PauseTurn),
            StopReason::PauseTurn
        );
    }

    #[test]
    fn test_stop_reason_refusal() {
        assert_eq!(
            from_anthropic_stop_reason(&AnthropicStopReason::Refusal),
            StopReason::ContentFiltered
        );
    }

    // ===== Image/Document Tool Result Tests =====

    #[test]
    fn test_tool_result_image_conversion() {
        let image_data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        let result = ToolResultBlock {
            tool_use_id: "tool_img".to_string(),
            content: ToolResult::Image {
                format: ImageFormat::Png,
                data: image_data,
            },
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);
        let msg = Message {
            role: Role::User,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlockParam::ToolResult { content, .. } => match content {
                    Some(AnthropicToolResultContent::Blocks(result_blocks)) => {
                        assert_eq!(result_blocks.len(), 1);
                        match &result_blocks[0] {
                            ToolResultContentBlock::Image { source } => match source {
                                ImageSource::Base64 { media_type, data } => {
                                    assert_eq!(media_type, "image/png");
                                    // Verify base64 encoding
                                    assert!(!data.is_empty());
                                }
                                _ => panic!("Expected Base64 source"),
                            },
                            _ => panic!("Expected Image block"),
                        }
                    }
                    _ => panic!("Expected blocks content"),
                },
                _ => panic!("Expected tool result block"),
            },
            _ => panic!("Expected blocks content"),
        }
    }

    #[test]
    fn test_tool_result_document_conversion() {
        let doc_data = vec![0x25, 0x50, 0x44, 0x46]; // PDF magic bytes
        let result = ToolResultBlock {
            tool_use_id: "tool_doc".to_string(),
            content: ToolResult::Document {
                format: DocumentFormat::Pdf,
                data: doc_data,
                name: Some("report.pdf".to_string()),
            },
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);
        let msg = Message {
            role: Role::User,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlockParam::ToolResult { content, .. } => match content {
                    Some(AnthropicToolResultContent::Blocks(result_blocks)) => {
                        match &result_blocks[0] {
                            ToolResultContentBlock::Document { source, title } => {
                                match source {
                                    DocumentSource::Base64 { media_type, data } => {
                                        assert_eq!(media_type, "application/pdf");
                                        assert!(!data.is_empty());
                                    }
                                    _ => panic!("Expected Base64 source"),
                                }
                                assert_eq!(*title, Some("report.pdf".to_string()));
                            }
                            _ => panic!("Expected Document block"),
                        }
                    }
                    _ => panic!("Expected blocks content"),
                },
                _ => panic!("Expected tool result block"),
            },
            _ => panic!("Expected blocks content"),
        }
    }

    // ===== Image Format Media Type Tests =====

    #[test]
    fn test_image_format_to_media_type_all() {
        assert_eq!(image_format_to_media_type(ImageFormat::Png), "image/png");
        assert_eq!(image_format_to_media_type(ImageFormat::Jpeg), "image/jpeg");
        assert_eq!(image_format_to_media_type(ImageFormat::Gif), "image/gif");
        assert_eq!(image_format_to_media_type(ImageFormat::Webp), "image/webp");
    }

    // ===== Document Format Media Type Tests =====

    #[test]
    fn test_document_format_to_media_type_all() {
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Pdf),
            "application/pdf"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Csv),
            "text/csv"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Doc),
            "application/msword"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Docx),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Html),
            "text/html"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Md),
            "text/markdown"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Txt),
            "text/plain"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Xls),
            "application/vnd.ms-excel"
        );
        assert_eq!(
            document_format_to_media_type(DocumentFormat::Xlsx),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
    }

    // ===== Thinking Block Conversion Tests =====

    #[test]
    fn test_thinking_block_to_anthropic() {
        let block = ContentBlock::Thinking {
            thinking: "Let me analyze this...".to_string(),
            signature: "sig_xyz789".to_string(),
        };
        let msg = Message {
            role: Role::Assistant,
            content: vec![block],
        };

        let anthropic_msg = to_anthropic_message(&msg).unwrap();
        match &anthropic_msg.content {
            MessageContent::Blocks(blocks) => match &blocks[0] {
                ContentBlockParam::Thinking {
                    thinking,
                    signature,
                } => {
                    assert_eq!(thinking, "Let me analyze this...");
                    assert_eq!(signature, "sig_xyz789");
                }
                _ => panic!("Expected Thinking block"),
            },
            _ => panic!("Expected blocks content"),
        }
    }

    // ===== Response Parsing (from_anthropic) Tests =====

    #[test]
    fn test_from_anthropic_message_text() {
        use mixtape_anthropic_sdk::Message as AnthropicMessage;
        use mixtape_anthropic_sdk::{ContentBlock as AnthropicContentBlock, Usage};

        let anthropic_msg = AnthropicMessage {
            id: "msg_123".to_string(),
            message_type: "message".to_string(),
            role: AnthropicRole::Assistant,
            content: vec![AnthropicContentBlock::Text {
                text: "Hello there!".to_string(),
            }],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: Some(AnthropicStopReason::EndTurn),
            stop_sequence: None,
            usage: Usage::default(),
        };

        let msg = from_anthropic_message(&anthropic_msg);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text(text) => assert_eq!(text, "Hello there!"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_from_anthropic_message_tool_use() {
        use mixtape_anthropic_sdk::Message as AnthropicMessage;
        use mixtape_anthropic_sdk::{ContentBlock as AnthropicContentBlock, Usage};

        let anthropic_msg = AnthropicMessage {
            id: "msg_456".to_string(),
            message_type: "message".to_string(),
            role: AnthropicRole::Assistant,
            content: vec![AnthropicContentBlock::ToolUse {
                id: "tool_abc".to_string(),
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "/tmp/test.txt"}),
            }],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: Some(AnthropicStopReason::ToolUse),
            stop_sequence: None,
            usage: Usage::default(),
        };

        let msg = from_anthropic_message(&anthropic_msg);

        assert_eq!(msg.role, Role::Assistant);
        match &msg.content[0] {
            ContentBlock::ToolUse(tu) => {
                assert_eq!(tu.id, "tool_abc");
                assert_eq!(tu.name, "read_file");
                assert_eq!(tu.input["path"], "/tmp/test.txt");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_from_anthropic_message_thinking() {
        use mixtape_anthropic_sdk::Message as AnthropicMessage;
        use mixtape_anthropic_sdk::{ContentBlock as AnthropicContentBlock, Usage};

        let anthropic_msg = AnthropicMessage {
            id: "msg_789".to_string(),
            message_type: "message".to_string(),
            role: AnthropicRole::Assistant,
            content: vec![
                AnthropicContentBlock::Thinking {
                    thinking: "Let me think about this...".to_string(),
                    signature: "sig_think".to_string(),
                },
                AnthropicContentBlock::Text {
                    text: "Here's my answer.".to_string(),
                },
            ],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: Some(AnthropicStopReason::EndTurn),
            stop_sequence: None,
            usage: Usage::default(),
        };

        let msg = from_anthropic_message(&anthropic_msg);

        assert_eq!(msg.content.len(), 2);
        match &msg.content[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "Let me think about this...");
                assert_eq!(signature, "sig_think");
            }
            _ => panic!("Expected Thinking block"),
        }
        match &msg.content[1] {
            ContentBlock::Text(text) => assert_eq!(text, "Here's my answer."),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_from_anthropic_message_redacted_thinking() {
        use mixtape_anthropic_sdk::Message as AnthropicMessage;
        use mixtape_anthropic_sdk::{ContentBlock as AnthropicContentBlock, Usage};

        let anthropic_msg = AnthropicMessage {
            id: "msg_redacted".to_string(),
            message_type: "message".to_string(),
            role: AnthropicRole::Assistant,
            content: vec![AnthropicContentBlock::RedactedThinking {
                data: "redacted_data_here".to_string(),
            }],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: Some(AnthropicStopReason::EndTurn),
            stop_sequence: None,
            usage: Usage::default(),
        };

        let msg = from_anthropic_message(&anthropic_msg);

        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert!(thinking.is_empty());
                assert_eq!(signature, "redacted_data_here");
            }
            _ => panic!("Expected Thinking block for redacted thinking"),
        }
    }

    #[test]
    fn test_from_anthropic_message_user_role() {
        use mixtape_anthropic_sdk::Message as AnthropicMessage;
        use mixtape_anthropic_sdk::{ContentBlock as AnthropicContentBlock, Usage};

        let anthropic_msg = AnthropicMessage {
            id: "msg_user".to_string(),
            message_type: "message".to_string(),
            role: AnthropicRole::User,
            content: vec![AnthropicContentBlock::Text {
                text: "User message".to_string(),
            }],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: None,
            stop_sequence: None,
            usage: Usage::default(),
        };

        let msg = from_anthropic_message(&anthropic_msg);
        assert_eq!(msg.role, Role::User);
    }

    // ===== Tool Schema Conversion Edge Cases =====

    #[test]
    fn test_tool_schema_with_additional_properties() {
        let tool_def = ToolDefinition {
            name: "flexible_tool".to_string(),
            description: "A tool with additionalProperties".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
            defer_loading: false,
        };

        let anthropic_tool = to_anthropic_tool(&tool_def).unwrap();
        assert_eq!(anthropic_tool.name, "flexible_tool");
        assert_eq!(anthropic_tool.input_schema.schema_type, "object");
        // The additionalProperties should be in the additional map
        assert!(anthropic_tool
            .input_schema
            .additional
            .contains_key("additionalProperties"));
    }

    #[test]
    fn test_tool_schema_minimal() {
        let tool_def = ToolDefinition {
            name: "minimal".to_string(),
            description: "A minimal tool".to_string(),
            input_schema: serde_json::json!({
                "type": "object"
            }),
            defer_loading: false,
        };

        let anthropic_tool = to_anthropic_tool(&tool_def).unwrap();
        assert_eq!(anthropic_tool.name, "minimal");
        assert!(anthropic_tool.input_schema.properties.is_none());
        assert!(anthropic_tool.input_schema.required.is_none());
    }

    #[test]
    fn test_tool_definition_with_defer_loading() {
        let tool_def = ToolDefinition {
            name: "deferred_tool".to_string(),
            description: "A deferred tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            defer_loading: true,
        };

        let anthropic_tool = to_anthropic_tool(&tool_def).unwrap();
        assert_eq!(anthropic_tool.defer_loading, Some(true));
    }
}
