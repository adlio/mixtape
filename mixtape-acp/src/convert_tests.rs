use super::*;
use mixtape_core::ToolResult;
use std::time::{Duration, Instant};

#[test]
fn test_model_call_streaming_converts_to_agent_message_chunk() {
    let event = AgentEvent::ModelCallStreaming {
        delta: "Hello".to_string(),
        accumulated_length: 5,
    };
    let update = agent_event_to_session_update(&event);
    assert!(update.is_some());
    assert!(matches!(
        update.unwrap(),
        SessionUpdate::AgentMessageChunk(_)
    ));
}

#[test]
fn test_tool_requested_converts_to_tool_call() {
    let event = AgentEvent::ToolRequested {
        tool_use_id: "tool-123".to_string(),
        name: "read_file".to_string(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
    };
    let update = agent_event_to_session_update(&event);
    assert!(update.is_some());
    assert!(matches!(update.unwrap(), SessionUpdate::ToolCall(_)));
}

#[test]
fn test_tool_executing_converts_to_tool_call_update_in_progress() {
    let event = AgentEvent::ToolExecuting {
        tool_use_id: "tool-123".to_string(),
        name: "read_file".to_string(),
    };
    let update = agent_event_to_session_update(&event);
    assert!(update.is_some());
    assert!(matches!(update.unwrap(), SessionUpdate::ToolCallUpdate(_)));
}

#[test]
fn test_tool_completed_converts_to_tool_call_update_completed() {
    let event = AgentEvent::ToolCompleted {
        tool_use_id: "tool-123".to_string(),
        name: "read_file".to_string(),
        output: ToolResult::text("file contents here"),
        duration: Duration::from_millis(100),
    };
    let update = agent_event_to_session_update(&event);
    assert!(update.is_some());
    assert!(matches!(update.unwrap(), SessionUpdate::ToolCallUpdate(_)));
}

#[test]
fn test_tool_failed_converts_to_tool_call_update_failed() {
    let event = AgentEvent::ToolFailed {
        tool_use_id: "tool-123".to_string(),
        name: "read_file".to_string(),
        error: "file not found".to_string(),
        duration: Duration::from_millis(50),
    };
    let update = agent_event_to_session_update(&event);
    assert!(update.is_some());
    assert!(matches!(update.unwrap(), SessionUpdate::ToolCallUpdate(_)));
}

#[test]
fn test_lifecycle_events_return_none() {
    let lifecycle_events = vec![
        AgentEvent::RunStarted {
            input: "hello".to_string(),
            timestamp: Instant::now(),
        },
        AgentEvent::RunCompleted {
            output: "world".to_string(),
            duration: Duration::from_secs(1),
        },
        AgentEvent::RunFailed {
            error: "oops".to_string(),
            duration: Duration::from_secs(1),
        },
        AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 0,
            timestamp: Instant::now(),
        },
        AgentEvent::ModelCallCompleted {
            response_content: "done".to_string(),
            tokens: None,
            duration: Duration::from_millis(500),
            stop_reason: None,
        },
    ];

    for event in &lifecycle_events {
        assert!(
            agent_event_to_session_update(event).is_none(),
            "Expected None for {:?}",
            event
        );
    }
}

#[test]
fn test_permission_events_return_none() {
    let events = vec![
        AgentEvent::PermissionRequired {
            proposal_id: "p-1".to_string(),
            tool_name: "shell".to_string(),
            params: serde_json::json!({}),
            params_hash: "abc".to_string(),
        },
        AgentEvent::PermissionGranted {
            tool_use_id: "t-1".to_string(),
            tool_name: "shell".to_string(),
            scope: None,
        },
        AgentEvent::PermissionDenied {
            tool_use_id: "t-1".to_string(),
            tool_name: "shell".to_string(),
            reason: "denied".to_string(),
        },
    ];

    for event in &events {
        assert!(
            agent_event_to_session_update(event).is_none(),
            "Expected None for {:?}",
            event
        );
    }
}

