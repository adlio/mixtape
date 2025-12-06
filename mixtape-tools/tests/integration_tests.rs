// Integration tests for mixtape-tools
//
// These tests verify that the tools work correctly when used together
// and integrate properly with the mixtape framework.

use mixtape_core::Tool;
use mixtape_tools::filesystem::*;
use mixtape_tools::process::*;
use mixtape_tools::search::*;
use tempfile::TempDir;

/// Test that filesystem tools integrate correctly
#[tokio::test]
async fn test_filesystem_tools_integration() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    // Test reading and writing
    let write_tool = WriteFileTool::with_base_path(base_path.clone());
    let read_tool = ReadFileTool::with_base_path(base_path.clone());

    // Write a file
    let write_input = serde_json::json!({
        "path": "test.txt",
        "content": "Hello, World!",
        "mode": "rewrite"
    });
    let write_input_typed = serde_json::from_value(write_input).unwrap();
    let write_result = write_tool.execute(write_input_typed).await;
    assert!(write_result.is_ok());

    // Read it back
    let read_input = serde_json::json!({
        "path": "test.txt"
    });
    let read_input_typed = serde_json::from_value(read_input).unwrap();
    let read_result = read_tool.execute(read_input_typed).await.unwrap();
    assert_eq!(read_result.as_text(), "Hello, World!");
}

/// Test that process tools work correctly
#[tokio::test]
async fn test_process_tools_basic() {
    let list_tool = ListProcessesTool;

    // List processes
    let input = serde_json::json!({});
    let input_typed = serde_json::from_value(input).unwrap();
    let result = list_tool.execute(input_typed).await;

    assert!(result.is_ok());
    let output = result.unwrap().as_text();
    assert!(output.contains("PID"));
    assert!(output.contains("NAME"));
}

/// Test that search tools integrate with filesystem
#[tokio::test]
async fn test_search_integration() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_path_buf();

    // Create some test files
    std::fs::write(
        temp_dir.path().join("test1.rs"),
        "fn main() { println!(\"hello\"); }",
    )
    .unwrap();
    std::fs::write(
        temp_dir.path().join("test2.rs"),
        "fn greet() { println!(\"world\"); }",
    )
    .unwrap();
    std::fs::write(temp_dir.path().join("readme.md"), "# Documentation").unwrap();

    let search_tool = SearchTool::with_base_path(base_path);

    // Search for Rust files
    let input = serde_json::json!({
        "root_path": ".",
        "pattern": "fn",
        "search_type": "content",
        "file_pattern": "*.rs"
    });
    let input_typed = serde_json::from_value(input).unwrap();
    let result = search_tool.execute(input_typed).await;

    assert!(result.is_ok());
    let output = result.unwrap().as_text();
    assert!(output.contains("test1.rs") || output.contains("test2.rs"));
}

/// Test tool schema generation
#[test]
fn test_tool_schemas_are_valid() {
    let read_tool = ReadFileTool::new();
    let schema = read_tool.input_schema();

    // Verify schema is valid JSON
    assert!(schema.is_object());

    // Verify it has expected fields
    let schema_obj = schema.as_object().unwrap();
    assert!(schema_obj.contains_key("$schema") || schema_obj.contains_key("definitions"));
}
