use mixtape_core::{
    provider::{ModelProvider, ProviderError},
    types::{ContentBlock, Message, Role, StopReason, ToolDefinition},
    ModelResponse,
};

use crate::error::AcpError;

use super::MixtapeAcpBuilder;

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

// ---------------------------------------------------------------------------
// MixtapeAcpBuilder::new
// ---------------------------------------------------------------------------

#[test]
fn new_stores_name_and_version() {
    let server = MixtapeAcpBuilder::new("my-agent", "1.2.3")
        .with_agent_factory(|| async {
            mixtape_core::Agent::builder()
                .provider(MockProvider)
                .build()
                .await
        })
        .build()
        .expect("build with a factory should succeed");

    assert_eq!(server.agent_name(), "my-agent");
    assert_eq!(server.agent_version(), "1.2.3");
}

#[test]
fn new_accepts_string_owned() {
    let name = "owned-name".to_string();
    let version = "9.9.9".to_string();

    let server = MixtapeAcpBuilder::new(name, version)
        .with_agent_factory(|| async {
            mixtape_core::Agent::builder()
                .provider(MockProvider)
                .build()
                .await
        })
        .build()
        .expect("build should succeed");

    assert_eq!(server.agent_name(), "owned-name");
    assert_eq!(server.agent_version(), "9.9.9");
}

// ---------------------------------------------------------------------------
// MixtapeAcpBuilder::build — error: no factory
// ---------------------------------------------------------------------------

#[test]
fn build_without_factory_returns_no_agent_factory_error() {
    let result = MixtapeAcpBuilder::new("agent", "0.1.0").build();

    match result {
        Err(AcpError::NoAgentFactory) => {}
        Err(other) => panic!("expected NoAgentFactory, got different error: {}", other),
        Ok(_) => panic!("expected Err(AcpError::NoAgentFactory), got Ok"),
    }
}

// ---------------------------------------------------------------------------
// MixtapeAcpBuilder::build — success path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn build_with_factory_succeeds_and_factory_is_callable() {
    let server = MixtapeAcpBuilder::new("test-agent", "0.0.1")
        .with_agent_factory(|| async {
            mixtape_core::Agent::builder()
                .provider(MockProvider)
                .build()
                .await
        })
        .build()
        .expect("build should succeed when factory is provided");

    let agent_result = (server.adapter.factory)().await;
    assert!(
        agent_result.is_ok(),
        "factory should produce an agent without error"
    );
}

#[tokio::test]
async fn build_creates_separate_notification_and_permission_channels() {
    let mut server = MixtapeAcpBuilder::new("ch-agent", "0.1.0")
        .with_agent_factory(|| async {
            mixtape_core::Agent::builder()
                .provider(MockProvider)
                .build()
                .await
        })
        .build()
        .expect("build should succeed");

    assert!(server.notification_rx.try_recv().is_err());
    assert!(server.permission_rx.try_recv().is_err());
}

// ---------------------------------------------------------------------------
// Factory reuse
// ---------------------------------------------------------------------------

#[tokio::test]
async fn factory_can_be_called_multiple_times() {
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let call_count_clone = std::sync::Arc::clone(&call_count);

    let server = MixtapeAcpBuilder::new("multi-call", "0.0.1")
        .with_agent_factory(move || {
            let counter = std::sync::Arc::clone(&call_count_clone);
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                mixtape_core::Agent::builder()
                    .provider(MockProvider)
                    .build()
                    .await
            }
        })
        .build()
        .expect("build should succeed");

    assert_eq!(
        call_count.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "factory should not be called during builder construction"
    );

    (server.adapter.factory)().await.unwrap();
    (server.adapter.factory)().await.unwrap();

    assert_eq!(
        call_count.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "factory should be called once per invocation"
    );
}