#[test]
fn test_agent_error_max_tokens_maps_to_max_tokens() {
    let err = mixtape_core::AgentError::MaxTokensExceeded;
    let result = agent_error_to_stop_reason(&err);
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        agent_client_protocol::StopReason::MaxTokens
    );
}

#[test]
fn test_agent_error_content_filtered_maps_to_refusal() {
    let err = mixtape_core::AgentError::ContentFiltered;
    let result = agent_error_to_stop_reason(&err);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), agent_client_protocol::StopReason::Refusal);
}

#[test]
fn test_agent_error_other_maps_to_internal_error() {
    let err = mixtape_core::AgentError::NoResponse;
    let result = agent_error_to_stop_reason(&err);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Edge cases: verify content fidelity, not just variant shape
// ---------------------------------------------------------------------------

#[test]
fn model_call_streaming_chunk_carries_delta_text() {
    let event = AgentEvent::ModelCallStreaming {
        delta: "streamed text".to_string(),
        accumulated_length: 13,
    };
    let update = agent_event_to_session_update(&event).expect("should produce Some");
    if let SessionUpdate::AgentMessageChunk(chunk) = update {
        // The chunk's content block should be a Text variant containing our delta.
        if let AcpContentBlock::Text(text_content) = chunk.content {
            assert_eq!(text_content.text, "streamed text");
        } else {
            panic!("expected AcpContentBlock::Text inside AgentMessageChunk");
        }
    } else {
        panic!("expected SessionUpdate::AgentMessageChunk");
    }
}

#[test]
fn tool_requested_preserves_tool_use_id() {
    let event = AgentEvent::ToolRequested {
        tool_use_id: "unique-id-abc".to_string(),
        name: "my_tool".to_string(),
        input: serde_json::json!({"key": "value"}),
    };
    let update = agent_event_to_session_update(&event).expect("should produce Some");
    if let SessionUpdate::ToolCall(tool_call) = update {
        assert_eq!(
            tool_call.tool_call_id.to_string(),
            "unique-id-abc",
            "tool_call_id should match the tool_use_id"
        );
    } else {
        panic!("expected SessionUpdate::ToolCall");
    }
}

#[test]
fn tool_failed_embeds_error_message_in_output() {
    let event = AgentEvent::ToolFailed {
        tool_use_id: "fail-id".to_string(),
        name: "my_tool".to_string(),
        error: "permission denied".to_string(),
        duration: Duration::from_millis(10),
    };
    let update = agent_event_to_session_update(&event).expect("should produce Some");
    if let SessionUpdate::ToolCallUpdate(upd) = update {
        let raw_output = upd
            .fields
            .raw_output
            .expect("ToolFailed should set raw_output");
        assert_eq!(
            raw_output,
            serde_json::Value::String("permission denied".to_string()),
            "raw_output should contain the error string"
        );
    } else {
        panic!("expected SessionUpdate::ToolCallUpdate");
    }
}

#[test]
fn agent_error_to_stop_reason_embeds_error_detail_on_failure() {
    // When the error maps to Err, the protocol error's data should include the
    // agent error's display text so operators can diagnose failures.
    let err = mixtape_core::AgentError::ToolNotFound("calculator".to_string());
    let proto_err = agent_error_to_stop_reason(&err).unwrap_err();
    let proto_str = proto_err.to_string();
    assert!(
        proto_str.contains("calculator"),
        "protocol error should embed the agent error text, got: {}",
        proto_str
    );
}

#[test]
fn all_mappable_agent_errors_covered() {
    // Table test: every AgentError variant that should become a stop reason.
    let cases = [
        (
            mixtape_core::AgentError::MaxTokensExceeded,
            agent_client_protocol::StopReason::MaxTokens,
        ),
        (
            mixtape_core::AgentError::ContentFiltered,
            agent_client_protocol::StopReason::Refusal,
        ),
    ];

    for (err, expected_stop_reason) in cases {
        let result = agent_error_to_stop_reason(&err);
        assert!(result.is_ok(), "{:?} should map to a stop reason", err);
        assert_eq!(
            result.unwrap(),
            expected_stop_reason,
            "wrong stop reason for {:?}",
            err
        );
    }
}

// ---------------------------------------------------------------------------
// ToolExecuting preserves tool_use_id in the update
// ---------------------------------------------------------------------------

#[test]
fn tool_executing_preserves_tool_use_id() {
    let event = AgentEvent::ToolExecuting {
        tool_use_id: "exec-id-42".to_string(),
        name: "bash".to_string(),
    };
    let update = agent_event_to_session_update(&event).expect("should produce Some");
    if let SessionUpdate::ToolCallUpdate(upd) = update {
        assert_eq!(
            upd.tool_call_id.to_string(),
            "exec-id-42",
            "ToolExecuting update should carry the tool_use_id"
        );
    } else {
        panic!("expected SessionUpdate::ToolCallUpdate");
    }
}

// ---------------------------------------------------------------------------
// ToolCompleted with non-Text ToolResult variants
//
// The conversion calls ToolResult::as_text(), which produces a descriptive
// string for Image and Json results.  Verify the raw_output reflects that.
// ---------------------------------------------------------------------------

#[test]
fn tool_completed_json_result_serializes_to_json_string() {
    let event = AgentEvent::ToolCompleted {
        tool_use_id: "json-tool-1".to_string(),
        name: "calculate".to_string(),
        output: ToolResult::Json(serde_json::json!({"answer": 42})),
        duration: Duration::from_millis(5),
    };
    let update = agent_event_to_session_update(&event).expect("should produce Some");
    if let SessionUpdate::ToolCallUpdate(upd) = update {
        let raw = upd
            .fields
            .raw_output
            .expect("ToolCompleted must set raw_output");
        // as_text() on Json calls Value::to_string(), so the output is a JSON string
        let raw_str = match &raw {
            serde_json::Value::String(s) => s.clone(),
            other => panic!("expected String, got: {:?}", other),
        };
        assert!(
            raw_str.contains("42"),
            "raw_output should contain the JSON value, got: {}",
            raw_str
        );
    } else {
        panic!("expected SessionUpdate::ToolCallUpdate");
    }
}

#[test]
fn tool_completed_preserves_tool_use_id() {
    let event = AgentEvent::ToolCompleted {
        tool_use_id: "completed-id-7".to_string(),
        name: "read_file".to_string(),
        output: ToolResult::text("contents"),
        duration: Duration::from_millis(1),
    };
    let update = agent_event_to_session_update(&event).expect("should produce Some");
    if let SessionUpdate::ToolCallUpdate(upd) = update {
        assert_eq!(
            upd.tool_call_id.to_string(),
            "completed-id-7",
            "ToolCompleted update should carry the tool_use_id"
        );
    } else {
        panic!("expected SessionUpdate::ToolCallUpdate");
    }
}

// ---------------------------------------------------------------------------
// agent_error_to_stop_reason: non-mappable variants all produce Err
//
// These errors don't have a corresponding ACP stop reason and should be
// forwarded as protocol-level errors so the client surfaces them as failures.
// ---------------------------------------------------------------------------

#[test]
fn non_mappable_agent_errors_all_produce_protocol_error() {
    use mixtape_core::AgentError;

    let cases: &[AgentError] = &[
        AgentError::NoResponse,
        AgentError::EmptyResponse,
        AgentError::ToolDenied("denied".to_string()),
        AgentError::ToolNotFound("missing".to_string()),
        AgentError::InvalidToolInput("bad input".to_string()),
        AgentError::PermissionFailed("no perm".to_string()),
        AgentError::UnexpectedStopReason("weird".to_string()),
    ];

    for err in cases {
        assert!(
            agent_error_to_stop_reason(err).is_err(),
            "{:?} should map to a protocol error, not a stop reason",
            err
        );
    }
}
