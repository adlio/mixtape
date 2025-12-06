//! Tool call authorization.

use super::grant::{hash_params, Grant};
use super::store::{GrantStore, GrantStoreError, MemoryGrantStore};
use serde_json::Value;

/// Policy for handling tool calls without matching grants.
///
/// This determines what happens when a tool is called and no grant exists
/// in the store for that tool/parameter combination.
///
/// # Security
///
/// **`AutoDeny` is the default** - tools without grants are denied immediately.
/// This is secure by default for automated environments (scripts, CI/CD, agents).
///
/// Use `Interactive` only when a human is available to approve tool calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolAuthorizationPolicy {
    /// Deny tools without grants immediately (default, secure).
    ///
    /// This is the recommended policy for non-interactive environments
    /// like scripts, CI/CD pipelines, or automated agents.
    #[default]
    AutoDeny,

    /// Prompt the user for authorization via `PermissionRequired` events.
    ///
    /// Use this policy for interactive environments like REPLs or CLIs
    /// where a human can approve or deny tool execution.
    ///
    /// Enable via `AgentBuilder::interactive()` or
    /// [`ToolCallAuthorizer::interactive()`].
    Interactive,
}

/// Authorizes tool calls against stored grants.
///
/// The authorizer wraps a [`GrantStore`] and provides the logic for:
/// - Granting permissions (tool-wide or params-specific)
/// - Checking if a tool call is authorized
/// - Revoking permissions
///
/// # Example
///
/// ```rust
/// use mixtape_core::permission::ToolCallAuthorizer;
///
/// # tokio_test::block_on(async {
/// let auth = ToolCallAuthorizer::new();
///
/// // Grant permission to use a tool
/// auth.grant_tool("echo").await.unwrap();
///
/// // Check if a call is authorized
/// let params = serde_json::json!({"message": "hello"});
/// let result = auth.check("echo", &params).await;
/// assert!(result.is_authorized());
/// # });
/// ```
pub struct ToolCallAuthorizer {
    store: Box<dyn GrantStore>,
    policy: ToolAuthorizationPolicy,
}

impl ToolCallAuthorizer {
    /// Create a new authorizer with an in-memory store and default policy (AutoDeny).
    pub fn new() -> Self {
        Self {
            store: Box::new(MemoryGrantStore::new()),
            policy: ToolAuthorizationPolicy::default(),
        }
    }

    /// Create an authorizer configured for interactive use.
    ///
    /// This sets the policy to [`ToolAuthorizationPolicy::Interactive`], which will
    /// emit `PermissionRequired` events for tools without grants.
    pub fn interactive() -> Self {
        Self::new().with_authorization_policy(ToolAuthorizationPolicy::Interactive)
    }

    /// Create an authorizer with a custom store.
    pub fn with_store(store: impl GrantStore + 'static) -> Self {
        Self {
            store: Box::new(store),
            policy: ToolAuthorizationPolicy::default(),
        }
    }

    /// Create an authorizer with a boxed store.
    pub fn with_boxed_store(store: Box<dyn GrantStore>) -> Self {
        Self {
            store,
            policy: ToolAuthorizationPolicy::default(),
        }
    }

