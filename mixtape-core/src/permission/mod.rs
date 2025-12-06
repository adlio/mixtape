//! Permission system for tool execution.
//!
//! This module provides authorization for tool calls. Users can grant
//! permission for tool invocations either for the entire tool or for
//! specific parameter combinations.
//!
//! # Overview
//!
//! - **[`ToolCallAuthorizer`]**: Checks grants and authorizes tool calls
//! - **[`ToolAuthorizationPolicy`]**: What to do when no grant exists (AutoDeny or Interactive)
//! - **[`Grant`]**: A stored permission (tool-wide or exact params)
//! - **[`GrantStore`]**: Trait for persisting grants
//! - **[`MemoryGrantStore`]**: In-memory store (cleared on exit)
//! - **[`FileGrantStore`]**: File-based persistent store
//!
//! # Default Behavior
//!
//! By default, tools without grants are **denied immediately**. This is secure
//! by default for non-interactive environments (scripts, CI/CD, automated agents).
//!
//! For interactive environments (REPLs, CLIs), use [`ToolCallAuthorizer::interactive()`]
//! to enable prompting users for authorization.
//!
//! # Example
//!
//! ```rust
//! use mixtape_core::permission::ToolCallAuthorizer;
//!
//! # tokio_test::block_on(async {
//! // Default: tools without grants are denied
//! let auth = ToolCallAuthorizer::new();
//!
//! // For interactive use: prompt for unknown tools
//! let auth = ToolCallAuthorizer::interactive();
//!
//! // Grant permission to use a tool
//! auth.grant_tool("echo").await.unwrap();
//!
//! // Check if a call is authorized
//! let params = serde_json::json!({"message": "hello"});
//! let result = auth.check("echo", &params).await;
//! assert!(result.is_authorized());
//! # });
//! ```
//!
//! # Grant Types
//!
//! | Type | Created With | Matches |
//! |------|--------------|---------|
//! | Tool-wide | `auth.grant_tool("name")` | Any invocation of the tool |
//! | Params | `auth.grant_params("name", &params)` | Only invocations with matching params |

mod authorizer;
mod grant;
mod store;

pub use authorizer::{
    Authorization, AuthorizationResponse, ToolAuthorizationPolicy, ToolCallAuthorizer,
};
pub use grant::{hash_params, Grant, Scope};
pub use store::{FileGrantStore, GrantStore, GrantStoreError, MemoryGrantStore};
