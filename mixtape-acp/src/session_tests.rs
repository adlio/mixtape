use std::sync::Arc;

use mixtape_core::{
    provider::{ModelProvider, ProviderError},
    types::{ContentBlock, Message, Role, StopReason, ToolDefinition},
    ModelResponse,
};

use super::SessionManager;

// A minimal mock provider so we can build real Agent instances without
// making any network calls.  This mirrors the pattern used in
// mixtape-core's own builder tests.
#[derive(Clone)]
struct MockProvider;

#[async_trait::async_trait]
impl ModelProvider for MockProvider {
    fn name(&self) -> &str {
        "MockProvider"
    }

    fn max_context_tokens(&self) -> usize {
        200_000
    }

    fn max_output_tokens(&self) -> usize {
        8_192
    }

    async fn generate(
        &self,
        _messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
        _system_prompt: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        Ok(ModelResponse {
            message: Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text("ok".to_string())],
            },
            stop_reason: StopReason::EndTurn,
            usage: None,
        })
    }
}

async fn make_agent() -> Arc<mixtape_core::Agent> {
    Arc::new(
        mixtape_core::Agent::builder()
            .provider(MockProvider)
            .build()
            .await
            .expect("MockProvider build should never fail"),
    )
}

// ---------------------------------------------------------------------------
// SessionManager::new / Default
// ---------------------------------------------------------------------------

#[test]
fn new_session_manager_starts_empty() {
    let mgr = SessionManager::new();
    assert!(mgr.get("any-session").is_none());
}

#[test]
fn default_session_manager_starts_empty() {
    let mgr = SessionManager::default();
    assert!(mgr.get("any-session").is_none());
}

// ---------------------------------------------------------------------------
// SessionManager::insert / get
// ---------------------------------------------------------------------------

#[tokio::test]
async fn insert_then_get_returns_agent() {
    let mgr = SessionManager::new();
    let agent = make_agent().await;

    mgr.insert("sess-1".to_string(), Arc::clone(&agent));

    let retrieved = mgr.get("sess-1");
    assert!(
        retrieved.is_some(),
        "should find the agent we just inserted"
    );
    assert!(
        Arc::ptr_eq(&agent, &retrieved.unwrap()),
        "retrieved agent should be the same Arc as the inserted one"
    );
}

#[test]
fn get_missing_key_returns_none() {
    let mgr = SessionManager::new();
    assert!(mgr.get("does-not-exist").is_none());
}

#[tokio::test]
async fn get_after_overwrite_returns_latest_agent() {
    let mgr = SessionManager::new();
    let first = make_agent().await;
    let second = make_agent().await;

    mgr.insert("sess".to_string(), Arc::clone(&first));
    mgr.insert("sess".to_string(), Arc::clone(&second));

    let retrieved = mgr.get("sess").expect("session must exist");
    assert!(
        Arc::ptr_eq(&second, &retrieved),
        "overwritten session should return the second agent"
    );
}

// ---------------------------------------------------------------------------
// SessionManager::remove
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remove_existing_session_returns_agent() {
    let mgr = SessionManager::new();
    let agent = make_agent().await;

    mgr.insert("sess-rm".to_string(), Arc::clone(&agent));
    let removed = mgr.remove("sess-rm");

    assert!(removed.is_some(), "remove should return the agent");
    assert!(
        Arc::ptr_eq(&agent, &removed.unwrap()),
        "removed agent should match what was inserted"
    );
}

#[test]
fn remove_missing_session_returns_none() {
    let mgr = SessionManager::new();
    assert!(mgr.remove("never-existed").is_none());
}

#[tokio::test]
async fn remove_makes_session_inaccessible() {
    let mgr = SessionManager::new();
    let agent = make_agent().await;

    mgr.insert("sess-del".to_string(), agent);
    mgr.remove("sess-del");

    assert!(
        mgr.get("sess-del").is_none(),
        "session should not be accessible after removal"
    );
}

// ---------------------------------------------------------------------------
// Multiple sessions coexist independently
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multiple_sessions_coexist() {
    let mgr = SessionManager::new();
    let a = make_agent().await;
    let b = make_agent().await;

    mgr.insert("alpha".to_string(), Arc::clone(&a));
    mgr.insert("beta".to_string(), Arc::clone(&b));

    assert!(Arc::ptr_eq(&a, &mgr.get("alpha").unwrap()));
    assert!(Arc::ptr_eq(&b, &mgr.get("beta").unwrap()));

    // Removing one does not disturb the other
    mgr.remove("alpha");
    assert!(mgr.get("alpha").is_none());
    assert!(mgr.get("beta").is_some());
}