    /// Set the policy for tools without grants.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mixtape_core::permission::{ToolCallAuthorizer, ToolAuthorizationPolicy};
    ///
    /// // For interactive use (prompts user)
    /// let auth = ToolCallAuthorizer::new()
    ///     .with_authorization_policy(ToolAuthorizationPolicy::Interactive);
    ///
    /// // For non-interactive use (denies immediately) - this is the default
    /// let auth = ToolCallAuthorizer::new()
    ///     .with_authorization_policy(ToolAuthorizationPolicy::AutoDeny);
    /// ```
    pub fn with_authorization_policy(mut self, policy: ToolAuthorizationPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Get the current authorization policy.
    pub fn policy(&self) -> ToolAuthorizationPolicy {
        self.policy
    }

    /// Grant permission to use a tool (any parameters).
    pub async fn grant_tool(&self, tool: &str) -> Result<(), GrantStoreError> {
        self.store.save(Grant::tool(tool)).await
    }

    /// Grant permission for specific parameters.
    ///
    /// The params are hashed internally using canonical JSON.
    pub async fn grant_params(&self, tool: &str, params: &Value) -> Result<(), GrantStoreError> {
        let hash = hash_params(params);
        self.store.save(Grant::exact(tool, hash)).await
    }

    /// Grant permission using a pre-computed hash.
    pub async fn grant_params_hash(
        &self,
        tool: &str,
        params_hash: &str,
    ) -> Result<(), GrantStoreError> {
        self.store.save(Grant::exact(tool, params_hash)).await
    }

    /// Check if a tool call is authorized.
    ///
    /// Returns:
    /// - [`Authorization::Granted`] if a matching grant exists
    /// - [`Authorization::Denied`] if no grant and policy is [`ToolAuthorizationPolicy::AutoDeny`]
    /// - [`Authorization::PendingApproval`] if no grant and policy is [`ToolAuthorizationPolicy::Interactive`]
    pub async fn check(&self, tool_name: &str, params: &Value) -> Authorization {
        let params_hash = hash_params(params);

        // Check for existing grant
        match self.store.load(tool_name).await {
            Ok(grants) => {
                for grant in grants {
                    if grant.matches(&params_hash) {
                        return Authorization::Granted { grant };
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to load grants for {}: {}", tool_name, e);
            }
        }

        // No grant found - apply policy
        match self.policy {
            ToolAuthorizationPolicy::AutoDeny => Authorization::Denied {
                reason: format!("No grant configured for tool '{}'", tool_name),
            },
            ToolAuthorizationPolicy::Interactive => Authorization::PendingApproval { params_hash },
        }
    }

    /// Revoke a grant.
    ///
    /// Pass `None` for params_hash to revoke a tool-wide grant.
    pub async fn revoke(
        &self,
        tool: &str,
        params_hash: Option<&str>,
    ) -> Result<bool, GrantStoreError> {
        self.store.delete(tool, params_hash).await
    }

    /// Get all stored grants.
    pub async fn grants(&self) -> Result<Vec<Grant>, GrantStoreError> {
        self.store.load_all().await
    }

    /// Clear all grants.
    pub async fn clear(&self) -> Result<(), GrantStoreError> {
        self.store.clear().await
    }
}

impl Default for ToolCallAuthorizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of an authorization check.
#[derive(Debug, Clone)]
pub enum Authorization {
    /// The call is authorized by an existing grant.
    Granted {
        /// The grant that authorized this call.
        grant: Grant,
    },
    /// The call is denied (no grant and policy is AutoDeny).
    Denied {
        /// Reason for denial.
        reason: String,
    },
    /// Authorization is pending user approval (no grant and policy is Interactive).
    PendingApproval {
        /// Hash of the parameters (for creating exact-match grants).
        params_hash: String,
    },
}

impl Authorization {
    /// Check if the call is authorized.
    pub fn is_authorized(&self) -> bool {
        matches!(self, Authorization::Granted { .. })
    }

    /// Check if the call is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, Authorization::Denied { .. })
    }

    /// Check if authorization is pending user approval.
    pub fn is_pending(&self) -> bool {
        matches!(self, Authorization::PendingApproval { .. })
    }
}

/// User's response to an authorization request.
#[derive(Debug, Clone)]
pub enum AuthorizationResponse {
    /// Allow this call once, don't save a grant.
    Once,

    /// Allow and save a grant.
    Trust {
        /// The grant to store.
        grant: Grant,
    },

    /// Deny the call.
    Deny {
        /// Optional reason for denial.
        reason: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Policy Tests =====

    #[test]
    fn test_default_policy_is_auto_deny() {
        let auth = ToolCallAuthorizer::new();
        assert_eq!(auth.policy(), ToolAuthorizationPolicy::AutoDeny);
    }

    #[test]
    fn test_interactive_constructor_sets_interactive_policy() {
        let auth = ToolCallAuthorizer::interactive();
        assert_eq!(auth.policy(), ToolAuthorizationPolicy::Interactive);
    }

    #[test]
    fn test_with_authorization_policy() {
        let auth = ToolCallAuthorizer::new()
            .with_authorization_policy(ToolAuthorizationPolicy::Interactive);
        assert_eq!(auth.policy(), ToolAuthorizationPolicy::Interactive);
    }

    #[tokio::test]
    async fn test_auto_deny_policy_returns_denied() {
        let auth = ToolCallAuthorizer::new(); // Default is AutoDeny

        let params = serde_json::json!({"key": "value"});
        let result = auth.check("test", &params).await;

        assert!(result.is_denied());
        assert!(!result.is_authorized());
        assert!(!result.is_pending());
    }

    #[tokio::test]
    async fn test_interactive_policy_returns_pending() {
        let auth = ToolCallAuthorizer::interactive(); // Interactive policy

        let params = serde_json::json!({"key": "value"});
        let result = auth.check("test", &params).await;

        assert!(result.is_pending());
        assert!(!result.is_authorized());
        assert!(!result.is_denied());
    }

    #[tokio::test]
    async fn test_grant_overrides_policy() {
        // Even with Deny policy, a grant should authorize
        let auth = ToolCallAuthorizer::new();
        auth.grant_tool("test").await.unwrap();

        let result = auth.check("test", &serde_json::json!({})).await;
        assert!(result.is_authorized());
    }

    // ===== Grant Tests =====

    #[tokio::test]
    async fn test_authorizer_tool_wide_grant() {
        let auth = ToolCallAuthorizer::new();
        auth.grant_tool("test").await.unwrap();

        // Any params should be authorized
        let result = auth.check("test", &serde_json::json!({"a": 1})).await;
        assert!(result.is_authorized());

        let result = auth.check("test", &serde_json::json!({"b": 2})).await;
        assert!(result.is_authorized());
    }

    #[tokio::test]
    async fn test_authorizer_params_grant() {
        let auth = ToolCallAuthorizer::new();

        let params = serde_json::json!({"key": "value"});
        auth.grant_params("test", &params).await.unwrap();

        // Exact params should be authorized
        let result = auth.check("test", &params).await;
        assert!(result.is_authorized());

        // Different params should be denied (default policy)
        let other = serde_json::json!({"key": "other"});
        let result = auth.check("test", &other).await;
        assert!(result.is_denied());
    }

    #[tokio::test]
    async fn test_authorizer_wrong_tool() {
        let auth = ToolCallAuthorizer::new();
        auth.grant_tool("tool_a").await.unwrap();

        let result = auth.check("tool_b", &serde_json::json!({})).await;
        assert!(result.is_denied());
    }

    #[tokio::test]
    async fn test_authorizer_revoke() {
        let auth = ToolCallAuthorizer::new();
        auth.grant_tool("test").await.unwrap();

        assert!(auth
            .check("test", &serde_json::json!({}))
            .await
            .is_authorized());

        auth.revoke("test", None).await.unwrap();

        assert!(auth.check("test", &serde_json::json!({})).await.is_denied());
    }

    #[tokio::test]
    async fn test_authorizer_grants() {
        let auth = ToolCallAuthorizer::new();
        auth.grant_tool("a").await.unwrap();
        auth.grant_tool("b").await.unwrap();

        let grants = auth.grants().await.unwrap();
        assert_eq!(grants.len(), 2);
    }

    #[tokio::test]
    async fn test_authorizer_clear() {
        let auth = ToolCallAuthorizer::new();
        auth.grant_tool("test").await.unwrap();

        auth.clear().await.unwrap();

        assert!(auth.grants().await.unwrap().is_empty());
    }

    // ===== Authorization Enum Tests =====

    #[test]
    fn test_authorization_methods() {
        let granted = Authorization::Granted {
            grant: Grant::tool("test"),
        };
        assert!(granted.is_authorized());
        assert!(!granted.is_denied());
        assert!(!granted.is_pending());

        let denied = Authorization::Denied {
            reason: "test".to_string(),
        };
        assert!(!denied.is_authorized());
        assert!(denied.is_denied());
        assert!(!denied.is_pending());

        let pending = Authorization::PendingApproval {
            params_hash: "abc".to_string(),
        };
        assert!(!pending.is_authorized());
        assert!(!pending.is_denied());
        assert!(pending.is_pending());
    }
}
