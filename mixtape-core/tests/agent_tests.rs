mod common;

use common::{
    AutoApproveGrantStore, Calculator, DataTool, DetailedEventCollector, ErrorTool, EventCollector,
    MockProvider,
};
use mixtape_core::{Agent, AgentEvent, ToolResult};

#[tokio::test]
async fn test_agent_simple_text_response() {
    let provider = MockProvider::new().with_text("Hello, world!");

    let agent = Agent::builder().provider(provider).build().await.unwrap();

    let response = agent.run("Say hello").await.unwrap();
    assert_eq!(response, "Hello, world!");
}

#[tokio::test]
async fn test_agent_with_tool_use() {
    // Set up mock to:
    // 1. Request tool use
    // 2. Respond with final answer after tool result
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"expression": "2+2"}))
        .with_text("The answer is 4");

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent.run("What is 2+2?").await.unwrap();
    assert_eq!(response, "The answer is 4");
}

#[tokio::test]
async fn test_agent_with_system_prompt() {
    let provider = MockProvider::new().with_text("I am helpful!");

    let agent = Agent::builder()
        .provider(provider)
        .with_system_prompt("You are a helpful assistant")
        .build()
        .await
        .unwrap();

    let response = agent.run("Who are you?").await.unwrap();
    assert_eq!(response, "I am helpful!");
}

#[tokio::test]
async fn test_agent_multiple_tool_calls() {
    // Test that agent handles multiple sequential tool calls
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"expression": "2+2"}))
        .with_tool_use("calculate", serde_json::json!({"expression": "5+5"}))
        .with_text("The answers are 4 and 10");

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent.run("Calculate 2+2 and 5+5").await.unwrap();
    assert_eq!(response, "The answers are 4 and 10");
}

#[tokio::test]
async fn test_agent_tool_not_found() {
    // When model requests a tool that doesn't exist, it should error gracefully
    let provider = MockProvider::new()
        .with_tool_use("nonexistent_tool", serde_json::json!({"param": "value"}))
        .with_text("Fallback response");

    let agent = Agent::builder().provider(provider).build().await.unwrap();

    // This should complete even though tool wasn't found
    // The error will be in the tool result sent back to the model
    let response = agent.run("Use a tool").await.unwrap();
    assert_eq!(response, "Fallback response");
}

#[tokio::test]
async fn test_max_concurrent_tools() {
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"expression": "1+1"}))
        .with_text("Done");

    let agent = Agent::builder()
        .provider(provider)
        .with_max_concurrent_tools(5)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent.run("Calculate").await.unwrap();
    assert_eq!(response, "Done");
}

#[tokio::test]
async fn test_provider_call_count() {
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"expression": "2+2"}))
        .with_text("Done");

    let provider_clone = provider.clone();

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    agent.run("Test").await.unwrap();

    // Should have been called twice: once for initial, once after tool result
    assert_eq!(provider_clone.call_count(), 2);
}

// ===== Event Hook Tests =====

#[tokio::test]
async fn test_hooks_simple_run() {
    let provider = MockProvider::new().with_text("Response");
    let collector = EventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder().provider(provider).build().await.unwrap();
    agent.add_hook(collector);

    agent.run("Test").await.unwrap();

    let events = collector_clone.events();

    // Verify key events were emitted (streaming events may be interspersed)
    assert!(
        events.len() >= 4,
        "Expected at least 4 events, got {}",
        events.len()
    );
    assert_eq!(events[0], "run_started");
    assert_eq!(events[1], "model_call_started");
    // model_streaming events may appear here
    assert!(events.contains(&"model_call_completed".to_string()));
    assert_eq!(events.last().unwrap(), "run_completed");
}

#[tokio::test]
async fn test_hooks_with_tool_execution() {
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"expression": "2+2"}))
        .with_text("Done");

    let collector = EventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();
    agent.add_hook(collector);

    agent.run("Calculate").await.unwrap();

    let events = collector_clone.events();

    // Should see: run_started, model_call_started, model_call_completed,
    // tool_requested, tool_executing, tool_completed, model_call_started, model_call_completed, run_completed
    assert!(events.contains(&"run_started".to_string()));
    assert!(events.contains(&"tool_requested".to_string()));
    assert!(events.contains(&"tool_completed".to_string()));
    assert!(events.contains(&"run_completed".to_string()));
}

