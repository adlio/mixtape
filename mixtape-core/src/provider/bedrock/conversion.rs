//! Type conversions between Mixtape and AWS Bedrock types

use super::ProviderError;
use crate::tool::ToolResult;
use crate::types::{
    ContentBlock, Message, Role, StopReason, ToolDefinition, ToolResultStatus, ToolUseBlock,
};
use aws_sdk_bedrockruntime::{
    primitives::Blob,
    types::{
        ContentBlock as BedrockContentBlock, ConversationRole, DocumentBlock,
        DocumentFormat as BedrockDocFormat, DocumentSource, ImageBlock,
        ImageFormat as BedrockImageFormat, ImageSource, Message as BedrockMessage,
        Tool as BedrockTool, ToolInputSchema, ToolResultBlock as BedrockToolResultBlock,
        ToolResultContentBlock, ToolResultStatus as BedrockToolResultStatus, ToolSpecification,
        ToolUseBlock as BedrockToolUseBlock,
    },
};
use aws_smithy_types::Document;

// ===== Type Conversion: Mixtape -> Bedrock =====

pub fn to_bedrock_message(msg: &Message) -> Result<BedrockMessage, ProviderError> {
    let role = match msg.role {
        Role::User => ConversationRole::User,
        Role::Assistant => ConversationRole::Assistant,
    };

    let content: Vec<BedrockContentBlock> = msg
        .content
        .iter()
        .map(to_bedrock_content_block)
        .collect::<Result<Vec<_>, _>>()?;

    BedrockMessage::builder()
        .role(role)
        .set_content(Some(content))
        .build()
        .map_err(|e| ProviderError::Configuration(e.to_string()))
}

fn to_bedrock_content_block(block: &ContentBlock) -> Result<BedrockContentBlock, ProviderError> {
    match block {
        ContentBlock::Text(text) => Ok(BedrockContentBlock::Text(text.clone())),
        ContentBlock::ToolUse(tool_use) => {
            let input_doc = json_to_document(&tool_use.input);
            let block = BedrockToolUseBlock::builder()
                .tool_use_id(&tool_use.id)
                .name(&tool_use.name)
                .input(input_doc)
                .build()
                .map_err(|e| ProviderError::Configuration(e.to_string()))?;
            Ok(BedrockContentBlock::ToolUse(block))
        }
        ContentBlock::ToolResult(result) => {
            let content = match &result.content {
                ToolResult::Text(text) => ToolResultContentBlock::Text(text.clone()),
                ToolResult::Json(json) => ToolResultContentBlock::Json(json_to_document(json)),
                ToolResult::Image { format, data } => {
                    let image_block = ImageBlock::builder()
                        .format(to_bedrock_image_format(*format))
                        .source(ImageSource::Bytes(Blob::new(data.clone())))
                        .build()
                        .map_err(|e| ProviderError::Configuration(e.to_string()))?;
                    ToolResultContentBlock::Image(image_block)
                }
                ToolResult::Document { format, data, name } => {
                    // Bedrock requires a document name; use provided name or default
                    let doc_name = name.clone().unwrap_or_else(|| "document".to_string());
                    let doc_block = DocumentBlock::builder()
                        .format(to_bedrock_doc_format(*format))
                        .source(DocumentSource::Bytes(Blob::new(data.clone())))
                        .name(doc_name)
                        .build()
                        .map_err(|e| ProviderError::Configuration(e.to_string()))?;
                    ToolResultContentBlock::Document(doc_block)
                }
            };
            let status = match result.status {
                ToolResultStatus::Success => BedrockToolResultStatus::Success,
                ToolResultStatus::Error => BedrockToolResultStatus::Error,
            };
            let block = BedrockToolResultBlock::builder()
                .tool_use_id(&result.tool_use_id)
                .content(content)
                .status(status)
                .build()
                .map_err(|e| ProviderError::Configuration(e.to_string()))?;
            Ok(BedrockContentBlock::ToolResult(block))
        }
        ContentBlock::Thinking { thinking, .. } => {
            // Pass thinking blocks as text for multi-turn conversations
            // Bedrock handles thinking through additionalModelRequestFields
            Ok(BedrockContentBlock::Text(format!(
                "<thinking>{}</thinking>",
                thinking
            )))
        }
        ContentBlock::ServerToolUse(server_use) => {
            // Server-side tool use blocks are informational - represent as text
            Ok(BedrockContentBlock::Text(format!(
                "[Server tool: {} ({})]",
                server_use.name, server_use.id
            )))
        }
        ContentBlock::ToolSearchResult(result) => {
            // Tool search results are informational - represent as text
            let refs: Vec<&str> = result
                .tool_references
                .iter()
                .map(|r| r.name.as_str())
                .collect();
            Ok(BedrockContentBlock::Text(format!(
                "[Tool search result: found tools: {}]",
                refs.join(", ")
            )))
        }
    }
}

