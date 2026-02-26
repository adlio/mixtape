use std::future::Future;
use std::sync::Arc;

use mixtape_core::Agent;
use tokio::sync::mpsc;

use crate::adapter::MixtapeAcpAgent;
use crate::error::AcpError;
use crate::session::SessionManager;
use crate::types::AgentFactory;

/// The server bundle returned by [`MixtapeAcpBuilder::build`].
///
/// Contains the ACP agent adapter and the receiver halves of the notification
/// and permission channels, which are driven by the relay task in
/// [`serve_stdio`](crate::serve_stdio).
///
/// Use [`agent_name`](Self::agent_name) and [`agent_version`](Self::agent_version)
/// to inspect the configured identity.
pub struct MixtapeAcpServer {
    pub(crate) adapter: MixtapeAcpAgent,
    pub(crate) notification_rx: mpsc::UnboundedReceiver<crate::types::NotificationMessage>,
    pub(crate) permission_rx: mpsc::UnboundedReceiver<crate::permission::PermissionBridgeRequest>,
}

impl MixtapeAcpServer {
    /// The agent name reported to ACP clients during initialization.
    pub fn agent_name(&self) -> &str {
        &self.adapter.name
    }

    /// The agent version reported to ACP clients during initialization.
    pub fn agent_version(&self) -> &str {
        &self.adapter.version
    }
}

/// Builder for configuring and constructing an ACP server.
///
/// # Example
///
/// ```rust,no_run
/// use mixtape_acp::MixtapeAcpBuilder;
/// use mixtape_core::Agent;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let server = MixtapeAcpBuilder::new("my-agent", "0.1.0")
///     .with_agent_factory(|| async {
///         Agent::builder()
///             // .bedrock(...)
///             .build()
///             .await
///     })
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct MixtapeAcpBuilder {
    agent_factory: Option<AgentFactory>,
    name: String,
    version: String,
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;

impl MixtapeAcpBuilder {
    /// Create a new builder with the given agent name and version.
    ///
    /// These are reported to the client in the ACP `initialize` response.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            agent_factory: None,
            name: name.into(),
            version: version.into(),
        }
    }

    /// Set the factory closure that creates new Agent instances.
    ///
    /// This is called once per `new_session` to produce a fresh agent with
    /// its own conversation state.
    pub fn with_agent_factory<F, Fut>(mut self, factory: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = mixtape_core::Result<Agent>> + Send + 'static,
    {
        self.agent_factory = Some(Arc::new(move || Box::pin(factory())));
        self
    }

    /// Build the ACP server, returning the adapter and channel receivers.
    pub fn build(self) -> Result<MixtapeAcpServer, AcpError> {
        let factory = self.agent_factory.ok_or(AcpError::NoAgentFactory)?;

        let (notification_tx, notification_rx) = mpsc::unbounded_channel();
        let (permission_tx, permission_rx) = mpsc::unbounded_channel();

        let adapter = MixtapeAcpAgent {
            factory,
            sessions: SessionManager::new(),
            name: self.name,
            version: self.version,
            notification_tx,
            permission_tx,
        };

        Ok(MixtapeAcpServer {
            adapter,
            notification_rx,
            permission_rx,
        })
    }
}
