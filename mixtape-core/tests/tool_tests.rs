use mixtape_core::{ToolError, ToolResult};
use serde::Serialize;

// ===== ToolResult Helper Method Tests =====

#[test]
fn test_tool_result_text_factory() {
    let result = ToolResult::text("Hello");
    assert!(matches!(result, ToolResult::Text(_)));

    if let ToolResult::Text(s) = result {
        assert_eq!(s, "Hello");
    }

    // Test with String
    let result2 = ToolResult::text(String::from("World"));
    if let ToolResult::Text(s) = result2 {
        assert_eq!(s, "World");
    }
}

#[test]
fn test_tool_result_json_factory() {
    #[derive(Serialize)]
    struct TestData {
        value: i32,
        name: String,
    }

    let data = TestData {
        value: 42,
        name: "test".to_string(),
    };

    let result = ToolResult::json(data).unwrap();
    assert!(matches!(result, ToolResult::Json(_)));

    if let ToolResult::Json(v) = result {
        assert_eq!(v["value"], 42);
        assert_eq!(v["name"], "test");
    }
}

#[test]
fn test_tool_result_as_text() {
    // Test with Text variant
    let text_result = ToolResult::Text("Hello".to_string());
    assert_eq!(text_result.as_text(), "Hello");

    // Test with Json variant
    let json_result = ToolResult::Json(serde_json::json!({"key": "value"}));
    let text = json_result.as_text();
    assert!(text.contains("key"));
    assert!(text.contains("value"));
}

#[test]
fn test_tool_result_as_str() {
    // Test with Text variant
    let text_result = ToolResult::Text("Hello".to_string());
    assert_eq!(text_result.as_str(), Some("Hello"));

    // Test with Json variant
    let json_result = ToolResult::Json(serde_json::json!({"key": "value"}));
    assert_eq!(json_result.as_str(), None);
}

#[test]
fn test_tool_result_from_string() {
    let result: ToolResult = String::from("Test").into();
    assert!(matches!(result, ToolResult::Text(_)));

    if let ToolResult::Text(s) = result {
        assert_eq!(s, "Test");
    }
}

#[test]
fn test_tool_result_from_str() {
    let result: ToolResult = "Test".into();
    assert!(matches!(result, ToolResult::Text(_)));

    if let ToolResult::Text(s) = result {
        assert_eq!(s, "Test");
    }
}

// ===== ToolError From Implementation Tests =====

#[test]
fn test_tool_error_from_string() {
    let error: ToolError = String::from("Test error").into();
    assert_eq!(error.to_string(), "Test error");
}

#[test]
fn test_tool_error_from_str() {
    let error: ToolError = "Test error".into();
    assert_eq!(error.to_string(), "Test error");
}

#[test]
fn test_tool_error_variants() {
    // Test Custom variant
    let custom = ToolError::Custom("custom error".to_string());
    assert_eq!(custom.to_string(), "custom error");

    // Test PathValidation variant
    let path_error = ToolError::PathValidation("invalid path".to_string());
    assert!(path_error.to_string().contains("invalid path"));
}
