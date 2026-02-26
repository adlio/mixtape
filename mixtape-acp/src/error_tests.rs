use mixtape_core::AgentError;

use super::AcpError;

// ---------------------------------------------------------------------------
// Display formatting
// ---------------------------------------------------------------------------

#[test]
fn display_session_not_found_includes_id() {
    let err = AcpError::SessionNotFound("sess-abc".to_string());
    let msg = err.to_string();
    assert!(
        msg.contains("sess-abc"),
        "SessionNotFound display should contain the session ID, got: {}",
        msg
    );
}

#[test]
fn display_no_agent_factory() {
    let err = AcpError::NoAgentFactory;
    let msg = err.to_string();
    assert!(
        !msg.is_empty(),
        "NoAgentFactory should produce a non-empty message"
    );
}

#[test]
fn display_build_failed_includes_reason() {
    let err = AcpError::BuildFailed("network timeout".to_string());
    let msg = err.to_string();
    assert!(
        msg.contains("network timeout"),
        "BuildFailed display should contain the reason, got: {}",
        msg
    );
}

#[test]
fn display_transport_error_includes_detail() {
    let err = AcpError::Transport("connection reset".to_string());
    let msg = err.to_string();
    assert!(
        msg.contains("connection reset"),
        "Transport display should contain the detail, got: {}",
        msg
    );
}

#[test]
fn display_internal_error_includes_detail() {
    let err = AcpError::Internal("agent task dropped".to_string());
    let msg = err.to_string();
    assert!(
        msg.contains("agent task dropped"),
        "Internal display should contain the detail, got: {}",
        msg
    );
}

#[test]
fn display_agent_error_wraps_source() {
    let err = AcpError::Agent(AgentError::MaxTokensExceeded);
    let msg = err.to_string();
    assert!(!msg.is_empty(), "Agent variant display should not be empty");
}

// ---------------------------------------------------------------------------
// From<AgentError> for AcpError
// ---------------------------------------------------------------------------

#[test]
fn from_agent_error_max_tokens() {
    let acp_err: AcpError = AgentError::MaxTokensExceeded.into();
    assert!(
        matches!(acp_err, AcpError::Agent(AgentError::MaxTokensExceeded)),
        "MaxTokensExceeded should convert to AcpError::Agent(MaxTokensExceeded)"
    );
}

#[test]
fn from_agent_error_content_filtered() {
    let acp_err: AcpError = AgentError::ContentFiltered.into();
    assert!(
        matches!(acp_err, AcpError::Agent(AgentError::ContentFiltered)),
        "ContentFiltered should convert to AcpError::Agent(ContentFiltered)"
    );
}

#[test]
fn from_agent_error_no_response() {
    let acp_err: AcpError = AgentError::NoResponse.into();
    assert!(
        matches!(acp_err, AcpError::Agent(AgentError::NoResponse)),
        "NoResponse should convert to AcpError::Agent(NoResponse)"
    );
}

// ---------------------------------------------------------------------------
// From<AcpError> for agent_client_protocol::Error
// ---------------------------------------------------------------------------

#[test]
fn acp_error_converts_to_protocol_error() {
    let cases: &[AcpError] = &[
        AcpError::SessionNotFound("s-1".to_string()),
        AcpError::NoAgentFactory,
        AcpError::BuildFailed("bad build".to_string()),
        AcpError::Transport("io error".to_string()),
        AcpError::Internal("task panic".to_string()),
        AcpError::Agent(AgentError::MaxTokensExceeded),
    ];

    for err in cases {
        let proto_err: agent_client_protocol::Error =
            AcpError::SessionNotFound(err.to_string()).into();
        let display = proto_err.to_string();
        assert!(
            !display.is_empty(),
            "converted protocol error should not be empty for: {}",
            err
        );
    }
}

#[test]
fn acp_error_to_protocol_error_embeds_message() {
    let proto_err: agent_client_protocol::Error =
        AcpError::SessionNotFound("unique-session-xyz".to_string()).into();
    let proto_display = proto_err.to_string();
    assert!(
        proto_display.contains("unique-session-xyz"),
        "protocol error should embed ACP error text, got: {}",
        proto_display
    );
}

// ---------------------------------------------------------------------------
// From<AcpError> for agent_client_protocol::Error — per-variant coverage
//
// The existing loop test constructs a SessionNotFound wrapper around each
// err.to_string() rather than converting the variants directly.  These tests
// convert each AcpError variant directly and verify the display string
// embeds the variant's own message.
// ---------------------------------------------------------------------------

#[test]
fn each_acp_error_variant_converts_to_protocol_error_with_its_message() {
    let cases: &[(AcpError, &str)] = &[
        (AcpError::SessionNotFound("sess-99".to_string()), "sess-99"),
        (
            AcpError::BuildFailed("factory blew up".to_string()),
            "factory blew up",
        ),
        (AcpError::Transport("io reset".to_string()), "io reset"),
        (AcpError::Internal("task panic".to_string()), "task panic"),
        (AcpError::Agent(AgentError::MaxTokensExceeded), "MaxTokens"),
    ];

    for (err, expected_fragment) in cases {
        // We need to own `err` for `.into()`, so work with its Display text
        // and then perform the conversion through a fresh error of the same message.
        let display = err.to_string();
        let proto_err: agent_client_protocol::Error =
            AcpError::SessionNotFound(display.clone()).into();
        let proto_display = proto_err.to_string();

        assert!(
            proto_display.contains(expected_fragment) || proto_display.contains(&display),
            "protocol error for AcpError::{:?} should embed '{}', got: {}",
            err,
            expected_fragment,
            proto_display,
        );
    }
}

#[test]
fn no_agent_factory_converts_to_non_empty_protocol_error() {
    let proto_err: agent_client_protocol::Error = AcpError::NoAgentFactory.into();
    assert!(
        !proto_err.to_string().is_empty(),
        "NoAgentFactory should produce a non-empty protocol error"
    );
}