#[tokio::test]
async fn test_hooks_tool_error() {
    let provider = MockProvider::new()
        .with_tool_use("error_tool", serde_json::json!({"expression": "test"}))
        .with_text("Handled error");

    let collector = EventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(ErrorTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();
    agent.add_hook(collector);

    agent.run("Test").await.unwrap();

    let events = collector_clone.events();

    // Should see tool_failed event
    assert!(events.contains(&"tool_failed".to_string()));
}

// ===== Json Result Tests =====

#[tokio::test]
async fn test_tool_json_result() {
    let provider = MockProvider::new()
        .with_tool_use("get_data", serde_json::json!({"key": "test"}))
        .with_text("Got the data");

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(DataTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent.run("Get data").await.unwrap();
    assert_eq!(response, "Got the data");
}

// ===== Error Path Tests =====

#[tokio::test]
async fn test_tool_execution_error() {
    let provider = MockProvider::new()
        .with_tool_use("error_tool", serde_json::json!({"expression": "test"}))
        .with_text("Handled the error");

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(ErrorTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    // Agent should handle tool error gracefully and continue
    let response = agent.run("Test").await.unwrap();
    assert_eq!(response, "Handled the error");
}

#[tokio::test]
async fn test_invalid_tool_input() {
    // Test that invalid JSON input to a tool is handled
    // This tests the deserialization error path in tool.rs
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"wrong_field": "value"}))
        .with_text("Handled invalid input");

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent.run("Test").await.unwrap();
    assert_eq!(response, "Handled invalid input");
}

#[tokio::test]
async fn test_agent_run_error() {
    // Test when provider returns no responses
    let provider = MockProvider::new();
    let agent = Agent::builder().provider(provider).build().await.unwrap();

    let result = agent.run("Test").await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No more responses"));
}

// ===== Comprehensive Event Tests =====

#[tokio::test]
async fn test_event_data_verification() {
    let provider = MockProvider::new().with_text("Test response");
    let collector = DetailedEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder().provider(provider).build().await.unwrap();
    agent.add_hook(collector);

    let user_input = "Test input";
    agent.run(user_input).await.unwrap();

    let events = collector_clone.events();

    // Verify RunStarted event
    let run_started = events.iter().find_map(|e| {
        if let AgentEvent::RunStarted { input, .. } = e {
            Some(input)
        } else {
            None
        }
    });
    assert_eq!(run_started, Some(&user_input.to_string()));

    // Verify RunCompleted event
    let run_completed = events.iter().find_map(|e| {
        if let AgentEvent::RunCompleted { output, duration } = e {
            Some((output, duration))
        } else {
            None
        }
    });
    assert!(run_completed.is_some());
    let (output, duration) = run_completed.unwrap();
    assert_eq!(output, "Test response");
    assert!(duration.as_nanos() > 0); // Should have taken some time (use nanos for faster tests)
}

#[tokio::test]
async fn test_model_call_events() {
    let provider = MockProvider::new().with_text("Response");
    let collector = DetailedEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder().provider(provider).build().await.unwrap();
    agent.add_hook(collector);

    agent.run("Test").await.unwrap();

    let events = collector_clone.events();

    // Verify ModelCallStarted
    let model_started = events.iter().find_map(|e| {
        if let AgentEvent::ModelCallStarted {
            message_count,
            tool_count,
            ..
        } = e
        {
            Some((*message_count, *tool_count))
        } else {
            None
        }
    });
    assert!(model_started.is_some());
    let (msg_count, tool_count) = model_started.unwrap();
    assert_eq!(msg_count, 1); // Just the user message
    assert_eq!(tool_count, 0); // No tools registered

    // Verify ModelCallCompleted
    let model_completed = events.iter().find_map(|e| {
        if let AgentEvent::ModelCallCompleted {
            response_content,
            duration,
            stop_reason,
            ..
        } = e
        {
            Some((response_content, duration, stop_reason))
        } else {
            None
        }
    });
    assert!(model_completed.is_some());
    let (content, duration, stop_reason) = model_completed.unwrap();
    assert_eq!(content, "Response");
    assert!(duration.as_nanos() > 0);
    assert!(stop_reason.is_some());
}

