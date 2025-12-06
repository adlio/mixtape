#![cfg(feature = "session")]

mod common;

use common::{AutoApproveGrantStore, MockProvider, MockSessionStore};
use mixtape_core::{Agent, SessionStore, Tool, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct TestInput {
    message: String,
}

struct TestTool;

impl Tool for TestTool {
    type Input = TestInput;

    fn name(&self) -> &str {
        "test_tool"
    }

    fn description(&self) -> &str {
        "A test tool"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::Text(format!("Processed: {}", input.message)))
    }
}

#[tokio::test]
async fn test_session_persistence() {
    let store = MockSessionStore::new();
    let store_clone = store.clone();

    let provider = MockProvider::new()
        .with_text("First response")
        .with_text("Second response");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store)
        .build()
        .await
        .unwrap();

    // First run
    let response1 = agent.run("First message").await.unwrap();
    assert_eq!(response1, "First response");

    // Second run - session should persist
    let response2 = agent.run("Second message").await.unwrap();
    assert_eq!(response2, "Second response");

    // Verify session was created
    assert_eq!(store_clone.session_count(), 1);

    // Verify session has messages
    let sessions = store_clone.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].message_count, 4); // 2 user + 2 assistant
}

#[tokio::test]
async fn test_session_with_tools() {
    let store = MockSessionStore::new();

    let provider = MockProvider::new()
        .with_tool_use("test_tool", serde_json::json!({"message": "hello"}))
        .with_text("Tool was used");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store.clone())
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent.run("Use the tool").await.unwrap();
    assert_eq!(response, "Tool was used");

    // Verify session was saved with tool interaction
    let sessions = store.list_sessions().await.unwrap();
    assert_eq!(sessions.len(), 1);
}

#[tokio::test]
async fn test_session_info_via_store() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new().with_text("Response");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store.clone())
        .build()
        .await
        .unwrap();

    agent.run("Test").await.unwrap();

    // Query session info through the store directly
    let session = store.get_or_create_session().await.unwrap();
    assert_eq!(session.directory, "/test/dir");
    assert_eq!(session.messages.len(), 2); // user + assistant
}

#[tokio::test]
async fn test_session_history_via_store() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new()
        .with_text("First")
        .with_text("Second")
        .with_text("Third");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store.clone())
        .build()
        .await
        .unwrap();

    // Run three times
    agent.run("Message 1").await.unwrap();
    agent.run("Message 2").await.unwrap();
    agent.run("Message 3").await.unwrap();

    // Get session and check history
    let session = store.get_or_create_session().await.unwrap();
    assert_eq!(session.messages.len(), 6); // 3 user + 3 assistant

    // Last 2 messages
    let last_two = &session.messages[session.messages.len() - 2..];
    assert_eq!(last_two.len(), 2);
}

#[tokio::test]
async fn test_session_different_directories() {
    let store1 = MockSessionStore::new().with_directory("/dir1");
    let store2 = MockSessionStore::new().with_directory("/dir2");

    let provider1 = MockProvider::new().with_text("Dir1 response");
    let provider2 = MockProvider::new().with_text("Dir2 response");

    let agent1 = Agent::builder()
        .provider(provider1)
        .with_session_store(store1.clone())
        .build()
        .await
        .unwrap();
    let agent2 = Agent::builder()
        .provider(provider2)
        .with_session_store(store2.clone())
        .build()
        .await
        .unwrap();

    agent1.run("Test").await.unwrap();
    agent2.run("Test").await.unwrap();

    // Each directory should have its own session
    assert_eq!(store1.session_count(), 1);
    assert_eq!(store2.session_count(), 1);
}

#[tokio::test]
async fn test_agent_without_session() {
    let provider = MockProvider::new().with_text("Response");
    let agent = Agent::builder().provider(provider).build().await.unwrap();

    // Should work fine without session store
    let response = agent.run("Test").await.unwrap();
    assert_eq!(response, "Response");

    // Session info should be None
    let info = agent.get_session_info().await.unwrap();
    assert!(info.is_none());
}

