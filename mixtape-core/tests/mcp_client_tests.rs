//! Integration tests for MCP client with mock server

#![cfg(feature = "mcp")]

mod common;

use common::mock_mcp_server;
use mixtape_core::mcp::{McpClient, McpServerConfig, McpTransport};

/// Helper to create a client configured for the mock server
fn mock_client(name: &str) -> McpClient {
    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        name,
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    );
    McpClient::new(config).expect("Failed to create client")
}

#[tokio::test]
async fn test_connect_to_mock_server() {
    let client = mock_client("test-server");

    // Should connect successfully
    let result = client.connect().await;
    assert!(result.is_ok(), "Failed to connect: {:?}", result.err());

    // Disconnect cleanly
    client.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_list_tools_from_mock_server() {
    let client = mock_client("test-server");

    let tools = client.list_tools().await.expect("Failed to list tools");

    // Mock server provides 3 tools: echo, add, fail
    assert_eq!(tools.len(), 3);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"));
    assert!(tool_names.contains(&"add"));
    assert!(tool_names.contains(&"fail"));

    // Check echo tool has correct schema
    let echo = tools.iter().find(|t| t.name == "echo").unwrap();
    assert_eq!(echo.description, "Echo back the input");
    assert!(echo.input_schema["properties"]["message"].is_object());
}

#[tokio::test]
async fn test_call_echo_tool() {
    let client = mock_client("test-server");

    let result = client
        .call_tool(
            "echo".to_string(),
            serde_json::json!({"message": "Hello, MCP!"}),
        )
        .await
        .expect("Failed to call tool");

    // Result should contain the echoed message
    let content = &result["content"];
    assert!(content.is_array());
    assert_eq!(content[0]["text"], "Hello, MCP!");
}

#[tokio::test]
async fn test_call_add_tool() {
    let client = mock_client("test-server");

    let result = client
        .call_tool("add".to_string(), serde_json::json!({"a": 5, "b": 3}))
        .await
        .expect("Failed to call tool");

    let content = &result["content"];
    assert_eq!(content[0]["text"], "8");
}

#[tokio::test]
async fn test_call_failing_tool() {
    let client = mock_client("test-server");

    let result = client
        .call_tool("fail".to_string(), serde_json::json!({}))
        .await
        .expect("Call should succeed even if tool reports error");

    // The tool returns isError: true
    assert_eq!(result["isError"], true);
    assert_eq!(result["content"][0]["text"], "This tool always fails");
}

#[tokio::test]
async fn test_idempotent_connect() {
    let client = mock_client("test-server");

    // Connect multiple times - should be safe
    client.connect().await.unwrap();
    client.connect().await.unwrap();
    client.connect().await.unwrap();

    // Should still work
    let tools = client.list_tools().await.unwrap();
    assert!(!tools.is_empty());
}

#[tokio::test]
async fn test_reconnect_after_disconnect() {
    let client = mock_client("test-server");

    // Connect, use, disconnect
    client.connect().await.unwrap();
    let tools1 = client.list_tools().await.unwrap();
    client.disconnect().await.unwrap();

    // Reconnect and use again
    client.connect().await.unwrap();
    let tools2 = client.list_tools().await.unwrap();

    assert_eq!(tools1.len(), tools2.len());
}

#[tokio::test]
async fn test_lazy_connect_on_list_tools() {
    let client = mock_client("test-server");

    // Don't explicitly connect - list_tools should lazy connect
    let tools = client.list_tools().await.expect("Lazy connect failed");
    assert!(!tools.is_empty());
}

#[tokio::test]
async fn test_lazy_connect_on_call_tool() {
    let client = mock_client("test-server");

    // Don't explicitly connect - call_tool should lazy connect
    let result = client
        .call_tool("echo".to_string(), serde_json::json!({"message": "lazy"}))
        .await
        .expect("Lazy connect failed");

    assert_eq!(result["content"][0]["text"], "lazy");
}

#[tokio::test]
async fn test_client_name() {
    let client = mock_client("my-test-server");
    assert_eq!(client.name(), "my-test-server");
}

#[tokio::test]
async fn test_tool_with_namespace() {
    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "namespaced",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .with_namespace("test"); // Add namespace prefix

    let client = McpClient::new(config).unwrap();
    let tools = client.list_tools().await.unwrap();

    // Should still get all 3 tools from server
    assert_eq!(tools.len(), 3);
}

// ============================================================================
// Agent MCP Integration Tests
// ============================================================================

use common::{AutoApproveGrantStore, EventCollector, MockProvider};
use mixtape_core::Agent;

/// Helper to create an MCP server config for the mock server
fn mock_mcp_config(name: &str) -> McpServerConfig {
    let (cmd, args) = mock_mcp_server::command();
    McpServerConfig::new(
        name,
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
}

#[tokio::test]
async fn test_agent_add_mcp_server() {
    let provider = MockProvider::new().with_text("Done");
    let mut agent = Agent::builder().provider(provider).build().await.unwrap();

    // Initially no tools
    assert_eq!(agent.list_tools().len(), 0);

    // Add MCP server
    let config = mock_mcp_config("mock-server");
    agent.add_mcp_server(config).await.unwrap();

    // Should now have the 3 tools from mock server
    let tools = agent.list_tools();
    assert_eq!(tools.len(), 3);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    // By default, tools are namespaced with server name
    assert!(tool_names.contains(&"mock-server_echo"));
    assert!(tool_names.contains(&"mock-server_add"));
    assert!(tool_names.contains(&"mock-server_fail"));

    // Clean shutdown
    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_add_mcp_server_without_namespace() {
    let provider = MockProvider::new().with_text("Done");
    let mut agent = Agent::builder().provider(provider).build().await.unwrap();

    // Add MCP server without namespace
    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "mock-server",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .without_namespace();

    agent.add_mcp_server(config).await.unwrap();

    // Tools should not have namespace prefix
    let tools = agent.list_tools();
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"));
    assert!(tool_names.contains(&"add"));
    assert!(tool_names.contains(&"fail"));

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_add_mcp_server_with_custom_namespace() {
    let provider = MockProvider::new().with_text("Done");
    let mut agent = Agent::builder().provider(provider).build().await.unwrap();

    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "server",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .with_namespace("custom");

    agent.add_mcp_server(config).await.unwrap();

    let tools = agent.list_tools();
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"custom_echo"));
    assert!(tool_names.contains(&"custom_add"));
    assert!(tool_names.contains(&"custom_fail"));

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_add_mcp_server_with_tool_filter() {
    let provider = MockProvider::new().with_text("Done");
    let mut agent = Agent::builder().provider(provider).build().await.unwrap();

    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "filtered",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .without_namespace()
    .only_tools(["echo", "add"]); // Exclude "fail" tool

    agent.add_mcp_server(config).await.unwrap();

    // Should only have echo and add, not fail
    let tools = agent.list_tools();
    assert_eq!(tools.len(), 2);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"));
    assert!(tool_names.contains(&"add"));
    assert!(!tool_names.contains(&"fail"));

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_add_mcp_server_with_exclude_filter() {
    let provider = MockProvider::new().with_text("Done");
    let mut agent = Agent::builder().provider(provider).build().await.unwrap();

    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "filtered",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .without_namespace()
    .exclude_tools(["fail"]); // Exclude "fail" tool

    agent.add_mcp_server(config).await.unwrap();

    // Should have echo and add, not fail
    let tools = agent.list_tools();
    assert_eq!(tools.len(), 2);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"));
    assert!(tool_names.contains(&"add"));
    assert!(!tool_names.contains(&"fail"));

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_builder_with_mcp_server() {
    let provider = MockProvider::new().with_text("Done");
    let config = mock_mcp_config("builder-test");

    let agent = Agent::builder()
        .provider(provider)
        .with_mcp_server(config)
        .build()
        .await
        .unwrap();

    // Should have tools from MCP server
    let tools = agent.list_tools();
    assert_eq!(tools.len(), 3);

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_multiple_mcp_servers() {
    let provider = MockProvider::new().with_text("Done");
    let mut agent = Agent::builder().provider(provider).build().await.unwrap();

    // Add two MCP servers with different namespaces
    let config1 = mock_mcp_config("server1");
    let config2 = mock_mcp_config("server2");

    agent.add_mcp_server(config1).await.unwrap();
    agent.add_mcp_server(config2).await.unwrap();

    // Should have 6 tools total (3 from each server)
    let tools = agent.list_tools();
    assert_eq!(tools.len(), 6);

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"server1_echo"));
    assert!(tool_names.contains(&"server2_echo"));

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_use_mcp_tool() {
    // Test that agent can actually execute MCP tools
    let provider = MockProvider::new()
        .with_tool_use("echo", serde_json::json!({"message": "Hello from MCP!"}))
        .with_text("The tool said: Hello from MCP!");

    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "mcp",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .without_namespace();

    let agent = Agent::builder()
        .provider(provider)
        .with_grant_store(AutoApproveGrantStore)
        .with_mcp_server(config)
        .build()
        .await
        .unwrap();

    // Run the agent
    let response = agent.run("Echo something").await.unwrap();
    assert_eq!(response, "The tool said: Hello from MCP!");

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_mcp_tool_events() {
    let provider = MockProvider::new()
        .with_tool_use("add", serde_json::json!({"a": 10, "b": 20}))
        .with_text("The sum is 30");

    let (cmd, args) = mock_mcp_server::command();
    let config = McpServerConfig::new(
        "math",
        McpTransport::stdio(&cmd).args(args.iter().map(|s| s.as_str())),
    )
    .without_namespace();

    let agent = Agent::builder()
        .provider(provider)
        .with_grant_store(AutoApproveGrantStore)
        .with_mcp_server(config)
        .build()
        .await
        .unwrap();

    // Add event collector
    let collector = EventCollector::new();
    let collector_clone = collector.clone();
    agent.add_hook(collector);

    agent.run("Add 10 and 20").await.unwrap();

    // Should have tool events
    let events = collector_clone.events();
    assert!(events.contains(&"tool_requested".to_string()));
    assert!(events.contains(&"tool_completed".to_string()));

    agent.shutdown().await;
}

#[tokio::test]
async fn test_agent_shutdown_disconnects_mcp() {
    let provider = MockProvider::new().with_text("Done");
    let config = mock_mcp_config("shutdown-test");

    let agent = Agent::builder()
        .provider(provider)
        .with_mcp_server(config)
        .build()
        .await
        .unwrap();

    // Verify we have tools (proving MCP server was connected)
    assert_eq!(agent.list_tools().len(), 3);

    // Shutdown should disconnect all MCP clients without panicking
    agent.shutdown().await;
}
