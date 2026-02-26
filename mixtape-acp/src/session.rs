use std::collections::HashMap;
use std::sync::Arc;

use mixtape_core::Agent;
use parking_lot::RwLock;

/// Manages the mapping from ACP session IDs to mixtape Agent instances.
///
/// Each ACP session gets its own Agent instance since agents maintain
/// internal conversation state (conversation manager, session store).
#[derive(Default)]
pub(crate) struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<Agent>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new agent for the given session ID.
    pub fn insert(&self, session_id: String, agent: Arc<Agent>) {
        self.sessions.write().insert(session_id, agent);
    }

    /// Get the agent for a session, if it exists.
    pub fn get(&self, session_id: &str) -> Option<Arc<Agent>> {
        self.sessions.read().get(session_id).cloned()
    }

    /// Remove a session and return its agent.
    pub fn remove(&self, session_id: &str) -> Option<Arc<Agent>> {
        self.sessions.write().remove(session_id)
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
