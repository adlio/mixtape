//! Comprehensive tests for AgentEvent to AG-UI event conversion.
//!
//! These tests verify message boundary management and edge cases.

use super::*;
use mixtape_core::events::AgentEvent;
use mixtape_core::tool::ToolResult;
use std::time::{Duration, Instant};

#[test]
fn test_multiple_sequential_model_calls() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // First model call
    let start1 = AgentEvent::ModelCallStarted {
        message_count: 1,
        tool_count: 0,
        timestamp: Instant::now(),
    };
    let events = convert_event(&start1, &mut ctx);
    assert_eq!(events.len(), 1);
    let _first_msg_id = if let AguiEvent::TextMessageStart { message_id, .. } = &events[0] {
        message_id.clone()
    } else {
        panic!("Expected TextMessageStart");
    };

    // Second model call should end the first message
    let start2 = AgentEvent::ModelCallStarted {
        message_count: 2,
        tool_count: 0,
        timestamp: Instant::now(),
    };
    let events = convert_event(&start2, &mut ctx);

    // Should produce: TextMessageEnd for first message, TextMessageStart for second
    assert_eq!(events.len(), 1, "Second ModelCallStarted should only start new message, not end previous (that's handled by tool calls or RunCompleted)");

    // Verify new message was started
    assert!(matches!(&events[0], AguiEvent::TextMessageStart { .. }));
}

#[test]
fn test_tool_requested_ends_current_message() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // Start a message
    let start = AgentEvent::ModelCallStarted {
        message_count: 1,
        tool_count: 0,
        timestamp: Instant::now(),
    };
    convert_event(&start, &mut ctx);
    assert!(ctx.current_message_id().is_some());

    // Tool request should end the message
    let tool_req = AgentEvent::ToolRequested {
        tool_use_id: "tc-1".to_string(),
        name: "echo".to_string(),
        input: serde_json::json!({"text": "hello"}),
    };
    let events = convert_event(&tool_req, &mut ctx);

    // Should end message, then emit 3 tool events
    assert!(events.len() >= 4);
    assert!(matches!(&events[0], AguiEvent::TextMessageEnd { .. }));
    assert!(matches!(&events[1], AguiEvent::ToolCallStart { .. }));
    assert!(ctx.current_message_id().is_none());
}

#[test]
fn test_tool_requested_without_current_message() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // Tool request with no active message (edge case)
    let tool_req = AgentEvent::ToolRequested {
        tool_use_id: "tc-1".to_string(),
        name: "echo".to_string(),
        input: serde_json::json!({"text": "hello"}),
    };
    let events = convert_event(&tool_req, &mut ctx);

    // Should only emit tool events, no TextMessageEnd
    assert_eq!(events.len(), 3); // Start, Args, End
    assert!(matches!(&events[0], AguiEvent::ToolCallStart { .. }));
    assert!(matches!(&events[1], AguiEvent::ToolCallArgs { .. }));
    assert!(matches!(&events[2], AguiEvent::ToolCallEnd { .. }));
}

#[test]
fn test_run_completed_ends_current_message() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // Start a message
    let start = AgentEvent::ModelCallStarted {
        message_count: 1,
        tool_count: 0,
        timestamp: Instant::now(),
    };
    convert_event(&start, &mut ctx);
    assert!(ctx.current_message_id().is_some());

    // RunCompleted should end the message
    let completed = AgentEvent::RunCompleted {
        output: "Done".to_string(),
        duration: Duration::from_secs(1),
    };
    let events = convert_event(&completed, &mut ctx);

    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], AguiEvent::TextMessageEnd { .. }));
    assert!(matches!(&events[1], AguiEvent::RunFinished { .. }));
    assert!(ctx.current_message_id().is_none());
}

#[test]
fn test_run_completed_without_current_message() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // RunCompleted with no active message
    let completed = AgentEvent::RunCompleted {
        output: "Done".to_string(),
        duration: Duration::from_secs(1),
    };
    let events = convert_event(&completed, &mut ctx);

    // Should only emit RunFinished
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], AguiEvent::RunFinished { .. }));
}

#[test]
fn test_streaming_without_current_message() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // Streaming event without active message (shouldn't happen, but test graceful handling)
    let streaming = AgentEvent::ModelCallStreaming {
        delta: "Hello".to_string(),
        accumulated_length: 5,
    };
    let events = convert_event(&streaming, &mut ctx);

    // Should return empty vec, not crash
    assert_eq!(events.len(), 0);
}

