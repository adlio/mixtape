//! HTTP handlers for the AgentCore runtime protocol.
//!
//! Implements the [AgentCore HTTP protocol contract](https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-http-protocol-contract.html):
//!
//! - `GET /ping` - Health check endpoint
//! - `POST /invocations` - Agent invocation endpoint (returns SSE stream)

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::stream::Stream;
use mixtape_core::events::AgentEvent;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use super::convert::{convert_event, ConversionContext};
use super::events::{AgentCoreEvent, InvocationRequest, PingResponse};
use crate::state::AppState;

/// AgentCore session ID header injected by the platform.
pub const SESSION_ID_HEADER: &str = "x-amzn-bedrock-agentcore-runtime-session-id";

/// AgentCore user ID header injected by the platform.
pub const USER_ID_HEADER: &str = "x-amzn-bedrock-agentcore-runtime-user-id";

/// Handle health check requests.
///
/// Returns `{"status": "Healthy", "time_of_last_update": <unix_timestamp>}`.
///
/// AgentCore calls this endpoint to determine if the agent is ready to
/// accept invocations. Returning `"Healthy"` indicates the agent is ready.
pub async fn ping_handler() -> Json<PingResponse> {
    Json(PingResponse {
        status: "Healthy".to_string(),
        time_of_last_update: chrono::Utc::now().timestamp(),
    })
}

/// Handle agent invocation requests.
///
/// Accepts a JSON body with a `prompt` field and returns an SSE stream
/// of [`AgentCoreEvent`]s. The AgentCore platform injects session and user
/// ID headers which are extracted and logged.
///
/// # Headers
///
/// - `X-Amzn-Bedrock-AgentCore-Runtime-Session-Id` - Session identifier
/// - `X-Amzn-Bedrock-AgentCore-Runtime-User-Id` - User identifier
pub async fn invocations_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<InvocationRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let agent = state.agent.clone();
    let prompt = request.prompt;

    // Extract AgentCore headers (available but not required for basic operation)
    let _session_id = headers
        .get(SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let _user_id = headers
        .get(USER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Create channel for streaming events
    let (tx, rx) = mpsc::channel::<AgentCoreEvent>(100);

    // Spawn the agent run task
    let tx_for_task = tx.clone();
    tokio::spawn(async move {
        // Set up event conversion context
        let ctx = Arc::new(parking_lot::Mutex::new(ConversionContext::new()));

        // Add hook to convert and forward events
        let ctx_for_hook = ctx.clone();
        let tx_for_hook = tx_for_task.clone();
        let hook_id = agent.add_hook(move |event: &AgentEvent| {
            let mut ctx_guard = ctx_for_hook.lock();
            let agentcore_events = convert_event(event, &mut ctx_guard);
            for agentcore_event in agentcore_events {
                let _ = tx_for_hook.try_send(agentcore_event);
            }
        });

        // Run the agent
        match agent.run(&prompt).await {
            Ok(_response) => {
                // RunCompleted event is already emitted via hook
            }
            Err(e) => {
                let _ = tx_for_task.try_send(AgentCoreEvent::RunError {
                    message: e.to_string(),
                });
            }
        }

        // Clean up hook
        agent.remove_hook(hook_id);
    });

    // Convert channel to SSE stream
    let stream = ReceiverStream::new(rx).map(|event| {
        let json = serde_json::to_string(&event).unwrap_or_else(|e| {
            serde_json::json!({
                "type": "run_error",
                "message": format!("Failed to serialize event: {}", e)
            })
            .to_string()
        });
        Ok::<_, Infallible>(Event::default().data(json))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;
