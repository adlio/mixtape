//! Agent module for orchestrating LLM interactions with tools
//!
//! The Agent is the core orchestrator that manages conversations with language models,
//! executes tools, handles permission workflows, and maintains session state.

mod builder;
mod context;
mod helpers;
#[cfg(feature = "mcp")]
mod mcp;
mod permission;
mod run;
mod streaming;
mod tools;
mod types;

#[cfg(feature = "session")]
mod session;

// Re-export public types
pub use builder::AgentBuilder;
pub use context::{ContextConfig, ContextError, ContextLoadResult, ContextSource};
pub use types::{
    AgentError, AgentResponse, PermissionError, TokenUsageStats, ToolCallInfo, ToolInfo,
    DEFAULT_MAX_CONCURRENT_TOOLS, DEFAULT_PERMISSION_TIMEOUT,
};

#[cfg(feature = "session")]
pub use types::SessionInfo;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use crate::conversation::BoxedConversationManager;
use crate::events::{AgentEvent, AgentHook};
use crate::permission::{AuthorizationResponse, ToolCallAuthorizer};
use crate::provider::ModelProvider;
use crate::tool::DynTool;
use crate::types::Message;

#[cfg(feature = "session")]
use crate::session::SessionStore;

/// Agent that orchestrates interactions between a language model and tools
///
/// Create an agent using the builder pattern:
///
/// ```ignore
/// use mixtape_core::{Agent, ClaudeSonnet4_5, Result};
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let agent = Agent::builder()
///         .bedrock(ClaudeSonnet4_5)
///         .with_system_prompt("You are a helpful assistant")
///         .build()
///         .await?;
///
///     let response = agent.run("Hello!").await?;
///     println!("{}", response);
///     Ok(())
/// }
/// ```
pub struct Agent {
    pub(super) provider: Arc<dyn ModelProvider>,
    pub(super) system_prompt: Option<String>,
    pub(super) max_concurrent_tools: usize,
    pub(super) tools: Vec<Box<dyn DynTool>>,
    pub(super) hooks: Arc<parking_lot::RwLock<Vec<Arc<dyn AgentHook>>>>,
    /// Tool call authorizer (always present, uses MemoryGrantStore by default)
    pub(super) authorizer: Arc<RwLock<ToolCallAuthorizer>>,
    /// Timeout for authorization requests
    pub(super) authorization_timeout: Duration,
    /// Pending authorization requests
    pub(super) pending_authorizations:
        Arc<RwLock<HashMap<String, mpsc::Sender<AuthorizationResponse>>>>,
    /// MCP clients for graceful shutdown
    #[cfg(feature = "mcp")]
    pub(super) mcp_clients: Vec<Arc<crate::mcp::McpClient>>,
    /// Conversation manager for context window handling
    pub(super) conversation_manager: parking_lot::RwLock<BoxedConversationManager>,

    #[cfg(feature = "session")]
    pub(super) session_store: Option<Arc<dyn SessionStore>>,

    // Context file fields
    /// Context file sources (resolved at runtime)
    pub(super) context_sources: Vec<ContextSource>,
    /// Context configuration (size limits)
    pub(super) context_config: ContextConfig,
    /// Last context load result (for inspection)
    pub(super) last_context_result: parking_lot::RwLock<Option<ContextLoadResult>>,
}

impl Agent {
    /// Add an event hook to observe agent execution
    ///
    /// Hooks receive notifications about agent lifecycle, model calls,
    /// and tool executions in real-time.
    ///
    /// # Example
    /// ```ignore
    /// use mixtape_core::{Agent, ClaudeSonnet4_5, AgentEvent, AgentHook};
    ///
    /// struct Logger;
    ///
    /// impl AgentHook for Logger {
    ///     fn on_event(&self, event: &AgentEvent) {
    ///         println!("Event: {:?}", event);
    ///     }
    /// }
    ///
    /// let agent = Agent::builder()
    ///     .bedrock(ClaudeSonnet4_5)
    ///     .build()
    ///     .await?;
    /// agent.add_hook(Logger);
    /// ```
    pub fn add_hook(&self, hook: impl AgentHook + 'static) {
        self.hooks.write().push(Arc::new(hook));
    }

    /// Emit an event to all registered hooks
    pub(crate) fn emit_event(&self, event: AgentEvent) {
        let hooks = self.hooks.read();
        for hook in hooks.iter() {
            hook.on_event(&event);
        }
    }

    /// Get the model name for display
    pub fn model_name(&self) -> &str {
        self.provider.name()
    }

    /// Gracefully shutdown the agent, disconnecting MCP servers
    ///
    /// Call this before dropping the agent to ensure clean subprocess termination.
    pub async fn shutdown(&self) {
        #[cfg(feature = "mcp")]
        for client in &self.mcp_clients {
            let _ = client.disconnect().await;
        }
    }

    /// Get current context usage information
    ///
    /// Returns statistics about how much of the context window is being used,
    /// including the number of messages and estimated token count.
    pub fn get_context_usage(&self) -> crate::conversation::ContextUsage {
        let limits = crate::conversation::ContextLimits::new(self.provider.max_context_tokens());
        let provider = &self.provider;
        let estimate_tokens = |msgs: &[Message]| provider.estimate_message_tokens(msgs);

        self.conversation_manager
            .read()
            .context_usage(limits, &estimate_tokens)
    }

    /// Get information about the most recently loaded context files
    ///
    /// Returns `None` if `run()` has not been called yet.
    ///
    /// # Example
    /// ```ignore
    /// let response = agent.run("Hello").await?;
    ///
    /// if let Some(ctx) = agent.last_context_info() {
    ///     println!("Loaded {} context files ({} bytes)",
    ///         ctx.files.len(), ctx.total_bytes);
    ///     for file in &ctx.files {
    ///         println!("  - {}", file.resolved_path.display());
    ///     }
    /// }
    /// ```
    pub fn last_context_info(&self) -> Option<ContextLoadResult> {
        self.last_context_result.read().clone()
    }
}
