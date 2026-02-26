use super::*;
use agent_client_protocol::{PermissionOptionId, PermissionOptionKind, SelectedPermissionOutcome};
use serde_json::json;

#[test]
fn test_build_permission_options_has_three_options() {
    let options = build_permission_options();
    assert_eq!(options.len(), 3);
}

#[test]
fn test_outcome_cancelled_maps_to_deny() {
    let outcome = RequestPermissionOutcome::Cancelled;
    let auth = outcome_to_authorization(outcome, "test_tool");
    match auth {
        AuthorizationResponse::Deny { reason } => {
            assert_eq!(reason, Some("User cancelled".to_string()));
        }
        _ => panic!("Expected Deny"),
    }
}

#[test]
fn test_outcome_allow_once_maps_to_once() {
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::from(OPTION_ALLOW_ONCE),
    ));
    let auth = outcome_to_authorization(outcome, "test_tool");
    assert!(matches!(auth, AuthorizationResponse::Once));
}

#[test]
fn test_outcome_allow_session_maps_to_trust() {
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::from(OPTION_ALLOW_SESSION),
    ));
    let auth = outcome_to_authorization(outcome, "test_tool");
    match auth {
        AuthorizationResponse::Trust { grant } => {
            assert_eq!(grant.tool, "test_tool");
            assert_eq!(grant.scope, Scope::Session);
        }
        _ => panic!("Expected Trust"),
    }
}

#[test]
fn test_outcome_deny_maps_to_deny_no_reason() {
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::from(OPTION_DENY),
    ));
    let auth = outcome_to_authorization(outcome, "test_tool");
    match auth {
        AuthorizationResponse::Deny { reason } => {
            assert!(reason.is_none());
        }
        _ => panic!("Expected Deny"),
    }
}

#[test]
fn test_outcome_unknown_option_maps_to_deny() {
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::from("unknown_option"),
    ));
    let auth = outcome_to_authorization(outcome, "test_tool");
    match auth {
        AuthorizationResponse::Deny { reason } => {
            assert!(reason.is_some());
            assert!(reason.unwrap().contains("Unknown permission option"));
        }
        _ => panic!("Expected Deny"),
    }
}

// ---------------------------------------------------------------------------
// Edge cases: option kinds and IDs are correctly assigned
// ---------------------------------------------------------------------------

#[test]
fn build_permission_options_assigns_correct_kinds() {
    let options = build_permission_options();
    assert_eq!(options.len(), 3);

    let kinds: Vec<&PermissionOptionKind> = options.iter().map(|o| &o.kind).collect();
    assert!(
        kinds.contains(&&PermissionOptionKind::AllowOnce),
        "options should include an AllowOnce kind"
    );
    assert!(
        kinds.contains(&&PermissionOptionKind::AllowAlways),
        "options should include an AllowAlways kind"
    );
    assert!(
        kinds.contains(&&PermissionOptionKind::RejectOnce),
        "options should include a RejectOnce kind"
    );
}

#[test]
fn build_permission_options_ids_match_constants() {
    let options = build_permission_options();
    let ids: Vec<String> = options.iter().map(|o| o.option_id.to_string()).collect();

    assert!(
        ids.contains(&OPTION_ALLOW_ONCE.to_string()),
        "allow_once ID must be present"
    );
    assert!(
        ids.contains(&OPTION_ALLOW_SESSION.to_string()),
        "allow_session ID must be present"
    );
    assert!(
        ids.contains(&OPTION_DENY.to_string()),
        "deny ID must be present"
    );
}

// ---------------------------------------------------------------------------
// build_permission_tool_call
// ---------------------------------------------------------------------------

#[test]
fn build_permission_tool_call_sets_tool_use_id() {
    let params = json!({"command": "ls -la"});
    let update = build_permission_tool_call("proposal-99", &params);
    assert_eq!(
        update.tool_call_id.to_string(),
        "proposal-99",
        "tool_call_id should match the proposal_id argument"
    );
}

#[test]
fn build_permission_tool_call_embeds_params_as_raw_input() {
    let params = json!({"path": "/etc/passwd", "mode": "read"});
    let update = build_permission_tool_call("p-id", &params);
    let raw = update.fields.raw_input.expect("raw_input must be set");
    assert_eq!(
        raw, params,
        "raw_input should be the params value unchanged"
    );
}

#[test]
fn build_permission_tool_call_handles_empty_params() {
    let params = json!({});
    let update = build_permission_tool_call("empty-params", &params);
    let raw = update
        .fields
        .raw_input
        .expect("raw_input must be set even for empty params");
    assert_eq!(raw, params);
}

// ---------------------------------------------------------------------------
// outcome_to_authorization: trust grant preserves tool name
// ---------------------------------------------------------------------------

#[test]
fn allow_session_trust_grant_uses_provided_tool_name() {
    let cases = [("bash", "bash"), ("read_file", "read_file"), ("", "")];

    for (tool_name, expected_grant_tool) in cases {
        let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            PermissionOptionId::from(OPTION_ALLOW_SESSION),
        ));
        let auth = outcome_to_authorization(outcome, tool_name);
        match auth {
            AuthorizationResponse::Trust { grant } => {
                assert_eq!(
                    grant.tool, expected_grant_tool,
                    "grant.tool should match the tool_name argument"
                );
                assert_eq!(grant.scope, Scope::Session);
            }
            _ => panic!("expected Trust for tool '{}'", tool_name),
        }
    }
}

#[test]
fn unknown_option_reason_includes_the_option_id() {
    let bad_id = "some_totally_unknown_id";
    let outcome = RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
        PermissionOptionId::from(bad_id),
    ));
    let auth = outcome_to_authorization(outcome, "tool");
    match auth {
        AuthorizationResponse::Deny { reason } => {
            let reason_text = reason.expect("unknown option should have a reason");
            assert!(
                reason_text.contains(bad_id),
                "reason should include the unknown option ID '{}', got: {}",
                bad_id,
                reason_text
            );
        }
        _ => panic!("expected Deny for unknown option"),
    }
}
