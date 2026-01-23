//! Comprehensive tests for AG-UI event serialization/deserialization.
//!
//! These tests verify the external API contract between the server and frontend.

use super::*;
use serde_json::json;

#[test]
fn test_all_lifecycle_events_serialization() {
    let cases = [
        (
            AguiEvent::RunStarted {
                thread_id: "t1".to_string(),
                run_id: "r1".to_string(),
            },
            "RUN_STARTED",
        ),
        (
            AguiEvent::RunFinished {
                thread_id: "t1".to_string(),
                run_id: "r1".to_string(),
            },
            "RUN_FINISHED",
        ),
        (
            AguiEvent::RunError {
                message: "failure".to_string(),
                code: None,
            },
            "RUN_ERROR",
        ),
        (
            AguiEvent::RunError {
                message: "failure".to_string(),
                code: Some("E001".to_string()),
            },
            "RUN_ERROR",
        ),
    ];

    for (event, expected_type) in cases {
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains(&format!("\"type\":\"{}\"", expected_type)),
            "Event {:?} should serialize with type {}",
            event,
            expected_type
        );
    }
}

#[test]
fn test_all_message_events_roundtrip() {
    let events = vec![
        AguiEvent::TextMessageStart {
            message_id: "msg-1".to_string(),
            role: MessageRole::Assistant,
        },
        AguiEvent::TextMessageContent {
            message_id: "msg-1".to_string(),
            delta: "Hello world".to_string(),
        },
        AguiEvent::TextMessageEnd {
            message_id: "msg-1".to_string(),
        },
    ];

    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();

        // Verify type preservation through roundtrip
        match (&event, &deserialized) {
            (AguiEvent::TextMessageStart { .. }, AguiEvent::TextMessageStart { .. }) => {}
            (AguiEvent::TextMessageContent { .. }, AguiEvent::TextMessageContent { .. }) => {}
            (AguiEvent::TextMessageEnd { .. }, AguiEvent::TextMessageEnd { .. }) => {}
            _ => panic!("Event type changed during roundtrip"),
        }
    }
}

#[test]
fn test_tool_call_events_complete_sequence() {
    let cases = [
        (
            AguiEvent::ToolCallStart {
                tool_call_id: "tc-1".to_string(),
                tool_call_name: "echo".to_string(),
                parent_message_id: None,
            },
            "TOOL_CALL_START",
        ),
        (
            AguiEvent::ToolCallStart {
                tool_call_id: "tc-1".to_string(),
                tool_call_name: "echo".to_string(),
                parent_message_id: Some("msg-1".to_string()),
            },
            "TOOL_CALL_START",
        ),
        (
            AguiEvent::ToolCallArgs {
                tool_call_id: "tc-1".to_string(),
                delta: r#"{"arg":"value"}"#.to_string(),
            },
            "TOOL_CALL_ARGS",
        ),
        (
            AguiEvent::ToolCallEnd {
                tool_call_id: "tc-1".to_string(),
            },
            "TOOL_CALL_END",
        ),
        (
            AguiEvent::ToolCallResult {
                message_id: "result-1".to_string(),
                tool_call_id: "tc-1".to_string(),
                content: "Success".to_string(),
                role: Some(MessageRole::Tool),
            },
            "TOOL_CALL_RESULT",
        ),
        (
            AguiEvent::ToolCallResult {
                message_id: "result-1".to_string(),
                tool_call_id: "tc-1".to_string(),
                content: "Success".to_string(),
                role: None,
            },
            "TOOL_CALL_RESULT",
        ),
    ];

    for (event, expected_type) in cases {
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains(&format!("\"type\":\"{}\"", expected_type)),
            "Event {:?} should serialize with type {}",
            event,
            expected_type
        );
    }
}

#[test]
fn test_state_events_with_complex_data() {
    // Snapshot with nested JSON
    let snapshot = json!({
        "users": [
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"}
        ],
        "count": 2
    });

    let event = AguiEvent::StateSnapshot {
        snapshot: snapshot.clone(),
    };

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();

    if let AguiEvent::StateSnapshot {
        snapshot: deser_snapshot,
    } = deserialized
    {
        assert_eq!(snapshot, deser_snapshot);
    } else {
        panic!("Wrong event type after deserialization");
    }
}

#[test]
fn test_state_delta_with_json_patch_ops() {
    let delta_ops = vec![
        JsonPatchOp {
            op: "add".to_string(),
            path: "/users/2".to_string(),
            value: Some(json!({"id": 3, "name": "Charlie"})),
        },
        JsonPatchOp {
            op: "remove".to_string(),
            path: "/users/0".to_string(),
            value: None,
        },
        JsonPatchOp {
            op: "replace".to_string(),
            path: "/count".to_string(),
            value: Some(json!(2)),
        },
    ];

    let event = AguiEvent::StateDelta { delta: delta_ops };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"STATE_DELTA\""));

    // Verify roundtrip
    let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();
    if let AguiEvent::StateDelta { delta } = deserialized {
        assert_eq!(delta.len(), 3);
        assert_eq!(delta[0].op, "add");
        assert_eq!(delta[1].op, "remove");
        assert_eq!(delta[2].op, "replace");
        assert!(delta[1].value.is_none()); // remove has no value
    } else {
        panic!("Wrong event type after deserialization");
    }
}

