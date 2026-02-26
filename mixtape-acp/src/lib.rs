//! ACP (Agent Client Protocol) adapter for mixtape agents.
//!
//! This crate bridges [mixtape-core](mixtape_core) agents to the
//! [Agent Client Protocol](https://agentclientprotocol.com), enabling editors
//! and IDEs — VS Code, Zed, Neovim, JetBrains, Emacs, etc. — to use
//! mixtape agents.
//!
//! # Architecture
//!
//! The ACP SDK returns `!Send` futures (uses RPITIT without `+ Send` bounds),
//! while mixtape's `Agent` is `Send + Sync`. The bridge works as follows:
//!
//! 1. The ACP protocol loop runs on a `tokio::task::LocalSet`
//! 2. `MixtapeAcpAgent` implements `acp::Agent` — its methods run in `!Send` context
//! 3. Inside `prompt()`, `Agent::run()` is dispatched via `tokio::spawn()` onto
//!    the multi-threaded runtime
//! 4. Events flow back via mpsc channels to a `spawn_local` relay task that
//!    calls `conn.session_notification()`
//! 5. Permission requests carry an `Arc<Agent>` — the relay task calls
//!    `conn.request_permission()` to show a dialog in the IDE, then delivers
//!    the result via `agent.respond_to_authorization()`, unblocking the agent's
//!    `request_authorization()` which is waiting on an mpsc channel
//!
//! # Example
//!
//! ```rust,no_run
//! use mixtape_acp::{MixtapeAcpBuilder, serve_stdio};
//! use mixtape_core::Agent;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = MixtapeAcpBuilder::new("my-agent", "0.1.0")
//!         .with_agent_factory(|| async {
//!             Agent::builder()
//!                 // .bedrock(ClaudeSonnet4)
//!                 .build()
//!                 .await
//!         })
//!         .build()?;
//!
//!     serve_stdio(server).await?;
//!     Ok(())
//! }
//! ```

mod adapter;
mod builder;
mod convert;
pub mod error;
mod permission;
mod session;
mod types;

pub use builder::{MixtapeAcpBuilder, MixtapeAcpServer};
pub use error::AcpError;

use agent_client_protocol::{
    AgentSideConnection, Client, RequestPermissionRequest, SessionNotification,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::permission::{
    build_permission_options, build_permission_tool_call, outcome_to_authorization,
};

/// Serve the ACP protocol over stdin/stdout.
///
/// This is the main entry point for running a mixtape agent as an ACP server.
/// It sets up the `LocalSet` required by the ACP SDK, connects via stdio,
/// and drives the notification/permission relay.
pub async fn serve_stdio(server: MixtapeAcpServer) -> Result<(), AcpError> {
    let MixtapeAcpServer {
        adapter,
        mut notification_rx,
        mut permission_rx,
    } = server;

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let stdin = tokio::io::stdin().compat();
            let stdout = tokio::io::stdout().compat_write();

            let (conn, io_future) = AgentSideConnection::new(adapter, stdout, stdin, |fut| {
                tokio::task::spawn_local(fut);
            });

            // Notification and permission relay task
            tokio::task::spawn_local(async move {
                loop {
                    tokio::select! {
                        Some(msg) = notification_rx.recv() => {
                            let notification = SessionNotification::new(
                                msg.session_id,
                                msg.update,
                            );
                            let _ = conn.session_notification(notification).await;
                        }
                        Some(req) = permission_rx.recv() => {
                            let tool_call = build_permission_tool_call(
                                &req.proposal_id,
                                &req.params,
                            );
                            let options = build_permission_options();

                            let perm_request = RequestPermissionRequest::new(
                                req.session_id,
                                tool_call,
                                options,
                            );

                            let auth = match conn.request_permission(perm_request).await {
                                Ok(response) => outcome_to_authorization(
                                    response.outcome,
                                    &req.tool_name,
                                ),
                                Err(_) => mixtape_core::AuthorizationResponse::Deny {
                                    reason: Some("Permission request failed".to_string()),
                                },
                            };

                            // Deliver the IDE's response back to the agent,
                            // unblocking request_authorization().
                            if let Err(e) = req.agent
                                .respond_to_authorization(&req.proposal_id, auth)
                                .await
                            {
                                log::warn!(
                                    "Failed to deliver permission response for {}: {}",
                                    req.proposal_id, e
                                );
                            }
                        }
                        else => break,
                    }
                }
            });

            io_future
                .await
                .map_err(|e| AcpError::Transport(e.to_string()))
        })
        .await
}
