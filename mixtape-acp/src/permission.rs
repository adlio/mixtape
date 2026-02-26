use std::sync::Arc;

use agent_client_protocol::{
    PermissionOption, PermissionOptionId, PermissionOptionKind, RequestPermissionOutcome,
    SessionId, ToolCallId, ToolCallUpdateFields,
};
use mixtape_core::{Agent, AuthorizationResponse, Grant, Scope};
use serde_json::Value;

/// A request to bridge a mixtape permission check to an ACP permission dialog.
///
/// Sent from the agent hook (via mpsc channel) to the relay task, which calls
/// `conn.request_permission()` and then delivers the result back to the agent
/// via `agent.respond_to_authorization()`.
pub(crate) struct PermissionBridgeRequest {
    pub session_id: SessionId,
    pub proposal_id: String,
    pub tool_name: String,
    pub params: Value,
    pub agent: Arc<Agent>,
}

/// Well-known permission option IDs.
const OPTION_ALLOW_ONCE: &str = "allow_once";
const OPTION_ALLOW_SESSION: &str = "allow_session";
const OPTION_DENY: &str = "deny";

/// Build the standard set of ACP permission options for a tool call.
pub(crate) fn build_permission_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new(
            PermissionOptionId::from(OPTION_ALLOW_ONCE),
            "Allow Once".to_string(),
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new(
            PermissionOptionId::from(OPTION_ALLOW_SESSION),
            "Always Allow (Session)".to_string(),
            PermissionOptionKind::AllowAlways,
        ),
        PermissionOption::new(
            PermissionOptionId::from(OPTION_DENY),
            "Deny".to_string(),
            PermissionOptionKind::RejectOnce,
        ),
    ]
}

/// Build the `ToolCallUpdate` describing the tool for a permission request.
pub(crate) fn build_permission_tool_call(
    tool_use_id: &str,
    params: &Value,
) -> agent_client_protocol::ToolCallUpdate {
    let fields = ToolCallUpdateFields::new().raw_input(params.clone());
    agent_client_protocol::ToolCallUpdate::new(ToolCallId::from(tool_use_id.to_string()), fields)
}

/// Convert an ACP `RequestPermissionOutcome` into a mixtape `AuthorizationResponse`.
pub(crate) fn outcome_to_authorization(
    outcome: RequestPermissionOutcome,
    tool_name: &str,
) -> AuthorizationResponse {
    match outcome {
        RequestPermissionOutcome::Cancelled => AuthorizationResponse::Deny {
            reason: Some("User cancelled".to_string()),
        },
        RequestPermissionOutcome::Selected(selected) => {
            let id = selected.option_id.to_string();
            match id.as_str() {
                OPTION_ALLOW_ONCE => AuthorizationResponse::Once,
                OPTION_ALLOW_SESSION => AuthorizationResponse::Trust {
                    grant: Grant::tool(tool_name).with_scope(Scope::Session),
                },
                OPTION_DENY => AuthorizationResponse::Deny { reason: None },
                // Unknown option — treat as deny for safety
                other => AuthorizationResponse::Deny {
                    reason: Some(format!("Unknown permission option: {}", other)),
                },
            }
        }
        // Catch any future variants
        #[allow(unreachable_patterns)]
        _ => AuthorizationResponse::Deny {
            reason: Some("Unrecognized permission outcome".to_string()),
        },
    }
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;
