//! HTTP handlers for AG-UI protocol endpoints.

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::Stream;
use mixtape_core::events::AgentEvent;
use mixtape_core::permission::{AuthorizationResponse, Grant, Scope};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use super::convert::{convert_event, ConversionContext};
use super::events::{AguiEvent, GrantScope, InterruptResponse};
use crate::error::ServerError;
use crate::state::AppState;

/// Request body for running an agent.
#[derive(Debug, Deserialize)]
pub struct AgentRequest {
    /// User message to send to the agent.
    pub message: String,
    /// Thread ID for conversation continuity.
    #[serde(default)]
    pub thread_id: Option<String>,
    /// Run ID for this specific run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Optional run options (included for AG-UI protocol compatibility).
    #[serde(default)]
    #[allow(dead_code)]
    pub options: RunOptions,
}

/// Options for agent run.
#[derive(Debug, Deserialize)]
pub struct RunOptions {
    /// Whether to stream responses (always true for AG-UI, included for compatibility).
    #[serde(default = "default_true")]
    #[allow(dead_code)]
    pub stream: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { stream: true }
    }
}

fn default_true() -> bool {
    true
}

/// Request body for responding to an interrupt (permission request).
#[derive(Debug, Deserialize)]
pub struct InterruptRequest {
    /// The interrupt ID to respond to.
    pub interrupt_id: String,
    /// Tool name (from interrupt data).
    pub tool_name: String,
    /// Params hash (from interrupt data, for exact grants).
    #[serde(default)]
    pub params_hash: Option<String>,
    /// The response action.
    pub response: InterruptResponse,
}

/// Handle AG-UI protocol requests.
///
/// Accepts POST with AgentRequest body, returns SSE stream of AG-UI events.
pub async fn agui_handler(
    State(state): State<AppState>,
    Json(request): Json<AgentRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let agent = state.agent.clone();
    let thread_id = request
        .thread_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let run_id = request
        .run_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let message = request.message;

    // Create channel for AG-UI events
    let (tx, rx) = mpsc::channel::<AguiEvent>(100);

    // Spawn agent run task
    let tx_for_task = tx.clone();
    let thread_id_clone = thread_id.clone();
    let run_id_clone = run_id.clone();

    tokio::spawn(async move {
        // Create conversion context with shared state
        let ctx = Arc::new(parking_lot::Mutex::new(ConversionContext::new(
            thread_id_clone,
            run_id_clone,
        )));

        // Add hook to forward events (capture hook ID for cleanup)
        let ctx_for_hook = ctx.clone();
        let tx_for_hook = tx_for_task.clone();
        let hook_id = agent.add_hook(move |event: &AgentEvent| {
            let mut ctx_guard = ctx_for_hook.lock();
            let agui_events = convert_event(event, &mut ctx_guard);
            for agui_event in agui_events {
                // Non-blocking send - drop events if channel is full
                let _ = tx_for_hook.try_send(agui_event);
            }
        });

        // Run the agent
        match agent.run(&message).await {
            Ok(_response) => {
                // RunCompleted event is already emitted via hook
            }
            Err(e) => {
                let _ = tx_for_task.try_send(AguiEvent::RunError {
                    message: e.to_string(),
                    code: None,
                });
            }
        }

        // Clean up: remove the hook after the run completes
        agent.remove_hook(hook_id);
    });

    // Convert channel to SSE stream
    let stream = ReceiverStream::new(rx).map(|event| {
        let json = serde_json::to_string(&event).unwrap_or_else(|e| {
            serde_json::json!({
                "type": "RUN_ERROR",
                "message": format!("Failed to serialize event: {}", e)
            })
            .to_string()
        });
        Ok::<_, Infallible>(Event::default().data(json))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Handle interrupt responses (permission decisions).
///
/// This endpoint receives permission decisions from the frontend and
/// forwards them to the agent.
pub async fn interrupt_handler(
    State(state): State<AppState>,
    Json(request): Json<InterruptRequest>,
) -> Result<Json<serde_json::Value>, ServerError> {
    // Convert InterruptResponse to AuthorizationResponse
    let auth_response = match request.response {
        InterruptResponse::ApproveOnce => AuthorizationResponse::Once,
        InterruptResponse::TrustTool { scope } => {
            let core_scope = convert_scope(scope);
            AuthorizationResponse::Trust {
                grant: Grant::tool(&request.tool_name).with_scope(core_scope),
            }
        }
        InterruptResponse::TrustExact { scope } => {
            let core_scope = convert_scope(scope);
            let hash = request.params_hash.ok_or_else(|| {
                ServerError::InvalidRequest("params_hash required for TrustExact".to_string())
            })?;
            AuthorizationResponse::Trust {
                grant: Grant::exact(&request.tool_name, &hash).with_scope(core_scope),
            }
        }
        InterruptResponse::Deny { reason } => AuthorizationResponse::Deny { reason },
    };

    state
        .agent
        .respond_to_authorization(&request.interrupt_id, auth_response)
        .await
        .map_err(|e| ServerError::Permission(e.to_string()))?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// Convert AG-UI GrantScope to mixtape-core Scope.
fn convert_scope(scope: GrantScope) -> Scope {
    match scope {
        GrantScope::Session => Scope::Session,
        GrantScope::Persistent => Scope::Persistent,
    }
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
