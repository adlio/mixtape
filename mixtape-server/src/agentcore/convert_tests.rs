//! Tests for AgentEvent to AgentCoreEvent conversion.

use super::*;
use mixtape_core::events::AgentEvent;
use mixtape_core::tool::ToolResult;
use std::time::{Duration, Instant};

#[test]
fn test_run_started_conversion() {
    let mut ctx = ConversionContext::new();
    let event = AgentEvent::RunStarted {
        input: "Hello".to_string(),
        timestamp: Instant::now(),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], AgentCoreEvent::RunStarted));
}

#[test]
fn test_run_completed_conversion() {
    let mut ctx = ConversionContext::new();

    // Simulate active text stream via ModelCallStarted
    convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 0,
            timestamp: Instant::now(),
        },
        &mut ctx,
    );
    assert!(ctx.in_text_stream());

    let event = AgentEvent::RunCompleted {
        output: "Done".to_string(),
        duration: Duration::from_secs(1),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AgentCoreEvent::RunFinished { response } = &events[0] {
        assert_eq!(response, "Done");
    } else {
        panic!("Expected RunFinished");
    }

    assert!(!ctx.in_text_stream());
}

#[test]
fn test_run_failed_conversion() {
    let mut ctx = ConversionContext::new();
    let event = AgentEvent::RunFailed {
        error: "Out of memory".to_string(),
        duration: Duration::from_secs(5),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AgentCoreEvent::RunError { message } = &events[0] {
        assert_eq!(message, "Out of memory");
    } else {
        panic!("Expected RunError");
    }
}

#[test]
fn test_model_call_started_enables_text_stream() {
    let mut ctx = ConversionContext::new();
    assert!(!ctx.in_text_stream());

    let event = AgentEvent::ModelCallStarted {
        message_count: 1,
        tool_count: 0,
        timestamp: Instant::now(),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 0); // No output event
    assert!(ctx.in_text_stream());
}

#[test]
fn test_streaming_produces_content_delta() {
    let mut ctx = ConversionContext::new();

    // Enable text streaming via ModelCallStarted
    convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 0,
            timestamp: Instant::now(),
        },
        &mut ctx,
    );

    let event = AgentEvent::ModelCallStreaming {
        delta: "Hello".to_string(),
        accumulated_length: 5,
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AgentCoreEvent::ContentDelta { text } = &events[0] {
        assert_eq!(text, "Hello");
    } else {
        panic!("Expected ContentDelta");
    }
}

#[test]
fn test_streaming_without_text_stream_produces_nothing() {
    let mut ctx = ConversionContext::new();

    let event = AgentEvent::ModelCallStreaming {
        delta: "Hello".to_string(),
        accumulated_length: 5,
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 0);
}

#[test]
fn test_model_call_completed_is_silent() {
    let mut ctx = ConversionContext::new();

    // Enable text streaming via ModelCallStarted
    convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 0,
            timestamp: Instant::now(),
        },
        &mut ctx,
    );

    let event = AgentEvent::ModelCallCompleted {
        response_content: "Done".to_string(),
        tokens: None,
        duration: Duration::from_secs(1),
        stop_reason: None,
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 0);
}

#[test]
fn test_tool_requested_produces_three_events() {
    let mut ctx = ConversionContext::new();

    // Enable text streaming via ModelCallStarted
    convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 1,
            timestamp: Instant::now(),
        },
        &mut ctx,
    );

    let event = AgentEvent::ToolRequested {
        tool_use_id: "tc-1".to_string(),
        name: "search".to_string(),
        input: serde_json::json!({"query": "rust"}),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 3);

    // ToolCallStart
    if let AgentCoreEvent::ToolCallStart { tool_call_id, name } = &events[0] {
        assert_eq!(tool_call_id, "tc-1");
        assert_eq!(name, "search");
    } else {
        panic!("Expected ToolCallStart, got {:?}", events[0]);
    }

    // ToolCallInput
    if let AgentCoreEvent::ToolCallInput {
        tool_call_id,
        input,
    } = &events[1]
    {
        assert_eq!(tool_call_id, "tc-1");
        assert_eq!(input, &serde_json::json!({"query": "rust"}));
    } else {
        panic!("Expected ToolCallInput");
    }

    // ToolCallEnd
    if let AgentCoreEvent::ToolCallEnd { tool_call_id } = &events[2] {
        assert_eq!(tool_call_id, "tc-1");
    } else {
        panic!("Expected ToolCallEnd");
    }

    // Text stream should be disabled
    assert!(!ctx.in_text_stream());
}