pub fn to_bedrock_image_format(format: crate::tool::ImageFormat) -> BedrockImageFormat {
    use crate::tool::ImageFormat;
    match format {
        ImageFormat::Png => BedrockImageFormat::Png,
        ImageFormat::Jpeg => BedrockImageFormat::Jpeg,
        ImageFormat::Gif => BedrockImageFormat::Gif,
        ImageFormat::Webp => BedrockImageFormat::Webp,
    }
}

pub fn to_bedrock_doc_format(format: crate::tool::DocumentFormat) -> BedrockDocFormat {
    use crate::tool::DocumentFormat;
    match format {
        DocumentFormat::Pdf => BedrockDocFormat::Pdf,
        DocumentFormat::Csv => BedrockDocFormat::Csv,
        DocumentFormat::Doc => BedrockDocFormat::Doc,
        DocumentFormat::Docx => BedrockDocFormat::Docx,
        DocumentFormat::Html => BedrockDocFormat::Html,
        DocumentFormat::Md => BedrockDocFormat::Md,
        DocumentFormat::Txt => BedrockDocFormat::Txt,
        DocumentFormat::Xls => BedrockDocFormat::Xls,
        DocumentFormat::Xlsx => BedrockDocFormat::Xlsx,
    }
}

pub fn to_bedrock_tool(tool: &ToolDefinition) -> Result<BedrockTool, ProviderError> {
    let input_schema = ToolInputSchema::Json(json_to_document(&tool.input_schema));
    let spec = ToolSpecification::builder()
        .name(&tool.name)
        .description(&tool.description)
        .input_schema(input_schema)
        .build()
        .map_err(|e| ProviderError::Configuration(e.to_string()))?;
    Ok(BedrockTool::ToolSpec(spec))
}

pub fn json_to_document(value: &serde_json::Value) -> Document {
    match value {
        serde_json::Value::Null => Document::Null,
        serde_json::Value::Bool(b) => Document::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Document::Number(aws_smithy_types::Number::NegInt(i))
            } else if let Some(u) = n.as_u64() {
                Document::Number(aws_smithy_types::Number::PosInt(u))
            } else if let Some(f) = n.as_f64() {
                Document::Number(aws_smithy_types::Number::Float(f))
            } else {
                Document::Null
            }
        }
        serde_json::Value::String(s) => Document::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Document::Array(arr.iter().map(json_to_document).collect())
        }
        serde_json::Value::Object(obj) => Document::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), json_to_document(v)))
                .collect(),
        ),
    }
}

// ===== Type Conversion: Bedrock -> Mixtape =====

pub fn from_bedrock_message(msg: &BedrockMessage) -> Message {
    let role = match msg.role() {
        ConversationRole::User => Role::User,
        ConversationRole::Assistant => Role::Assistant,
        _ => Role::Assistant, // Default to assistant for unknown roles
    };

    let content: Vec<ContentBlock> = msg
        .content()
        .iter()
        .filter_map(from_bedrock_content_block)
        .collect();

    Message { role, content }
}

fn from_bedrock_content_block(block: &BedrockContentBlock) -> Option<ContentBlock> {
    match block {
        BedrockContentBlock::Text(text) => Some(ContentBlock::Text(text.clone())),
        BedrockContentBlock::ToolUse(tool_use) => {
            let input = document_to_json(tool_use.input());
            Some(ContentBlock::ToolUse(ToolUseBlock {
                id: tool_use.tool_use_id().to_string(),
                name: tool_use.name().to_string(),
                input,
            }))
        }
        _ => None, // Skip other content types (images, etc.)
    }
}

