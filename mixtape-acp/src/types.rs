use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mixtape_core::Agent;

/// Type-erased async factory that produces new Agent instances.
///
/// Called once per ACP `new_session` to create a fresh agent with its own
/// conversation state.
pub(crate) type AgentFactory = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = mixtape_core::Result<Agent>> + Send>> + Send + Sync,
>;

/// A notification message sent from agent hooks to the relay task.
///
/// Contains a session update to forward to the IDE via
/// `conn.session_notification()`.
pub(crate) struct NotificationMessage {
    pub session_id: agent_client_protocol::SessionId,
    pub update: agent_client_protocol::SessionUpdate,
}
