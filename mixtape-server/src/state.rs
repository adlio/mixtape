//! Application state for the mixtape server.

use std::sync::Arc;

use mixtape_core::Agent;

/// Shared application state containing the agent.
///
/// This state is cloned for each request handler and provides
/// access to the shared agent instance.
#[derive(Clone)]
pub struct AppState {
    /// The shared agent instance.
    pub agent: Arc<Agent>,
}

impl AppState {
    /// Create new application state from an Arc<Agent>.
    pub fn from_arc(agent: Arc<Agent>) -> Self {
        Self { agent }
    }
}