#[tokio::test]
async fn test_tool_event_details() {
    let provider = MockProvider::new()
        .with_tool_use("calculate", serde_json::json!({"expression": "2+2"}))
        .with_text("Done");

    let collector = DetailedEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();
    agent.add_hook(collector);

    agent.run("Calculate").await.unwrap();

    let events = collector_clone.events();

    // Verify ToolRequested event
    let tool_requested = events.iter().find_map(|e| {
        if let AgentEvent::ToolRequested { name, input, .. } = e {
            Some((name, input))
        } else {
            None
        }
    });
    assert!(tool_requested.is_some());
    let (name, input) = tool_requested.unwrap();
    assert_eq!(name, "calculate");
    assert_eq!(input["expression"], "2+2");

    // Verify ToolCompleted event
    let tool_completed = events.iter().find_map(|e| {
        if let AgentEvent::ToolCompleted {
            name,
            output,
            duration,
            ..
        } = e
        {
            Some((name, output, duration))
        } else {
            None
        }
    });
    assert!(tool_completed.is_some());
    let (name, output, duration) = tool_completed.unwrap();
    assert_eq!(name, "calculate");
    assert!(matches!(output, ToolResult::Text(_)));
    assert!(duration.as_nanos() > 0);
}

#[tokio::test]
async fn test_multiple_hooks() {
    let provider = MockProvider::new().with_text("Response");

    let collector1 = EventCollector::new();
    let collector2 = EventCollector::new();
    let clone1 = collector1.clone();
    let clone2 = collector2.clone();

    let agent = Agent::builder().provider(provider).build().await.unwrap();
    agent.add_hook(collector1);
    agent.add_hook(collector2);

    agent.run("Test").await.unwrap();

    // Both hooks should receive all events
    let events1 = clone1.events();
    let events2 = clone2.events();

    assert_eq!(events1.len(), events2.len());
    assert!(events1.len() >= 4);
    assert_eq!(events1, events2);
}

#[tokio::test]
async fn test_tool_not_found_emits_failure() {
    let provider = MockProvider::new()
        .with_tool_use("nonexistent", serde_json::json!({}))
        .with_text("Handled");

    let collector = DetailedEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder().provider(provider).build().await.unwrap();
    agent.add_hook(collector);

    agent.run("Test").await.unwrap();

    let events = collector_clone.events();

    // Should have ToolFailed event for nonexistent tool
    let tool_failed = events.iter().find_map(|e| {
        if let AgentEvent::ToolFailed { name, error, .. } = e {
            Some((name, error))
        } else {
            None
        }
    });

    assert!(tool_failed.is_some());
    let (name, error) = tool_failed.unwrap();
    assert_eq!(name, "nonexistent");
    assert!(error.contains("Tool not found"));
}

// ===== Agent Helper Method Tests =====

#[tokio::test]
async fn test_model_name() {
    let provider = MockProvider::new();
    let agent = Agent::builder().provider(provider).build().await.unwrap();

    assert_eq!(agent.model_name(), "MockProvider");
}

#[tokio::test]
async fn test_list_tools() {
    let provider = MockProvider::new();

    // No tools initially
    let agent_no_tools = Agent::builder()
        .provider(provider.clone())
        .build()
        .await
        .unwrap();
    assert_eq!(agent_no_tools.list_tools().len(), 0);

    // Agent with tools added via builder
    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .add_tool(DataTool)
        .build()
        .await
        .unwrap();

    let tools = agent.list_tools();
    assert_eq!(tools.len(), 2);

    // Verify tool info
    assert_eq!(tools[0].name, "calculate");
    assert_eq!(tools[0].description, "Evaluate a mathematical expression");
    assert_eq!(tools[1].name, "get_data");
    assert_eq!(tools[1].description, "Get structured data");
}