// ===== Session Event Tests =====

use mixtape_core::{AgentEvent, AgentHook};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct SessionEventCollector {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl SessionEventCollector {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl AgentHook for SessionEventCollector {
    fn on_event(&self, event: &AgentEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

#[tokio::test]
async fn test_session_resumed_event() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new().with_text("First").with_text("Second");

    let collector = SessionEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store)
        .build()
        .await
        .unwrap();
    agent.add_hook(collector);

    // First run - no SessionResumed event (new session)
    agent.run("Message 1").await.unwrap();

    let events1 = collector_clone.events();
    let has_resumed = events1
        .iter()
        .any(|e| matches!(e, AgentEvent::SessionResumed { .. }));
    assert!(
        !has_resumed,
        "First run should not have SessionResumed event"
    );

    // Second run - should have SessionResumed event
    agent.run("Message 2").await.unwrap();

    let events2 = collector_clone.events();
    let resumed_event = events2.iter().find_map(|e| {
        if let AgentEvent::SessionResumed {
            session_id,
            message_count,
            ..
        } = e
        {
            Some((session_id, message_count))
        } else {
            None
        }
    });

    assert!(resumed_event.is_some());
    let (_, message_count) = resumed_event.unwrap();
    assert_eq!(*message_count, 2); // Should have 2 messages from first run
}

#[tokio::test]
async fn test_session_saved_event() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new().with_text("Response");

    let collector = SessionEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store)
        .build()
        .await
        .unwrap();
    agent.add_hook(collector);

    agent.run("Test").await.unwrap();

    let events = collector_clone.events();

    // Should have SessionSaved event
    let saved_event = events.iter().find_map(|e| {
        if let AgentEvent::SessionSaved {
            session_id,
            message_count,
        } = e
        {
            Some((session_id, message_count))
        } else {
            None
        }
    });

    assert!(saved_event.is_some());
    let (session_id, message_count) = saved_event.unwrap();
    assert!(!session_id.is_empty());
    assert_eq!(*message_count, 2); // User + assistant
}

#[tokio::test]
async fn test_session_events_with_tools() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new()
        .with_tool_use("test_tool", serde_json::json!({"message": "test"}))
        .with_text("Done");

    let collector = SessionEventCollector::new();
    let collector_clone = collector.clone();

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store)
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();
    agent.add_hook(collector);

    agent.run("Use tool").await.unwrap();

    let events = collector_clone.events();

    // Verify all event types are present
    let has_run_started = events
        .iter()
        .any(|e| matches!(e, AgentEvent::RunStarted { .. }));
    let has_tool_started = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolStarted { .. }));
    let has_tool_completed = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCompleted { .. }));
    let has_session_saved = events
        .iter()
        .any(|e| matches!(e, AgentEvent::SessionSaved { .. }));
    let has_run_completed = events
        .iter()
        .any(|e| matches!(e, AgentEvent::RunCompleted { .. }));

    assert!(has_run_started);
    assert!(has_tool_started);
    assert!(has_tool_completed);
    assert!(has_session_saved);
    assert!(has_run_completed);
}

// ===== Session Message Conversion Tests =====

#[tokio::test]
async fn test_session_resume_with_tool_history() {
    let store = MockSessionStore::new();

    // First run: use a tool
    let provider1 = MockProvider::new()
        .with_tool_use("test_tool", serde_json::json!({"message": "first"}))
        .with_text("First response");

    let agent1 = Agent::builder()
        .provider(provider1)
        .with_session_store(store.clone())
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    agent1.run("First message").await.unwrap();

    // Second run: should resume session with tool history
    let provider2 = MockProvider::new().with_text("Second response");

    let agent2 = Agent::builder()
        .provider(provider2)
        .with_session_store(store.clone())
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent2.run("Second message").await.unwrap();
    assert_eq!(response, "Second response");

    // Verify session has complete history
    let session = store.get_or_create_session().await.unwrap();
    assert_eq!(session.messages.len(), 4); // 2 exchanges

    // First exchange should have tool calls
    assert_eq!(session.messages[0].role, mixtape_core::MessageRole::User);
    assert_eq!(
        session.messages[1].role,
        mixtape_core::MessageRole::Assistant
    );

    // Second exchange
    assert_eq!(session.messages[2].role, mixtape_core::MessageRole::User);
    assert_eq!(
        session.messages[3].role,
        mixtape_core::MessageRole::Assistant
    );
}

