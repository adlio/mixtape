use std::sync::Arc;

use agent_client_protocol::{
    AuthenticateRequest, AuthenticateResponse, CancelNotification, ContentBlock as AcpContentBlock,
    Implementation, InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
    PromptRequest, PromptResponse, ProtocolVersion, SessionId, StopReason,
};
use tokio::sync::mpsc;

use crate::convert::{agent_error_to_stop_reason, agent_event_to_session_update};
use crate::error::AcpError;
use crate::permission::PermissionBridgeRequest;
use crate::session::SessionManager;
use crate::types::{AgentFactory, NotificationMessage};

/// The core ACP adapter that bridges mixtape agents to the ACP protocol.
///
/// Implements `agent_client_protocol::Agent` to handle JSON-RPC requests
/// from editors and IDEs. Each ACP session maps to a dedicated mixtape
/// `Agent` instance created by the factory closure.
pub(crate) struct MixtapeAcpAgent {
    pub(crate) factory: AgentFactory,
    pub(crate) sessions: SessionManager,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) notification_tx: mpsc::UnboundedSender<NotificationMessage>,
    pub(crate) permission_tx: mpsc::UnboundedSender<PermissionBridgeRequest>,
}

#[async_trait::async_trait(?Send)]
impl agent_client_protocol::Agent for MixtapeAcpAgent {
    async fn initialize(
        &self,
        _args: InitializeRequest,
    ) -> agent_client_protocol::Result<InitializeResponse> {
        Ok(InitializeResponse::new(ProtocolVersion::V1)
            .agent_info(Implementation::new(&self.name, &self.version)))
    }

    async fn authenticate(
        &self,
        _args: AuthenticateRequest,
    ) -> agent_client_protocol::Result<AuthenticateResponse> {
        Ok(AuthenticateResponse::new())
    }

    async fn new_session(
        &self,
        _args: NewSessionRequest,
    ) -> agent_client_protocol::Result<NewSessionResponse> {
        let agent = (self.factory)()
            .await
            .map_err(|e| AcpError::BuildFailed(e.to_string()))?;

        let session_id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(session_id.clone(), Arc::new(agent));

        Ok(NewSessionResponse::new(SessionId::from(session_id)))
    }

    async fn prompt(&self, args: PromptRequest) -> agent_client_protocol::Result<PromptResponse> {
        let session_id_str = args.session_id.to_string();
        let agent = self
            .sessions
            .get(&session_id_str)
            .ok_or_else(|| AcpError::SessionNotFound(session_id_str))?;

        let text = extract_text_from_prompt(&args.prompt);

        let notification_tx = self.notification_tx.clone();
        let permission_tx = self.permission_tx.clone();
        let session_id = args.session_id.clone();
        let agent_for_hook = Arc::clone(&agent);

        // Hook that forwards agent events to the relay task.
        //
        // Notifications are fire-and-forget. Permission requests carry the
        // Arc<Agent> so the relay task can call agent.respond_to_authorization()
        // after the IDE responds — this is what closes the loop back to the
        // agent's request_authorization() which is blocking on an mpsc::recv().
        let hook_id = agent.add_hook(move |event: &mixtape_core::AgentEvent| {
            if let Some(update) = agent_event_to_session_update(event) {
                let msg = NotificationMessage {
                    session_id: session_id.clone(),
                    update,
                };
                let _ = notification_tx.send(msg);
            }

            if let mixtape_core::AgentEvent::PermissionRequired {
                proposal_id,
                tool_name,
                params,
                ..
            } = event
            {
                let req = PermissionBridgeRequest {
                    session_id: session_id.clone(),
                    proposal_id: proposal_id.clone(),
                    tool_name: tool_name.clone(),
                    params: params.clone(),
                    agent: Arc::clone(&agent_for_hook),
                };
                let _ = permission_tx.send(req);
            }
        });

        // Spawn the agent run on the multi-threaded runtime (Agent::run is Send)
        let agent_clone = Arc::clone(&agent);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let result = agent_clone.run(&text).await;
            let _ = result_tx.send(result);
        });

        let run_result = result_rx
            .await
            .map_err(|_| AcpError::Internal("Agent task dropped".to_string()))?;

        agent.remove_hook(hook_id);

        match run_result {
            Ok(_) => Ok(PromptResponse::new(StopReason::EndTurn)),
            Err(ref e) => agent_error_to_stop_reason(e).map(PromptResponse::new),
        }
    }

    async fn cancel(&self, args: CancelNotification) -> agent_client_protocol::Result<()> {
        let session_id_str = args.session_id.to_string();
        if self.sessions.remove(&session_id_str).is_some() {
            log::info!("Session {} cancelled and removed", session_id_str);
        } else {
            log::warn!("Cancel requested for unknown session {}", session_id_str);
        }
        Ok(())
    }
}