#[test]
fn test_empty_streaming_delta() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // Start message
    let start = AgentEvent::ModelCallStarted {
        message_count: 1,
        tool_count: 0,
        timestamp: Instant::now(),
    };
    convert_event(&start, &mut ctx);

    // Empty delta should still produce event
    let streaming = AgentEvent::ModelCallStreaming {
        delta: "".to_string(),
        accumulated_length: 0,
    };
    let events = convert_event(&streaming, &mut ctx);

    assert_eq!(events.len(), 1);
    if let AguiEvent::TextMessageContent { delta, .. } = &events[0] {
        assert_eq!(delta, "");
    } else {
        panic!("Expected TextMessageContent");
    }
}

#[test]
fn test_tool_completed_with_different_result_types() {
    let test_cases = [
        (ToolResult::Text("Success".to_string()), "Success"),
        (
            ToolResult::Json(serde_json::json!({"status": "ok", "count": 42})),
            r#"{"count":42,"status":"ok"}"#, // Note: JSON objects are sorted by key
        ),
        (
            ToolResult::Image {
                format: mixtape_core::tool::ImageFormat::Png,
                data: vec![0x89, 0x50, 0x4E, 0x47], // PNG header
            },
            "[Image: Png, 4 bytes]",
        ),
        (
            ToolResult::Document {
                format: mixtape_core::tool::DocumentFormat::Pdf,
                data: vec![0x25, 0x50, 0x44, 0x46], // PDF header
                name: Some("report.pdf".to_string()),
            },
            "[Document: Pdf, report.pdf, 4 bytes]",
        ),
        (
            ToolResult::Document {
                format: mixtape_core::tool::DocumentFormat::Txt,
                data: vec![0x48, 0x69], // "Hi"
                name: None,
            },
            "[Document: Txt, unnamed, 2 bytes]",
        ),
    ];

    for (result, expected_content) in test_cases {
        let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

        let event = AgentEvent::ToolCompleted {
            tool_use_id: "tc-1".to_string(),
            name: "test_tool".to_string(),
            output: result,
            duration: Duration::from_millis(100),
        };

        let events = convert_event(&event, &mut ctx);
        assert_eq!(events.len(), 1);

        if let AguiEvent::ToolCallResult { content, .. } = &events[0] {
            assert_eq!(content, expected_content);
        } else {
            panic!("Expected ToolCallResult");
        }
    }
}

#[test]
fn test_tool_failed_error_formatting() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    let event = AgentEvent::ToolFailed {
        tool_use_id: "tc-1".to_string(),
        name: "dangerous_tool".to_string(),
        error: "Permission denied".to_string(),
        duration: Duration::from_millis(10),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AguiEvent::ToolCallResult { content, .. } = &events[0] {
        assert_eq!(content, "Error: Permission denied");
    } else {
        panic!("Expected ToolCallResult");
    }
}

#[test]
fn test_tool_call_args_with_complex_json() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    let complex_input = serde_json::json!({
        "nested": {
            "array": [1, 2, 3],
            "object": {"key": "value"}
        },
        "string": "test",
        "number": 42,
        "boolean": true,
        "null": null
    });

    let event = AgentEvent::ToolRequested {
        tool_use_id: "tc-1".to_string(),
        name: "complex_tool".to_string(),
        input: complex_input.clone(),
    };

    let events = convert_event(&event, &mut ctx);

    // Find the ToolCallArgs event
    let args_event = events
        .iter()
        .find(|e| matches!(e, AguiEvent::ToolCallArgs { .. }));
    assert!(args_event.is_some());

    if let AguiEvent::ToolCallArgs { delta, .. } = args_event.unwrap() {
        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(delta).unwrap();
        assert_eq!(parsed, complex_input);
    }
}

#[test]
fn test_tool_call_args_serialization_fallback() {
    // This tests the unwrap_or_default() on line 132 of convert.rs
    // We can't easily trigger a serialization error with Value, but we verify
    // the happy path works correctly
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    let event = AgentEvent::ToolRequested {
        tool_use_id: "tc-1".to_string(),
        name: "tool".to_string(),
        input: serde_json::json!({"key": "value"}),
    };

    let events = convert_event(&event, &mut ctx);

    // Should produce valid JSON delta
    let args_event = events
        .iter()
        .find(|e| matches!(e, AguiEvent::ToolCallArgs { .. }));
    if let Some(AguiEvent::ToolCallArgs { delta, .. }) = args_event {
        assert!(serde_json::from_str::<serde_json::Value>(delta).is_ok());
    }
}