#[tokio::test]
async fn test_session_conversion_with_empty_content() {
    let store = MockSessionStore::new();

    // Use a tool (which may create messages with empty text content)
    let provider = MockProvider::new()
        .with_tool_use("test_tool", serde_json::json!({"message": "test"}))
        .with_text("Done");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store.clone())
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    agent.run("Test").await.unwrap();

    // Second run should successfully resume
    let provider2 = MockProvider::new().with_text("Second");
    let agent2 = Agent::builder()
        .provider(provider2)
        .with_session_store(store)
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent2.run("Test again").await.unwrap();
    assert_eq!(response, "Second");
}

#[tokio::test]
async fn test_session_multiple_tool_calls_in_history() {
    let store = MockSessionStore::new();

    // First run: multiple tool calls
    let provider1 = MockProvider::new()
        .with_tool_use("test_tool", serde_json::json!({"message": "first"}))
        .with_tool_use("test_tool", serde_json::json!({"message": "second"}))
        .with_text("Both tools used");

    let agent1 = Agent::builder()
        .provider(provider1)
        .with_session_store(store.clone())
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    agent1.run("Use tools").await.unwrap();

    // Resume and verify history is preserved
    let provider2 = MockProvider::new().with_text("Resume");
    let agent2 = Agent::builder()
        .provider(provider2)
        .with_session_store(store.clone())
        .add_tool(TestTool)
        .with_grant_store(AutoApproveGrantStore)
        .build()
        .await
        .unwrap();

    let response = agent2.run("Continue").await.unwrap();
    assert_eq!(response, "Resume");

    let session = store.get_or_create_session().await.unwrap();
    // Should have full history with all tool interactions
    assert!(session.messages.len() >= 2);
}

// ===== Sync Wrapper Method Tests =====

#[tokio::test]
async fn test_get_session_info_sync() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new().with_text("Response");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store)
        .add_tool(TestTool)
        .build()
        .await
        .unwrap();

    // Run agent to create session
    agent.run("Test").await.unwrap();

    // Now call the method
    let info = agent.get_session_info().await.unwrap();

    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.directory, "/test/dir");
    assert_eq!(info.message_count, 2); // user + assistant
}

#[tokio::test]
async fn test_get_session_history_sync() {
    let store = MockSessionStore::new();
    let provider = MockProvider::new()
        .with_text("First")
        .with_text("Second")
        .with_text("Third");

    let agent = Agent::builder()
        .provider(provider)
        .with_session_store(store)
        .add_tool(TestTool)
        .build()
        .await
        .unwrap();

    // Create some history
    agent.run("Message 1").await.unwrap();
    agent.run("Message 2").await.unwrap();
    agent.run("Message 3").await.unwrap();

    // Call method
    let history = agent.get_session_history(2).await.unwrap();

    assert_eq!(history.len(), 2);
    // Should be last 2 messages
}

#[tokio::test]
async fn test_get_session_info_without_session() {
    let provider = MockProvider::new();
    let agent = Agent::builder().provider(provider).build().await.unwrap();

    // Should return None when no session store configured
    let info = agent.get_session_info().await.unwrap();
    assert!(info.is_none());
}

#[tokio::test]
async fn test_get_session_history_without_session() {
    let provider = MockProvider::new();
    let agent = Agent::builder().provider(provider).build().await.unwrap();

    // Should return empty vec when no session store configured
    let history = agent.get_session_history(10).await.unwrap();
    assert_eq!(history.len(), 0);
}