#[test]
fn test_tool_completed_conversion() {
    let mut ctx = ConversionContext::new();

    let event = AgentEvent::ToolCompleted {
        tool_use_id: "tc-1".to_string(),
        name: "search".to_string(),
        output: ToolResult::Text("42 results found".to_string()),
        duration: Duration::from_millis(100),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AgentCoreEvent::ToolCallResult {
        tool_call_id,
        content,
    } = &events[0]
    {
        assert_eq!(tool_call_id, "tc-1");
        assert_eq!(content, "42 results found");
    } else {
        panic!("Expected ToolCallResult");
    }
}

#[test]
fn test_tool_completed_with_json_result() {
    let mut ctx = ConversionContext::new();

    let event = AgentEvent::ToolCompleted {
        tool_use_id: "tc-1".to_string(),
        name: "api_call".to_string(),
        output: ToolResult::Json(serde_json::json!({"status": "ok", "count": 42})),
        duration: Duration::from_millis(200),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AgentCoreEvent::ToolCallResult { content, .. } = &events[0] {
        // JSON results are serialized to string
        assert!(content.contains("ok") || content.contains("42"));
    } else {
        panic!("Expected ToolCallResult");
    }
}

#[test]
fn test_tool_failed_conversion() {
    let mut ctx = ConversionContext::new();

    let event = AgentEvent::ToolFailed {
        tool_use_id: "tc-1".to_string(),
        name: "risky_tool".to_string(),
        error: "Permission denied".to_string(),
        duration: Duration::from_millis(10),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 1);

    if let AgentCoreEvent::ToolCallError {
        tool_call_id,
        error,
    } = &events[0]
    {
        assert_eq!(tool_call_id, "tc-1");
        assert_eq!(error, "Permission denied");
    } else {
        panic!("Expected ToolCallError");
    }
}

#[test]
fn test_permission_events_are_silent() {
    let mut ctx = ConversionContext::new();

    let silent_events = vec![
        AgentEvent::PermissionRequired {
            proposal_id: "p-1".to_string(),
            tool_name: "tool".to_string(),
            params: serde_json::json!({}),
            params_hash: "hash".to_string(),
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
            "Permission event {:?} should produce no output",
            event
        );
    }
}

#[test]
fn test_tool_executing_is_silent() {
    let mut ctx = ConversionContext::new();

    let event = AgentEvent::ToolExecuting {
        tool_use_id: "tc-1".to_string(),
        name: "tool".to_string(),
    };

    let events = convert_event(&event, &mut ctx);
    assert_eq!(events.len(), 0);
}

#[test]
fn test_full_agent_flow_sequence() {
    let mut ctx = ConversionContext::new();
    let mut all_events = Vec::new();

    // 1. Run starts
    all_events.extend(convert_event(
        &AgentEvent::RunStarted {
            input: "Search for Rust".to_string(),
            timestamp: Instant::now(),
        },
        &mut ctx,
    ));

    // 2. Model starts
    all_events.extend(convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 1,
            timestamp: Instant::now(),
        },
        &mut ctx,
    ));

    // 3. Model streams text
    all_events.extend(convert_event(
        &AgentEvent::ModelCallStreaming {
            delta: "Let me ".to_string(),
            accumulated_length: 7,
        },
        &mut ctx,
    ));
    all_events.extend(convert_event(
        &AgentEvent::ModelCallStreaming {
            delta: "search for that.".to_string(),
            accumulated_length: 23,
        },
        &mut ctx,
    ));

    // 4. Tool requested
    all_events.extend(convert_event(
        &AgentEvent::ToolRequested {
            tool_use_id: "tc-1".to_string(),
            name: "search".to_string(),
            input: serde_json::json!({"query": "Rust"}),
        },
        &mut ctx,
    ));

    // 5. Tool executes
    all_events.extend(convert_event(
        &AgentEvent::ToolExecuting {
            tool_use_id: "tc-1".to_string(),
            name: "search".to_string(),
        },
        &mut ctx,
    ));

    // 6. Tool completes
    all_events.extend(convert_event(
        &AgentEvent::ToolCompleted {
            tool_use_id: "tc-1".to_string(),
            name: "search".to_string(),
            output: ToolResult::Text("Found 42 results".to_string()),
            duration: Duration::from_millis(200),
        },
        &mut ctx,
    ));

    // 7. Model streams more text
    all_events.extend(convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 3,
            tool_count: 1,
            timestamp: Instant::now(),
        },
        &mut ctx,
    ));
    all_events.extend(convert_event(
        &AgentEvent::ModelCallStreaming {
            delta: "I found 42 results.".to_string(),
            accumulated_length: 19,
        },
        &mut ctx,
    ));

    // 8. Run completes
    all_events.extend(convert_event(
        &AgentEvent::RunCompleted {
            output: "I found 42 results.".to_string(),
            duration: Duration::from_secs(2),
        },
        &mut ctx,
    ));

    // Verify event sequence
    assert!(matches!(&all_events[0], AgentCoreEvent::RunStarted));
    assert!(matches!(&all_events[1], AgentCoreEvent::ContentDelta { text } if text == "Let me "));
    assert!(
        matches!(&all_events[2], AgentCoreEvent::ContentDelta { text } if text == "search for that.")
    );
    assert!(
        matches!(&all_events[3], AgentCoreEvent::ToolCallStart { name, .. } if name == "search")
    );
    assert!(matches!(
        &all_events[4],
        AgentCoreEvent::ToolCallInput { .. }
    ));
    assert!(matches!(&all_events[5], AgentCoreEvent::ToolCallEnd { .. }));
    assert!(
        matches!(&all_events[6], AgentCoreEvent::ToolCallResult { content, .. } if content == "Found 42 results")
    );
    assert!(
        matches!(&all_events[7], AgentCoreEvent::ContentDelta { text } if text == "I found 42 results.")
    );
    assert!(
        matches!(&all_events[8], AgentCoreEvent::RunFinished { response } if response == "I found 42 results.")
    );
    assert_eq!(all_events.len(), 9);
}

#[test]
fn test_multiple_tools_in_sequence() {
    let mut ctx = ConversionContext::new();

    // Start text stream
    convert_event(
        &AgentEvent::ModelCallStarted {
            message_count: 1,
            tool_count: 2,
            timestamp: Instant::now(),
        },
        &mut ctx,
    );
    assert!(ctx.in_text_stream());

    // First tool
    let events = convert_event(
        &AgentEvent::ToolRequested {
            tool_use_id: "tc-1".to_string(),
            name: "tool1".to_string(),
            input: serde_json::json!({}),
        },
        &mut ctx,
    );
    assert_eq!(events.len(), 3);
    assert!(!ctx.in_text_stream());

    // Second tool
    let events = convert_event(
        &AgentEvent::ToolRequested {
            tool_use_id: "tc-2".to_string(),
            name: "tool2".to_string(),
            input: serde_json::json!({}),
        },
        &mut ctx,
    );
    assert_eq!(events.len(), 3);
}

#[test]
fn test_context_starts_not_streaming() {
    let ctx = ConversionContext::new();
    assert!(!ctx.in_text_stream());
}