#[test]
fn test_interrupt_event_serialization() {
    let event = AguiEvent::Interrupt {
        interrupt_id: "int-1".to_string(),
        interrupt_type: InterruptType::ToolApproval,
        data: InterruptData {
            tool_use_id: "tu-1".to_string(),
            tool_name: "dangerous_cmd".to_string(),
            params: json!({"cmd": "rm -rf /"}),
            params_hash: "abc123".to_string(),
        },
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"INTERRUPT\""));
    assert!(json.contains("\"interrupt_type\":\"tool_approval\""));
    assert!(json.contains("dangerous_cmd"));
}

#[test]
fn test_message_role_all_variants() {
    let cases = [
        (MessageRole::User, "user"),
        (MessageRole::Assistant, "assistant"),
        (MessageRole::System, "system"),
        (MessageRole::Tool, "tool"),
    ];

    for (role, expected_str) in cases {
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, format!("\"{}\"", expected_str));

        // Roundtrip
        let deserialized: MessageRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, deserialized);
    }
}

#[test]
fn test_interrupt_response_all_variants() {
    // Test approve_once
    let json = r#"{"action":"approve_once"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(response, InterruptResponse::ApproveOnce));

    // Test trust_tool with session scope
    let json = r#"{"action":"trust_tool","scope":"session"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(
        response,
        InterruptResponse::TrustTool {
            scope: GrantScope::Session
        }
    ));

    // Test trust_tool with persistent scope
    let json = r#"{"action":"trust_tool","scope":"persistent"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(
        response,
        InterruptResponse::TrustTool {
            scope: GrantScope::Persistent
        }
    ));

    // Test trust_exact with session scope
    let json = r#"{"action":"trust_exact","scope":"session"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(
        response,
        InterruptResponse::TrustExact {
            scope: GrantScope::Session
        }
    ));

    // Test trust_exact with persistent scope
    let json = r#"{"action":"trust_exact","scope":"persistent"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(
        response,
        InterruptResponse::TrustExact {
            scope: GrantScope::Persistent
        }
    ));

    // Test deny without reason
    let json = r#"{"action":"deny"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(response, InterruptResponse::Deny { reason: None }));

    // Test deny with reason
    let json = r#"{"action":"deny","reason":"Too dangerous"}"#;
    let response: InterruptResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(
        response,
        InterruptResponse::Deny { reason: Some(_) }
    ));
}

#[test]
fn test_grant_scope_all_variants() {
    let cases = [
        (GrantScope::Session, "session"),
        (GrantScope::Persistent, "persistent"),
    ];

    for (scope, expected_str) in cases {
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(json, format!("\"{}\"", expected_str));

        // Roundtrip
        let deserialized: GrantScope = serde_json::from_str(&json).unwrap();
        assert_eq!(scope, deserialized);
    }
}

#[test]
fn test_event_with_empty_strings() {
    // Empty strings should be valid
    let event = AguiEvent::TextMessageContent {
        message_id: "".to_string(),
        delta: "".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();

    if let AguiEvent::TextMessageContent { message_id, delta } = deserialized {
        assert_eq!(message_id, "");
        assert_eq!(delta, "");
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_event_with_special_characters() {
    // Test special characters that need escaping in JSON
    let special_chars = "Hello \"world\"\n\t\r\\slash/forward";

    let event = AguiEvent::TextMessageContent {
        message_id: "msg-1".to_string(),
        delta: special_chars.to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();

    if let AguiEvent::TextMessageContent { delta, .. } = deserialized {
        assert_eq!(delta, special_chars);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_event_with_unicode() {
    // Test Unicode characters
    let unicode_text = "Hello 世界 🌍 Привет مرحبا";

    let event = AguiEvent::TextMessageContent {
        message_id: "msg-1".to_string(),
        delta: unicode_text.to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();

    if let AguiEvent::TextMessageContent { delta, .. } = deserialized {
        assert_eq!(delta, unicode_text);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_event_with_very_long_strings() {
    // Test handling of large content
    let large_delta = "x".repeat(10_000);

    let event = AguiEvent::TextMessageContent {
        message_id: "msg-1".to_string(),
        delta: large_delta.clone(),
    };

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: AguiEvent = serde_json::from_str(&json).unwrap();

    if let AguiEvent::TextMessageContent { delta, .. } = deserialized {
        assert_eq!(delta.len(), 10_000);
        assert_eq!(delta, large_delta);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_skip_serializing_if_behavior() {
    // Test that None fields are omitted from JSON
    let event = AguiEvent::RunError {
        message: "error".to_string(),
        code: None,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(
        !json.contains("\"code\""),
        "None code should be omitted from JSON"
    );

    // Test that Some fields are included
    let event = AguiEvent::RunError {
        message: "error".to_string(),
        code: Some("E001".to_string()),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(
        json.contains("\"code\":\"E001\""),
        "Some code should be included in JSON"
    );
}

#[test]
fn test_malformed_interrupt_response_fails_gracefully() {
    let bad_json_cases = [
        r#"{"action":"unknown_action"}"#, // Invalid action
        r#"{"action":"trust_tool"}"#,     // Missing required scope
        r#"{"action":"trust_exact"}"#,    // Missing required scope
        r#"{}"#,                          // Missing action
        r#"{"scope":"session"}"#,         // Action without scope
    ];

    for bad_json in bad_json_cases {
        let result: Result<InterruptResponse, _> = serde_json::from_str(bad_json);
        assert!(result.is_err(), "Should fail to deserialize: {}", bad_json);
    }
}
