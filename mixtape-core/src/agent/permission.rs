//! Authorization Handling for Tool Execution
//!
//! This module provides the authorization system for controlling tool execution.
//!
//! ## Default Behavior
//!
//! By default, tools without grants are **denied immediately**. This is secure
//! for non-interactive environments (scripts, CI/CD, automated agents).
//!
//! ## Interactive Mode
//!
//! For REPLs and CLIs where a human can approve tools, use `.interactive()`:
//!
//! ```ignore
//! use mixtape_core::Agent;
//!
//! // Interactive mode: prompts user for tools without grants
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4_5)
//!     .interactive()  // Enable permission prompts
//!     .build()
//!     .await?;
//! ```
//!
//! ## Pre-Granting Tools
//!
//! For both interactive and non-interactive use, you can pre-grant tools:
//!
//! ```ignore
//! use mixtape_core::{Agent, MemoryGrantStore};
//!
//! // Pre-grant some tools
//! let store = MemoryGrantStore::new();
//! store.grant_tool("echo").await?;
//! store.grant_tool("read_file").await?;
//!
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4_5)
//!     .with_grant_store(store)
//!     .build()
//!     .await?;
//! ```
//!
//! ## Handling Permission Events
//!
//! In interactive mode, respond to [`AgentEvent::PermissionRequired`] using:
//! - [`Agent::respond_to_authorization()`] - Full control with [`AuthorizationResponse`]
//! - [`Agent::authorize_once()`] - One-time authorization
//! - [`Agent::deny_authorization()`] - Deny the request

use std::time::Duration;

use super::builder::AgentBuilder;
use super::types::PermissionError;
use super::Agent;
use crate::permission::{
    AuthorizationResponse, Grant, GrantStore, Scope, ToolAuthorizationPolicy, ToolCallAuthorizer,
};

impl Agent {
    /// Get the authorizer to grant/revoke permissions.
    pub fn authorizer(&self) -> &tokio::sync::RwLock<ToolCallAuthorizer> {
        &self.authorizer
    }

    /// Respond to an authorization request with a choice.
    ///
    /// Use this to respond to [`crate::AgentEvent::PermissionRequired`] events.
    pub async fn respond_to_authorization(
        &self,
        proposal_id: &str,
        response: AuthorizationResponse,
    ) -> Result<(), PermissionError> {
        let pending = self.pending_authorizations.read().await;

        if let Some(tx) = pending.get(proposal_id) {
            tx.send(response)
                .await
                .map_err(|_| PermissionError::ChannelClosed)?;
            Ok(())
        } else {
            Err(PermissionError::RequestNotFound(proposal_id.to_string()))
        }
    }

    /// Grant permission to trust this tool entirely.
    ///
    /// This saves a tool-wide grant that will auto-authorize all future calls.
    pub async fn grant_tool_permission(
        &self,
        proposal_id: &str,
        tool_name: &str,
        scope: Scope,
    ) -> Result<(), PermissionError> {
        let grant = Grant::tool(tool_name).with_scope(scope);
        self.respond_to_authorization(proposal_id, AuthorizationResponse::Trust { grant })
            .await
    }

    /// Grant permission for this exact call.
    ///
    /// This saves an exact-match grant for the specific parameters.
    pub async fn grant_params_permission(
        &self,
        proposal_id: &str,
        tool_name: &str,
        params_hash: &str,
        scope: Scope,
    ) -> Result<(), PermissionError> {
        let grant = Grant::exact(tool_name, params_hash).with_scope(scope);
        self.respond_to_authorization(proposal_id, AuthorizationResponse::Trust { grant })
            .await
    }

    /// Authorize a request once (don't save).
    pub async fn authorize_once(&self, proposal_id: &str) -> Result<(), PermissionError> {
        self.respond_to_authorization(proposal_id, AuthorizationResponse::Once)
            .await
    }

    /// Deny an authorization request.
    pub async fn deny_authorization(
        &self,
        proposal_id: &str,
        reason: Option<String>,
    ) -> Result<(), PermissionError> {
        self.respond_to_authorization(proposal_id, AuthorizationResponse::Deny { reason })
            .await
    }
}

impl AgentBuilder {
    /// Set a custom grant store for tool authorization.
    ///
    /// By default, the agent uses an in-memory store. Use this to provide
    /// a persistent store (e.g., [`crate::FileGrantStore`]).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::{Agent, FileGrantStore};
    ///
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .with_grant_store(FileGrantStore::new("./grants.json"))
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_grant_store(mut self, store: impl GrantStore + 'static) -> Self {
        self.grant_store = Some(Box::new(store));
        self
    }

    /// Set the timeout for authorization requests.
    ///
    /// If an authorization request is not responded to within this duration,
    /// it will be automatically denied.
    ///
    /// Default: 5 minutes
    pub fn with_authorization_timeout(mut self, timeout: Duration) -> Self {
        self.authorization_timeout = timeout;
        self
    }

    /// Enable interactive authorization prompts.
    ///
    /// By default, tools without grants are **denied immediately**. This is
    /// secure for non-interactive environments (scripts, CI/CD, automated agents).
    ///
    /// Call `.interactive()` to enable prompting users for authorization when
    /// a tool has no matching grant. The agent will emit [`crate::AgentEvent::PermissionRequired`]
    /// events that can be handled to approve or deny tool calls.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mixtape_core::Agent;
    ///
    /// // Interactive mode: prompts user for tools without grants
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .interactive()
    ///     .build()
    ///     .await?;
    /// ```
    ///
    /// This can be combined with a grant store for hybrid behavior:
    ///
    /// ```ignore
    /// use mixtape_core::{Agent, FileGrantStore};
    ///
    /// // Pre-approved tools run immediately, others prompt user
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .interactive()
    ///     .with_grant_store(FileGrantStore::new("./grants.json"))
    ///     .build()
    ///     .await?;
    /// ```
    pub fn interactive(mut self) -> Self {
        self.authorization_policy = ToolAuthorizationPolicy::Interactive;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_authorization_timeout() {
        let timeout = Duration::from_secs(60);
        let builder = Agent::builder().with_authorization_timeout(timeout);
        assert_eq!(builder.authorization_timeout, timeout);
    }

    #[test]
    fn test_builder_grant_store() {
        use crate::permission::MemoryGrantStore;
        let builder = Agent::builder().with_grant_store(MemoryGrantStore::new());
        assert!(builder.grant_store.is_some());
    }
}
