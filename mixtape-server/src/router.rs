//! Router builder for mixtape HTTP endpoints.

use std::sync::Arc;

use axum::Router;
use mixtape_core::Agent;

use crate::error::BuildError;
use crate::state::AppState;

/// Builder for configuring mixtape HTTP endpoints.
///
/// # Example
///
/// ```rust,no_run
/// use mixtape_server::MixtapeRouter;
/// use mixtape_core::Agent;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let agent: Agent = todo!();
/// // Simple setup with AG-UI endpoint
/// let app = MixtapeRouter::new(agent)
///     .with_agui("/api/copilotkit")
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct MixtapeRouter {
    agent: Arc<Agent>,
    #[cfg(feature = "agui")]
    agui_path: Option<String>,
    #[cfg(feature = "agui")]
    interrupt_path: Option<String>,
}

impl MixtapeRouter {
    /// Create a new router builder with the given agent.
    ///
    /// The agent will be wrapped in an `Arc` for sharing across handlers.
    pub fn new(agent: Agent) -> Self {
        Self {
            agent: Arc::new(agent),
            #[cfg(feature = "agui")]
            agui_path: None,
            #[cfg(feature = "agui")]
            interrupt_path: None,
        }
    }

    /// Create a new router builder from an existing `Arc<Agent>`.
    ///
    /// Use this when you need to share the agent with other parts of your application.
    pub fn from_arc(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            #[cfg(feature = "agui")]
            agui_path: None,
            #[cfg(feature = "agui")]
            interrupt_path: None,
        }
    }

    /// Enable AG-UI protocol endpoint at the specified path.
    ///
    /// This also enables an interrupt endpoint at `{path}/interrupt` for handling
    /// permission responses. Use [`interrupt_path`](Self::interrupt_path) to customize
    /// the interrupt endpoint path.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use mixtape_server::MixtapeRouter;
    /// # use mixtape_core::Agent;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let agent: Agent = todo!();
    /// let app = MixtapeRouter::new(agent)
    ///     .with_agui("/api/copilotkit")  // SSE endpoint at /api/copilotkit
    ///     .build()?;                      // Interrupt at /api/copilotkit/interrupt
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "agui")]
    pub fn with_agui(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        self.interrupt_path = Some(format!("{}/interrupt", path));
        self.agui_path = Some(path);
        self
    }

    /// Set a custom path for the interrupt endpoint.
    ///
    /// By default, the interrupt endpoint is at `{agui_path}/interrupt`.
    /// Use this method to override that default.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use mixtape_server::MixtapeRouter;
    /// # use mixtape_core::Agent;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let agent: Agent = todo!();
    /// let app = MixtapeRouter::new(agent)
    ///     .with_agui("/api/copilotkit")
    ///     .interrupt_path("/api/approve")  // Custom interrupt path
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "agui")]
    pub fn interrupt_path(mut self, path: impl Into<String>) -> Self {
        self.interrupt_path = Some(path.into());
        self
    }

    /// Build the router with all configured endpoints.
    ///
    /// Returns an axum `Router` that can be served directly or merged
    /// with other routes.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::NoEndpoints`] if no endpoints were configured.
    /// Call `.with_agui()` before `.build()`.
    pub fn build(self) -> Result<Router, BuildError> {
        // Validate that at least one endpoint is configured
        #[cfg(feature = "agui")]
        let has_endpoints = self.agui_path.is_some();
        #[cfg(not(feature = "agui"))]
        let has_endpoints = false;

        if !has_endpoints {
            return Err(BuildError::NoEndpoints);
        }

        let state = AppState::from_arc(self.agent);
        let mut router = Router::new();

        // Add AG-UI endpoints if enabled and configured
        #[cfg(feature = "agui")]
        if let Some(agui_path) = self.agui_path {
            use crate::agui::handler::{agui_handler, interrupt_handler};
            use axum::routing::post;

            router = router.route(&agui_path, post(agui_handler));

            if let Some(interrupt_path) = self.interrupt_path {
                router = router.route(&interrupt_path, post(interrupt_handler));
            }
        }

        Ok(router.with_state(state))
    }

    /// Build the router and nest it under a prefix path.
    ///
    /// This is useful when integrating with an existing application router.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::NoEndpoints`] if no endpoints were configured.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use mixtape_server::MixtapeRouter;
    /// # use mixtape_core::Agent;
    /// # use axum::Router;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let agent: Agent = todo!();
    /// // Nest mixtape routes under /agent
    /// let mixtape = MixtapeRouter::new(agent)
    ///     .with_agui("/stream")  // Will be at /agent/stream
    ///     .build_nested("/agent")?;
    ///
    /// // Merge with existing routes
    /// let app = Router::new()
    ///     .merge(mixtape);
    /// # Ok(())
    /// # }
    /// ```
    pub fn build_nested(self, prefix: impl Into<String>) -> Result<Router, BuildError> {
        Ok(Router::new().nest(&prefix.into(), self.build()?))
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