pub fn document_to_json(doc: &Document) -> serde_json::Value {
    match doc {
        Document::Null => serde_json::Value::Null,
        Document::Bool(b) => serde_json::Value::Bool(*b),
        Document::Number(n) => match n {
            aws_smithy_types::Number::PosInt(i) => serde_json::json!(*i),
            aws_smithy_types::Number::NegInt(i) => serde_json::json!(*i),
            aws_smithy_types::Number::Float(f) => serde_json::Value::Number(
                serde_json::Number::from_f64(*f).unwrap_or_else(|| 0.into()),
            ),
        },
        Document::String(s) => serde_json::Value::String(s.clone()),
        Document::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(document_to_json).collect())
        }
        Document::Object(obj) => serde_json::Value::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), document_to_json(v)))
                .collect(),
        ),
    }
}

pub fn from_bedrock_stop_reason(reason: &aws_sdk_bedrockruntime::types::StopReason) -> StopReason {
    match reason {
        aws_sdk_bedrockruntime::types::StopReason::EndTurn => StopReason::EndTurn,
        aws_sdk_bedrockruntime::types::StopReason::ToolUse => StopReason::ToolUse,
        aws_sdk_bedrockruntime::types::StopReason::MaxTokens => StopReason::MaxTokens,
        aws_sdk_bedrockruntime::types::StopReason::ContentFiltered => StopReason::ContentFiltered,
        aws_sdk_bedrockruntime::types::StopReason::StopSequence => StopReason::StopSequence,
        _ => StopReason::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolResultBlock;
    use aws_sdk_bedrockruntime::types::{
        ConversationRole, DocumentSource, ImageSource, Message as BedrockMessage, ToolInputSchema,
        ToolUseBlock as BedrockToolUseBlock,
    };

    #[test]
    fn test_json_to_document_roundtrip() {
        let json = serde_json::json!({
            "string": "hello",
            "number": 42,
            "float": 1.23,
            "bool": true,
            "null": null,
            "array": [1, 2, 3],
            "object": {"nested": "value"}
        });

        let doc = json_to_document(&json);
        let back = document_to_json(&doc);

        assert_eq!(json["string"], back["string"]);
        assert_eq!(json["number"], back["number"]);
        assert_eq!(json["bool"], back["bool"]);
        assert_eq!(json["null"], back["null"]);
        assert_eq!(json["array"], back["array"]);
        assert_eq!(json["object"], back["object"]);
    }

    #[test]
    fn test_message_conversion() {
        let msg = Message::user("Hello, world!");
        let bedrock_msg = to_bedrock_message(&msg).unwrap();

        assert_eq!(*bedrock_msg.role(), ConversationRole::User);
        assert_eq!(bedrock_msg.content().len(), 1);

        match &bedrock_msg.content()[0] {
            BedrockContentBlock::Text(text) => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected text content"),
        }

        // Convert back
        let back = from_bedrock_message(&bedrock_msg);
        assert_eq!(back.role, Role::User);
        assert_eq!(back.text(), "Hello, world!");
    }

    // ===== Content Block Conversion Tests =====

    #[test]
    fn test_content_block_tool_use_conversion() {
        let tool_use = ToolUseBlock {
            id: "tool_abc123".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({
                "path": "/tmp/test.txt",
                "encoding": "utf-8"
            }),
        };
        let block = ContentBlock::ToolUse(tool_use);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        // Verify using getters
        if let BedrockContentBlock::ToolUse(tu) = bedrock_block {
            assert_eq!(tu.tool_use_id(), "tool_abc123");
            assert_eq!(tu.name(), "read_file");

            // Round-trip the Document back to JSON to verify input
            let input_json = document_to_json(tu.input());
            assert_eq!(input_json["path"], "/tmp/test.txt");
            assert_eq!(input_json["encoding"], "utf-8");
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn test_content_block_tool_result_text_conversion() {
        let result = ToolResultBlock {
            tool_use_id: "tool_xyz789".to_string(),
            content: ToolResult::Text("File contents here".to_string()),
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        if let BedrockContentBlock::ToolResult(tr) = bedrock_block {
            assert_eq!(tr.tool_use_id(), "tool_xyz789");
            assert_eq!(tr.status(), Some(&BedrockToolResultStatus::Success));

            // Check content
            let content = tr.content();
            assert_eq!(content.len(), 1);
            match &content[0] {
                ToolResultContentBlock::Text(text) => assert_eq!(text, "File contents here"),
                _ => panic!("Expected Text content"),
            }
        } else {
            panic!("Expected ToolResult block");
        }
    }

    #[test]
    fn test_content_block_tool_result_json_conversion() {
        let result = ToolResultBlock {
            tool_use_id: "tool_json".to_string(),
            content: ToolResult::Json(serde_json::json!({
                "files": ["a.txt", "b.txt"],
                "count": 2
            })),
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        if let BedrockContentBlock::ToolResult(tr) = bedrock_block {
            assert_eq!(tr.tool_use_id(), "tool_json");

            let content = tr.content();
            assert_eq!(content.len(), 1);
            match &content[0] {
                ToolResultContentBlock::Json(doc) => {
                    let json = document_to_json(doc);
                    assert_eq!(json["count"], 2);
                    assert_eq!(json["files"][0], "a.txt");
                    assert_eq!(json["files"][1], "b.txt");
                }
                _ => panic!("Expected Json content"),
            }
        } else {
            panic!("Expected ToolResult block");
        }
    }

    #[test]
    fn test_content_block_tool_result_error_status() {
        let result = ToolResultBlock {
            tool_use_id: "tool_err".to_string(),
            content: ToolResult::Text("Error: file not found".to_string()),
            status: ToolResultStatus::Error,
        };
        let block = ContentBlock::ToolResult(result);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        if let BedrockContentBlock::ToolResult(tr) = bedrock_block {
            assert_eq!(tr.status(), Some(&BedrockToolResultStatus::Error));
        } else {
            panic!("Expected ToolResult block");
        }
    }

    #[test]
    fn test_content_block_tool_result_image_conversion() {
        use crate::tool::ImageFormat;

        let image_data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG magic bytes
        let result = ToolResultBlock {
            tool_use_id: "tool_img".to_string(),
            content: ToolResult::Image {
                format: ImageFormat::Png,
                data: image_data.clone(),
            },
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        if let BedrockContentBlock::ToolResult(tr) = bedrock_block {
            assert_eq!(tr.tool_use_id(), "tool_img");
            let content = tr.content();
            assert_eq!(content.len(), 1);
            match &content[0] {
                ToolResultContentBlock::Image(img) => {
                    assert_eq!(img.format(), &BedrockImageFormat::Png);
                    // Verify image data via source
                    if let Some(ImageSource::Bytes(blob)) = img.source() {
                        assert_eq!(blob.as_ref(), &image_data);
                    } else {
                        panic!("Expected Bytes source");
                    }
                }
                _ => panic!("Expected Image content"),
            }
        } else {
            panic!("Expected ToolResult block");
        }
    }

    #[test]
    fn test_content_block_tool_result_document_conversion() {
        use crate::tool::DocumentFormat;

        let doc_data = vec![0x25, 0x50, 0x44, 0x46]; // PDF magic bytes
        let result = ToolResultBlock {
            tool_use_id: "tool_doc".to_string(),
            content: ToolResult::Document {
                format: DocumentFormat::Pdf,
                data: doc_data.clone(),
                name: Some("report.pdf".to_string()),
            },
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        if let BedrockContentBlock::ToolResult(tr) = bedrock_block {
            let content = tr.content();
            assert_eq!(content.len(), 1);
            match &content[0] {
                ToolResultContentBlock::Document(doc) => {
                    assert_eq!(doc.format(), &BedrockDocFormat::Pdf);
                    assert_eq!(doc.name(), "report.pdf");
                    if let Some(DocumentSource::Bytes(blob)) = doc.source() {
                        assert_eq!(blob.as_ref(), &doc_data);
                    } else {
                        panic!("Expected Bytes source");
                    }
                }
                _ => panic!("Expected Document content"),
            }
        } else {
            panic!("Expected ToolResult block");
        }
    }

    #[test]
    fn test_content_block_tool_result_document_without_name() {
        use crate::tool::DocumentFormat;

        let result = ToolResultBlock {
            tool_use_id: "tool_doc_noname".to_string(),
            content: ToolResult::Document {
                format: DocumentFormat::Txt,
                data: vec![0x48, 0x65, 0x6c, 0x6c, 0x6f], // "Hello"
                name: None,
            },
            status: ToolResultStatus::Success,
        };
        let block = ContentBlock::ToolResult(result);

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        if let BedrockContentBlock::ToolResult(tr) = bedrock_block {
            let content = tr.content();
            match &content[0] {
                ToolResultContentBlock::Document(doc) => {
                    assert_eq!(doc.format(), &BedrockDocFormat::Txt);
                    // When name is not provided, we default to "document"
                    assert_eq!(doc.name(), "document");
                }
                _ => panic!("Expected Document content"),
            }
        } else {
            panic!("Expected ToolResult block");
        }
    }

    // ===== Image Format Conversion Tests =====

    #[test]
    fn test_image_format_conversion() {
        use crate::tool::ImageFormat;

        assert_eq!(
            to_bedrock_image_format(ImageFormat::Png),
            BedrockImageFormat::Png
        );
        assert_eq!(
            to_bedrock_image_format(ImageFormat::Jpeg),
            BedrockImageFormat::Jpeg
        );
        assert_eq!(
            to_bedrock_image_format(ImageFormat::Gif),
            BedrockImageFormat::Gif
        );
        assert_eq!(
            to_bedrock_image_format(ImageFormat::Webp),
            BedrockImageFormat::Webp
        );
    }

    // ===== Document Format Conversion Tests =====

    #[test]
    fn test_document_format_conversion() {
        use crate::tool::DocumentFormat;

        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Pdf),
            BedrockDocFormat::Pdf
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Csv),
            BedrockDocFormat::Csv
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Doc),
            BedrockDocFormat::Doc
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Docx),
            BedrockDocFormat::Docx
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Html),
            BedrockDocFormat::Html
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Md),
            BedrockDocFormat::Md
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Txt),
            BedrockDocFormat::Txt
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Xls),
            BedrockDocFormat::Xls
        );
        assert_eq!(
            to_bedrock_doc_format(DocumentFormat::Xlsx),
            BedrockDocFormat::Xlsx
        );
    }

    // ===== Tool Definition Conversion Tests =====

    #[test]
    fn test_tool_definition_conversion() {
        let tool_def = ToolDefinition {
            name: "search".to_string(),
            description: "Search for files".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "max_results": {"type": "integer"}
                },
                "required": ["query"]
            }),
            defer_loading: false,
        };

        let bedrock_tool = to_bedrock_tool(&tool_def).unwrap();

        if let BedrockTool::ToolSpec(spec) = bedrock_tool {
            assert_eq!(spec.name(), "search");
            assert_eq!(spec.description(), Some("Search for files"));

            // Verify schema via round-trip
            if let Some(ToolInputSchema::Json(doc)) = spec.input_schema() {
                let schema = document_to_json(doc);
                assert_eq!(schema["type"], "object");
                assert_eq!(schema["properties"]["query"]["type"], "string");
                assert_eq!(schema["properties"]["max_results"]["type"], "integer");
                assert_eq!(schema["required"][0], "query");
            } else {
                panic!("Expected Json schema");
            }
        } else {
            panic!("Expected ToolSpec");
        }
    }

    // ===== Stop Reason Conversion Tests =====

    #[test]
    fn test_stop_reason_conversion_all_variants() {
        use aws_sdk_bedrockruntime::types::StopReason as BedrockStopReason;

        assert_eq!(
            from_bedrock_stop_reason(&BedrockStopReason::EndTurn),
            StopReason::EndTurn
        );
        assert_eq!(
            from_bedrock_stop_reason(&BedrockStopReason::ToolUse),
            StopReason::ToolUse
        );
        assert_eq!(
            from_bedrock_stop_reason(&BedrockStopReason::MaxTokens),
            StopReason::MaxTokens
        );
        assert_eq!(
            from_bedrock_stop_reason(&BedrockStopReason::ContentFiltered),
            StopReason::ContentFiltered
        );
        assert_eq!(
            from_bedrock_stop_reason(&BedrockStopReason::StopSequence),
            StopReason::StopSequence
        );
    }

    // ===== Role Conversion Tests =====

    #[test]
    fn test_message_conversion_assistant() {
        let msg = Message::assistant("I can help with that.");
        let bedrock_msg = to_bedrock_message(&msg).unwrap();

        assert_eq!(*bedrock_msg.role(), ConversationRole::Assistant);
        assert_eq!(bedrock_msg.content().len(), 1);

        match &bedrock_msg.content()[0] {
            BedrockContentBlock::Text(text) => assert_eq!(text, "I can help with that."),
            _ => panic!("Expected text content"),
        }

        // Convert back
        let back = from_bedrock_message(&bedrock_msg);
        assert_eq!(back.role, Role::Assistant);
        assert_eq!(back.text(), "I can help with that.");
    }

    // ===== Thinking Block Conversion Tests =====

    #[test]
    fn test_content_block_thinking_conversion() {
        let block = ContentBlock::Thinking {
            thinking: "Let me analyze this problem...".to_string(),
            signature: "sig_abc123".to_string(),
        };

        let bedrock_block = to_bedrock_content_block(&block).unwrap();

        // Thinking blocks are converted to text with <thinking> tags for Bedrock
        match bedrock_block {
            BedrockContentBlock::Text(text) => {
                assert!(text.contains("<thinking>"));
                assert!(text.contains("Let me analyze this problem..."));
                assert!(text.contains("</thinking>"));
            }
            _ => panic!("Expected Text block for thinking"),
        }
    }

    // ===== JSON/Document Number Edge Cases =====

    #[test]
    fn test_json_to_document_negative_integer() {
        let json = serde_json::json!(-42);
        let doc = json_to_document(&json);
        let back = document_to_json(&doc);
        assert_eq!(back, serde_json::json!(-42));
    }

    #[test]
    fn test_json_to_document_large_positive_integer() {
        let json = serde_json::json!(9007199254740991_u64); // Max safe integer
        let doc = json_to_document(&json);
        let back = document_to_json(&doc);
        assert_eq!(back, serde_json::json!(9007199254740991_u64));
    }

    #[test]
    fn test_json_to_document_float_special_values() {
        // Test regular float (avoiding mathematical constants like pi/e)
        let json = serde_json::json!(1.23456);
        let doc = json_to_document(&json);
        let back = document_to_json(&doc);
        assert!((back.as_f64().unwrap() - 1.23456).abs() < 0.0001);
    }

    #[test]
    fn test_json_to_document_nested_structure() {
        let json = serde_json::json!({
            "level1": {
                "level2": {
                    "level3": ["a", "b", "c"]
                }
            }
        });
        let doc = json_to_document(&json);
        let back = document_to_json(&doc);
        assert_eq!(back["level1"]["level2"]["level3"][0], "a");
        assert_eq!(back["level1"]["level2"]["level3"][2], "c");
    }

    #[test]
    fn test_json_to_document_empty_object() {
        let json = serde_json::json!({});
        let doc = json_to_document(&json);
        let back = document_to_json(&doc);
        assert!(back.is_object());
        assert_eq!(back.as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_json_to_document_empty_array() {
        let json = serde_json::json!([]);
        let doc = json_to_document(&json);
        let back = document_to_json(&doc);
        assert!(back.is_array());
        assert_eq!(back.as_array().unwrap().len(), 0);
    }

    // ===== Response Parsing (from_bedrock) Tests =====

    #[test]
    fn test_from_bedrock_content_block_tool_use() {
        // Build a tool use block using the SDK builder
        let tool_use = BedrockToolUseBlock::builder()
            .tool_use_id("tool_response_123")
            .name("get_weather")
            .input(json_to_document(&serde_json::json!({"city": "Seattle"})))
            .build()
            .unwrap();

        let bedrock_block = BedrockContentBlock::ToolUse(tool_use);
        let result = from_bedrock_content_block(&bedrock_block);

        assert!(result.is_some());
        match result.unwrap() {
            ContentBlock::ToolUse(tu) => {
                assert_eq!(tu.id, "tool_response_123");
                assert_eq!(tu.name, "get_weather");
                assert_eq!(tu.input["city"], "Seattle");
            }
            _ => panic!("Expected ToolUse block"),
        }
    }

    #[test]
    fn test_from_bedrock_message_multiple_content_blocks() {
        // Create a message with text and tool use
        let text_block = BedrockContentBlock::Text("Here's the weather info:".to_string());
        let tool_use = BedrockToolUseBlock::builder()
            .tool_use_id("tool_456")
            .name("format_response")
            .input(json_to_document(&serde_json::json!({})))
            .build()
            .unwrap();
        let tool_block = BedrockContentBlock::ToolUse(tool_use);

        let bedrock_msg = BedrockMessage::builder()
            .role(ConversationRole::Assistant)
            .content(text_block)
            .content(tool_block)
            .build()
            .unwrap();

        let msg = from_bedrock_message(&bedrock_msg);

        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert!(matches!(&msg.content[0], ContentBlock::Text(_)));
        assert!(matches!(&msg.content[1], ContentBlock::ToolUse(_)));
    }
}
