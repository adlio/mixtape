//! HTTP server and AG-UI protocol support for mixtape agents.
//!
//! This crate provides HTTP endpoints for running mixtape agents via web services,
//! with optional support for the AG-UI protocol used by CopilotKit.
//!
//! # Features
//!
//! - `agui` - Enable AG-UI protocol support for CopilotKit integration
//!
//! # Example
//!
//! ```rust,no_run
//! use mixtape_server::MixtapeRouter;
//! use mixtape_core::Agent;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create your agent (requires provider feature in mixtape-core)
//! # let agent: Agent = todo!();
//!
//! // Build the router with AG-UI support
//! let app = MixtapeRouter::new(agent)
//!     .with_agui("/api/copilotkit")
//!     .build()?;
//!
//! // Serve with axum
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
//! axum::serve(listener, app).await?;
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod router;
pub(crate) mod state;

#[cfg(feature = "agui")]
pub(crate) mod agui;

// Re-exports
pub use error::{BuildError, ServerError, ServerResult};
pub use router::MixtapeRouter;

// AG-UI protocol types (for consumers who need to reference the event types)
#[cfg(feature = "agui")]
pub use agui::events::{
    AguiEvent, GrantScope, InterruptData, InterruptResponse, InterruptType, MessageRole,
};