#[test]
fn test_permission_required_with_special_characters_in_params() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    let event = AgentEvent::PermissionRequired {
        proposal_id: "prop-1".to_string(),
        tool_name: "shell".to_string(),
        params: serde_json::json!({
            "cmd": "echo \"Hello\\nWorld\"",
            "special": "chars: \t\r\n"
        }),
        params_hash: "hash123".to_string(),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AguiEvent::Interrupt { data, .. } = &events[0] {
        // Verify special characters are preserved
        assert_eq!(data.params["cmd"], "echo \"Hello\\nWorld\"");
        assert_eq!(data.params["special"], "chars: \t\r\n");
    } else {
        panic!("Expected Interrupt event");
    }
}

#[test]
fn test_multiple_tools_in_sequence() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    // Start message
    convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 2,
            timestamp: Instant::now(),
        },
        &mut ctx,
    );

    // First tool
    let tool1 = AgentEvent::ToolRequested {
        tool_use_id: "tc-1".to_string(),
        name: "tool1".to_string(),
        input: serde_json::json!({}),
    };
    convert_event(&tool1, &mut ctx);
    assert!(ctx.current_message_id().is_none()); // Message ended by tool

    // First tool completes
    let complete1 = AgentEvent::ToolCompleted {
        tool_use_id: "tc-1".to_string(),
        name: "tool1".to_string(),
        output: ToolResult::Text("Result 1".to_string()),
        duration: Duration::from_millis(100),
    };
    convert_event(&complete1, &mut ctx);

    // Second tool
    let tool2 = AgentEvent::ToolRequested {
        tool_use_id: "tc-2".to_string(),
        name: "tool2".to_string(),
        input: serde_json::json!({}),
    };
    let events = convert_event(&tool2, &mut ctx);

    // Should not try to end a message (none is active)
    let has_message_end = events
        .iter()
        .any(|e| matches!(e, AguiEvent::TextMessageEnd { .. }));
    assert!(
        !has_message_end,
        "Should not emit TextMessageEnd when no message is active"
    );
}

#[test]
fn test_silent_events_produce_no_output() {
    let mut ctx = ConversionContext::new("thread-1".to_string(), "run-1".to_string());

    let silent_events = vec![
        AgentEvent::ToolExecuting {
            tool_use_id: "tc-1".to_string(),
            name: "tool".to_string(),
        },
        AgentEvent::ModelCallCompleted {
            response_content: "Done".to_string(),
            tokens: None,
            duration: Duration::from_secs(1),
            stop_reason: None,
        },
        AgentEvent::PermissionGranted {
            tool_use_id: "tc-1".to_string(),
            tool_name: "tool".to_string(),
            scope: None,
        },
        AgentEvent::PermissionDenied {
            tool_use_id: "tc-1".to_string(),
            tool_name: "tool".to_string(),
            reason: "denied".to_string(),
        },
    ];

    for event in silent_events {
        let events = convert_event(&event, &mut ctx);
        assert_eq!(
            events.len(),
            0,
            "Event {:?} should produce no AG-UI events",
            event
        );
    }
}

#[test]
fn test_context_thread_and_run_ids() {
    let mut ctx = ConversionContext::new("custom-thread".to_string(), "custom-run".to_string());

    let event = AgentEvent::RunStarted {
        input: "test".to_string(),
        timestamp: Instant::now(),
    };

    let events = convert_event(&event, &mut ctx);

    if let AguiEvent::RunStarted { thread_id, run_id } = &events[0] {
        assert_eq!(thread_id, "custom-thread");
        assert_eq!(run_id, "custom-run");
    } else {
        panic!("Expected RunStarted");
    }
}

#[test]
fn test_conversion_context_message_id_operations() {
    let mut ctx = ConversionContext::new("t1".to_string(), "r1".to_string());

    // Initially no message ID
    assert!(ctx.current_message_id().is_none());

    // Set message ID
    ctx.set_current_message_id("msg-1".to_string());
    assert_eq!(ctx.current_message_id(), Some("msg-1"));

    // Take message ID (removes it)
    let id = ctx.take_current_message_id();
    assert_eq!(id, Some("msg-1".to_string()));
    assert!(ctx.current_message_id().is_none());

    // Take from empty returns None
    let id = ctx.take_current_message_id();
    assert!(id.is_none());
}