/// Extract text content from ACP prompt content blocks.
fn extract_text_from_prompt(content: &[AcpContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            AcpContentBlock::Text(text_content) => Some(text_content.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    // Bring the ACP Agent trait into scope so its methods are callable on MixtapeAcpAgent.
    // Aliased to avoid collision with mixtape_core::Agent which is also used below.
    use agent_client_protocol::Agent as _;
    use agent_client_protocol::{
        AuthMethodId, AuthenticateRequest, CancelNotification, ContentBlock as AcpContentBlock,
        ImageContent, InitializeRequest, NewSessionRequest, PromptRequest, ProtocolVersion,
        SessionId, StopReason, TextContent,
    };
    use tokio::sync::mpsc;

    use mixtape_core::{
        provider::{ModelProvider, ProviderError},
        types::{ContentBlock, Message, Role, StopReason as CoreStopReason, ToolDefinition},
        ModelResponse,
    };

    use crate::session::SessionManager;
    use crate::types::AgentFactory;

    use super::extract_text_from_prompt;
    use super::MixtapeAcpAgent;

    // ------------------------------------------------------------------
    // Minimal mock provider — same pattern as builder_tests / session_tests
    // ------------------------------------------------------------------

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
                stop_reason: CoreStopReason::EndTurn,
                usage: None,
            })
        }
    }

    fn make_factory() -> AgentFactory {
        Arc::new(|| {
            Box::pin(async {
                mixtape_core::Agent::builder()
                    .provider(MockProvider)
                    .build()
                    .await
            })
        })
    }

    fn make_adapter() -> MixtapeAcpAgent {
        let (notification_tx, _notification_rx) = mpsc::unbounded_channel();
        let (permission_tx, _permission_rx) = mpsc::unbounded_channel();
        MixtapeAcpAgent {
            factory: make_factory(),
            sessions: SessionManager::new(),
            name: "test-agent".to_string(),
            version: "0.0.1".to_string(),
            notification_tx,
            permission_tx,
        }
    }

    // ------------------------------------------------------------------
    // initialize — returns protocol version and agent identity
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn initialize_returns_v1_protocol_version() {
        let adapter = make_adapter();
        let req = InitializeRequest::new(ProtocolVersion::V1);
        let resp = adapter
            .initialize(req)
            .await
            .expect("initialize should succeed");
        assert_eq!(resp.protocol_version, ProtocolVersion::V1);
    }

    #[tokio::test]
    async fn initialize_embeds_agent_name_and_version() {
        let adapter = make_adapter();
        let req = InitializeRequest::new(ProtocolVersion::V1);
        let resp = adapter
            .initialize(req)
            .await
            .expect("initialize should succeed");
        let info = resp.agent_info.expect("agent_info should be set");
        assert_eq!(info.name, "test-agent");
        assert_eq!(info.version, "0.0.1");
    }

    // ------------------------------------------------------------------
    // authenticate — always succeeds (no auth required)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn authenticate_always_succeeds() {
        let adapter = make_adapter();
        let req = AuthenticateRequest::new(AuthMethodId::new("any-method"));
        let result = adapter.authenticate(req).await;
        assert!(result.is_ok(), "authenticate should always succeed");
    }

    // ------------------------------------------------------------------
    // new_session — creates a session and stores the agent
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn new_session_creates_a_session_and_returns_id() {
        let adapter = make_adapter();
        let req = NewSessionRequest::new("/tmp");
        let resp = adapter
            .new_session(req)
            .await
            .expect("new_session should succeed");
        let session_id = resp.session_id.to_string();
        assert!(!session_id.is_empty(), "session_id should not be empty");
        assert!(
            adapter.sessions.get(&session_id).is_some(),
            "session should be stored in the session manager"
        );
    }

    #[tokio::test]
    async fn new_session_multiple_calls_create_distinct_sessions() {
        let adapter = make_adapter();
        let req1 = NewSessionRequest::new("/tmp");
        let req2 = NewSessionRequest::new("/tmp");
        let resp1 = adapter.new_session(req1).await.unwrap();
        let resp2 = adapter.new_session(req2).await.unwrap();
        assert_ne!(
            resp1.session_id.to_string(),
            resp2.session_id.to_string(),
            "each new_session call should produce a unique session ID"
        );
    }

    // ------------------------------------------------------------------
    // cancel — removes existing session, tolerates unknown session
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn cancel_removes_known_session() {
        let adapter = make_adapter();
        let session_resp = adapter
            .new_session(NewSessionRequest::new("/tmp"))
            .await
            .unwrap();
        let session_id = session_resp.session_id.clone();

        assert!(adapter.sessions.get(&session_id.to_string()).is_some());

        let cancel = CancelNotification::new(session_id.clone());
        adapter.cancel(cancel).await.expect("cancel should succeed");

        assert!(
            adapter.sessions.get(&session_id.to_string()).is_none(),
            "session should be removed after cancel"
        );
    }

    #[tokio::test]
    async fn cancel_unknown_session_is_not_an_error() {
        let adapter = make_adapter();
        let cancel = CancelNotification::new(SessionId::from("does-not-exist".to_string()));
        let result = adapter.cancel(cancel).await;
        assert!(
            result.is_ok(),
            "cancel for an unknown session should return Ok"
        );
    }

    // ------------------------------------------------------------------
    // prompt — session-not-found error path
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn prompt_unknown_session_returns_session_not_found_error() {
        let adapter = make_adapter();
        let req = PromptRequest::new(
            SessionId::from("nonexistent-session".to_string()),
            vec![AcpContentBlock::Text(TextContent::new("hello"))],
        );
        let result = adapter.prompt(req).await;
        assert!(
            result.is_err(),
            "prompt for an unknown session should return Err"
        );
    }

    // ------------------------------------------------------------------
    // prompt — success path with mock provider
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn prompt_known_session_returns_end_turn() {
        let adapter = make_adapter();
        let session_resp = adapter
            .new_session(NewSessionRequest::new("/tmp"))
            .await
            .unwrap();
        let session_id = session_resp.session_id.clone();

        let req = PromptRequest::new(
            session_id,
            vec![AcpContentBlock::Text(TextContent::new("say hello"))],
        );
        let resp = adapter
            .prompt(req)
            .await
            .expect("prompt should succeed for a known session");
        assert_eq!(
            resp.stop_reason,
            StopReason::EndTurn,
            "MockProvider returns EndTurn, so stop_reason should be EndTurn"
        );
    }

    fn text_block(s: &str) -> AcpContentBlock {
        AcpContentBlock::Text(TextContent::new(s))
    }

    fn image_block() -> AcpContentBlock {
        AcpContentBlock::Image(ImageContent::new("base64data==", "image/png"))
    }

    #[test]
    fn empty_slice_produces_empty_string() {
        assert_eq!(extract_text_from_prompt(&[]), "");
    }

    #[test]
    fn single_text_block_returns_its_text() {
        let blocks = [text_block("hello world")];
        assert_eq!(extract_text_from_prompt(&blocks), "hello world");
    }

    #[test]
    fn multiple_text_blocks_are_joined_with_newline() {
        let blocks = [
            text_block("first"),
            text_block("second"),
            text_block("third"),
        ];
        assert_eq!(extract_text_from_prompt(&blocks), "first\nsecond\nthird");
    }

    #[test]
    fn non_text_blocks_are_skipped() {
        let blocks = [image_block()];
        assert_eq!(
            extract_text_from_prompt(&blocks),
            "",
            "image blocks should produce no text"
        );
    }

    #[test]
    fn mixed_blocks_extract_only_text() {
        let blocks = [
            text_block("question"),
            image_block(),
            text_block("follow-up"),
        ];
        assert_eq!(
            extract_text_from_prompt(&blocks),
            "question\nfollow-up",
            "only text blocks should contribute to output"
        );
    }

    #[test]
    fn empty_text_block_contributes_empty_segment() {
        let blocks = [text_block(""), text_block("after")];
        assert_eq!(extract_text_from_prompt(&blocks), "\nafter");
    }

    #[test]
    fn all_non_text_blocks_produce_empty_string() {
        let blocks = [image_block(), image_block()];
        assert_eq!(extract_text_from_prompt(&blocks), "");
    }
}
