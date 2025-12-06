use mixtape_core::{
    ClaudeSonnet4_5, ContentBlock, DocumentFormat, ImageFormat, Message, Model, ToolResult,
    ToolResultBlock, ToolResultStatus, ToolUseBlock,
};

// ===== Token Estimation Tests =====

#[test]
fn test_estimate_token_count_basic() {
    let model = ClaudeSonnet4_5;

    // Empty string
    assert_eq!(model.estimate_token_count(""), 0);

    // Short string (~4 chars per token)
    assert_eq!(model.estimate_token_count("test"), 1);

    // 8 chars = 2 tokens
    assert_eq!(model.estimate_token_count("testtest"), 2);

    // 100 chars = 25 tokens
    let long_text = "x".repeat(100);
    assert_eq!(model.estimate_token_count(&long_text), 25);
}

#[test]
fn test_estimate_message_tokens_user_text() {
    let model = ClaudeSonnet4_5;

    let messages = vec![Message::user("Hello, world!")]; // 13 chars = 4 tokens + 4 role overhead

    let estimate = model.estimate_message_tokens(&messages);
    assert!(estimate > 0);
    // Should be approximately 4 (role) + 4 (text) = 8
    assert_eq!(estimate, 8);
}

#[test]
fn test_estimate_message_tokens_multiple_messages() {
    let model = ClaudeSonnet4_5;

    let messages = vec![
        Message::user("Hello"),       // 5 chars = 2 tokens + 4 role
        Message::assistant("Hi"),     // 2 chars = 1 token + 4 role
        Message::user("How are you"), // 11 chars = 3 tokens + 4 role
    ];

    let estimate = model.estimate_message_tokens(&messages);
    // 3 messages * 4 role overhead + (2 + 1 + 3) text tokens = 12 + 6 = 18
    assert_eq!(estimate, 18);
}

#[test]
fn test_estimate_content_block_tokens_text() {
    let model = ClaudeSonnet4_5;

    let block = ContentBlock::Text("This is a test message".to_string());
    let estimate = model.estimate_content_block_tokens(&block);

    // 22 chars / 4 = 6 tokens
    assert_eq!(estimate, 6);
}

#[test]
fn test_estimate_content_block_tokens_tool_use() {
    let model = ClaudeSonnet4_5;

    let tool_use = ToolUseBlock {
        id: "tool_123".to_string(),
        name: "read_file".to_string(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
    };
    let block = ContentBlock::ToolUse(tool_use);

    let estimate = model.estimate_content_block_tokens(&block);

    // Should include: name tokens + id tokens + input JSON tokens + 10 overhead
    // "read_file" = 9 chars = 3 tokens
    // "tool_123" = 8 chars = 2 tokens
    // JSON string ~25 chars = 7 tokens
    // + 10 overhead = ~22 tokens
    assert!(estimate > 10, "Tool use should have reasonable token count");
    assert!(estimate < 50, "Tool use shouldn't be excessive");
}

#[test]
fn test_estimate_content_block_tokens_tool_result_text() {
    let model = ClaudeSonnet4_5;

    let result = ToolResultBlock {
        tool_use_id: "tool_123".to_string(),
        content: ToolResult::Text("File contents here".to_string()),
        status: ToolResultStatus::Success,
    };
    let block = ContentBlock::ToolResult(result);

    let estimate = model.estimate_content_block_tokens(&block);

    // "tool_123" = 2 tokens + "File contents here" = 5 tokens + 10 overhead = 17
    assert!(estimate > 5);
    assert!(estimate < 30);
}

#[test]
fn test_estimate_content_block_tokens_tool_result_json() {
    let model = ClaudeSonnet4_5;

    let result = ToolResultBlock {
        tool_use_id: "id".to_string(),
        content: ToolResult::Json(serde_json::json!({
            "files": ["a.txt", "b.txt", "c.txt"],
            "count": 3
        })),
        status: ToolResultStatus::Success,
    };
    let block = ContentBlock::ToolResult(result);

    let estimate = model.estimate_content_block_tokens(&block);
    assert!(
        estimate > 10,
        "JSON result should have meaningful token count"
    );
}

#[test]
fn test_estimate_content_block_tokens_tool_result_image() {
    let model = ClaudeSonnet4_5;

    // 7500 bytes = ~10 tokens (1 token per 750 bytes) + 85 base overhead
    let image_data = vec![0u8; 7500];

    let result = ToolResultBlock {
        tool_use_id: "img_1".to_string(),
        content: ToolResult::Image {
            format: ImageFormat::Png,
            data: image_data,
        },
        status: ToolResultStatus::Success,
    };
    let block = ContentBlock::ToolResult(result);

    let estimate = model.estimate_content_block_tokens(&block);

    // Should be: id tokens + (7500/750 + 85) image tokens + 10 overhead
    // = 2 + 95 + 10 = ~107
    assert!(
        estimate > 90,
        "Image should have significant token overhead"
    );
    assert!(estimate < 150);
}

#[test]
fn test_estimate_content_block_tokens_tool_result_document() {
    let model = ClaudeSonnet4_5;

    // 5000 bytes = 10 tokens (1 per 500 bytes) + 50 base overhead
    let doc_data = vec![0u8; 5000];

    let result = ToolResultBlock {
        tool_use_id: "doc_1".to_string(),
        content: ToolResult::Document {
            format: DocumentFormat::Pdf,
            data: doc_data,
            name: Some("report.pdf".to_string()),
        },
        status: ToolResultStatus::Success,
    };
    let block = ContentBlock::ToolResult(result);

    let estimate = model.estimate_content_block_tokens(&block);

    // Should be: id tokens + (5000/500 + 50) doc tokens + 10 overhead
    // = 2 + 60 + 10 = ~72
    assert!(
        estimate > 50,
        "Document should have significant token overhead"
    );
    assert!(estimate < 100);
}

#[test]
fn test_estimate_message_tokens_with_tool_content() {
    let model = ClaudeSonnet4_5;

    // Create a message with multiple content blocks including tool use
    let tool_use = ToolUseBlock {
        id: "t1".to_string(),
        name: "test".to_string(),
        input: serde_json::json!({}),
    };

    let message = Message {
        role: mixtape_core::Role::Assistant,
        content: vec![
            ContentBlock::Text("Let me help you.".to_string()),
            ContentBlock::ToolUse(tool_use),
        ],
    };

    let estimate = model.estimate_message_tokens(&[message]);

    // Should include role overhead + text tokens + tool use tokens
    assert!(estimate > 10);
}

#[test]
fn test_estimate_empty_messages() {
    let model = ClaudeSonnet4_5;

    let messages: Vec<Message> = vec![];
    let estimate = model.estimate_message_tokens(&messages);

    assert_eq!(estimate, 0);
}
