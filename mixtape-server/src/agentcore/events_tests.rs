//! Tests for AgentCore event types serialization/deserialization.

use super::*;
use serde_json::json;

#[test]
fn test_all_event_types_serialize_with_correct_type_tag() {
    let cases: Vec<(AgentCoreEvent, &str)> = vec![
        (AgentCoreEvent::RunStarted, "run_started"),
        (
            AgentCoreEvent::RunFinished {
                response: "done".to_string(),
            },
            "run_finished",
        ),
        (
            AgentCoreEvent::RunError {
                message: "fail".to_string(),
            },
            "run_error",
        ),
        (
            AgentCoreEvent::ContentDelta {
                text: "hello".to_string(),
            },
            "content_delta",
        ),
        (
            AgentCoreEvent::ToolCallStart {
                tool_call_id: "tc-1".to_string(),
                name: "search".to_string(),
            },
            "tool_call_start",
        ),
        (
            AgentCoreEvent::ToolCallInput {
                tool_call_id: "tc-1".to_string(),
                input: json!({"q": "test"}),
            },
            "tool_call_input",
        ),
        (
            AgentCoreEvent::ToolCallEnd {
                tool_call_id: "tc-1".to_string(),
            },
            "tool_call_end",
        ),
        (
            AgentCoreEvent::ToolCallResult {
                tool_call_id: "tc-1".to_string(),
                content: "result".to_string(),
            },
            "tool_call_result",
        ),
        (
            AgentCoreEvent::ToolCallError {
                tool_call_id: "tc-1".to_string(),
                error: "bad".to_string(),
            },
            "tool_call_error",
        ),
    ];

    for (event, expected_type) in cases {
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains(&format!("\"type\":\"{}\"", expected_type)),
            "Event {:?} should serialize with type {}. Got: {}",
            event,
            expected_type,
            json,
        );
    }
}

#[test]
fn test_all_events_roundtrip() {
    let events = vec![
        AgentCoreEvent::RunStarted,
        AgentCoreEvent::RunFinished {
            response: "Hello world".to_string(),
        },
        AgentCoreEvent::RunError {
            message: "Something went wrong".to_string(),
        },
        AgentCoreEvent::ContentDelta {
            text: "chunk".to_string(),
        },
        AgentCoreEvent::ToolCallStart {
            tool_call_id: "tc-1".to_string(),
            name: "search".to_string(),
        },
        AgentCoreEvent::ToolCallInput {
            tool_call_id: "tc-1".to_string(),
            input: json!({"query": "rust lang", "limit": 10}),
        },
        AgentCoreEvent::ToolCallEnd {
            tool_call_id: "tc-1".to_string(),
        },
        AgentCoreEvent::ToolCallResult {
            tool_call_id: "tc-1".to_string(),
            content: "Found results".to_string(),
        },
        AgentCoreEvent::ToolCallError {
            tool_call_id: "tc-2".to_string(),
            error: "Not found".to_string(),
        },
    ];

    for event in events {
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AgentCoreEvent = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(json, json2, "Roundtrip failed for: {}", json);
    }
}

#[test]
fn test_content_delta_with_special_characters() {
    let special_texts = [
        "Hello \"world\"\n\t\r\\",
        "",
        "x".repeat(10_000).as_str().to_string().leak(),
        "Hello 世界 🌍 Привет",
    ];

    for text in special_texts {
        let event = AgentCoreEvent::ContentDelta {
            text: text.to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: AgentCoreEvent = serde_json::from_str(&json).unwrap();
        if let AgentCoreEvent::ContentDelta { text: deser_text } = deserialized {
            assert_eq!(text, deser_text);
        } else {
            panic!("Wrong event type");
        }
    }
}

#[test]
fn test_tool_call_input_with_complex_json() {
    let complex_input = json!({
        "nested": {
            "array": [1, 2, 3],
            "object": {"key": "value"}
        },
        "string": "test",
        "number": 42,
        "boolean": true,
        "null": null
    });

    let event = AgentCoreEvent::ToolCallInput {
        tool_call_id: "tc-1".to_string(),
        input: complex_input.clone(),
    };

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: AgentCoreEvent = serde_json::from_str(&json).unwrap();

    if let AgentCoreEvent::ToolCallInput { input, .. } = deserialized {
        assert_eq!(input, complex_input);
    } else {
        panic!("Wrong event type");
    }
}

#[test]
fn test_invocation_request_minimal() {
    let json = r#"{"prompt": "Hello"}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "Hello");
}

#[test]
fn test_invocation_request_empty_prompt() {
    let json = r#"{"prompt": ""}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "");
}

#[test]
fn test_invocation_request_missing_prompt() {
    let json = r#"{}"#;
    let result: Result<InvocationRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn test_invocation_request_with_extra_fields() {
    // AgentCore may forward additional fields; we should ignore them gracefully
    let json = r#"{"prompt": "Hello", "session_id": "s-123", "extra": true}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "Hello");
}

#[test]
fn test_invocation_request_with_unicode() {
    let json = r#"{"prompt": "Hello 世界 🌍"}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "Hello 世界 🌍");
}

#[test]
fn test_invocation_request_large_prompt() {
    let large_prompt = "x".repeat(100_000);
    let json = format!(r#"{{"prompt": "{}"}}"#, large_prompt);
    let request: InvocationRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(request.prompt.len(), 100_000);
}

#[test]
fn test_ping_response_serialization() {
    let response = PingResponse {
        status: "Healthy".to_string(),
        time_of_last_update: 1700000000,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"status\":\"Healthy\""));
    assert!(json.contains("\"time_of_last_update\":1700000000"));
}

#[test]
fn test_ping_response_healthy_busy() {
    let response = PingResponse {
        status: "HealthyBusy".to_string(),
        time_of_last_update: 1700000000,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"status\":\"HealthyBusy\""));
}
