//! Tests for AG-UI HTTP handlers focusing on error paths and edge cases.

use super::*;

#[test]
fn test_agent_request_all_fields() {
    let json = r#"{
        "message": "Hello",
        "thread_id": "thread-123",
        "run_id": "run-456",
        "options": {"stream": false}
    }"#;

    let request: AgentRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.message, "Hello");
    assert_eq!(request.thread_id, Some("thread-123".to_string()));
    assert_eq!(request.run_id, Some("run-456".to_string()));
    assert!(!request.options.stream);
}

#[test]
fn test_agent_request_minimal() {
    let json = r#"{"message": "Hello"}"#;
    let request: AgentRequest = serde_json::from_str(json).unwrap();

    assert_eq!(request.message, "Hello");
    assert!(request.thread_id.is_none());
    assert!(request.run_id.is_none());
    assert!(request.options.stream); // default is true
}

#[test]
fn test_agent_request_empty_message() {
    // Empty message is valid (let agent decide if it's an error)
    let json = r#"{"message": ""}"#;
    let request: AgentRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.message, "");
}

#[test]
fn test_agent_request_missing_message_field() {
    let json = r#"{"thread_id": "thread-123"}"#;
    let result: Result<AgentRequest, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "Should fail without required message field"
    );
}

#[test]
fn test_agent_request_with_special_characters() {
    let json = r#"{"message": "Hello\n\"World\"\t\r"}"#;
    let request: AgentRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.message, "Hello\n\"World\"\t\r");
}

#[test]
fn test_agent_request_with_unicode() {
    let json = r#"{"message": "Hello 世界 🌍"}"#;
    let request: AgentRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.message, "Hello 世界 🌍");
}

#[test]
fn test_run_options_default() {
    let options = RunOptions::default();
    assert!(options.stream);
}

#[test]
fn test_interrupt_request_approve_once() {
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "echo",
        "response": {"action": "approve_once"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.interrupt_id, "int-1");
    assert_eq!(request.tool_name, "echo");
    assert!(request.params_hash.is_none());
    assert!(matches!(request.response, InterruptResponse::ApproveOnce));
}

#[test]
fn test_interrupt_request_trust_tool_session() {
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "safe_tool",
        "response": {"action": "trust_tool", "scope": "session"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(
        request.response,
        InterruptResponse::TrustTool {
            scope: GrantScope::Session
        }
    ));
}

#[test]
fn test_interrupt_request_trust_tool_persistent() {
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "safe_tool",
        "response": {"action": "trust_tool", "scope": "persistent"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(
        request.response,
        InterruptResponse::TrustTool {
            scope: GrantScope::Persistent
        }
    ));
}

#[test]
fn test_interrupt_request_trust_exact_with_hash() {
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "cmd",
        "params_hash": "abc123",
        "response": {"action": "trust_exact", "scope": "session"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.params_hash, Some("abc123".to_string()));
    assert!(matches!(
        request.response,
        InterruptResponse::TrustExact {
            scope: GrantScope::Session
        }
    ));
}

#[test]
fn test_interrupt_request_trust_exact_without_hash() {
    // This should deserialize fine - the handler will catch the missing hash
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "cmd",
        "response": {"action": "trust_exact", "scope": "session"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert!(request.params_hash.is_none());
    assert!(matches!(
        request.response,
        InterruptResponse::TrustExact { .. }
    ));
}

#[test]
fn test_interrupt_request_deny_without_reason() {
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "dangerous",
        "response": {"action": "deny"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(
        request.response,
        InterruptResponse::Deny { reason: None }
    ));
}

#[test]
fn test_interrupt_request_deny_with_reason() {
    let json = r#"{
        "interrupt_id": "int-1",
        "tool_name": "dangerous",
        "response": {"action": "deny", "reason": "Too risky"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    if let InterruptResponse::Deny { reason } = request.response {
        assert_eq!(reason, Some("Too risky".to_string()));
    } else {
        panic!("Expected Deny response");
    }
}

#[test]
fn test_interrupt_request_malformed() {
    let bad_cases = vec![
        r#"{}"#,                                             // Missing all fields
        r#"{"interrupt_id": "int-1"}"#,                      // Missing tool_name and response
        r#"{"interrupt_id": "int-1", "tool_name": "echo"}"#, // Missing response
        r#"{"response": {"action": "approve_once"}}"#,       // Missing interrupt_id and tool_name
    ];

    for bad_json in bad_cases {
        let result: Result<InterruptRequest, _> = serde_json::from_str(bad_json);
        assert!(
            result.is_err(),
            "Should reject malformed request: {}",
            bad_json
        );
    }
}

#[test]
fn test_scope_conversion_all_variants() {
    use super::convert_scope;

    let cases = [
        (
            GrantScope::Session,
            mixtape_core::permission::Scope::Session,
        ),
        (
            GrantScope::Persistent,
            mixtape_core::permission::Scope::Persistent,
        ),
    ];

    for (agui_scope, core_scope) in cases {
        let converted = convert_scope(agui_scope);
        assert!(
            matches!(
                (&converted, &core_scope),
                (
                    mixtape_core::permission::Scope::Session,
                    mixtape_core::permission::Scope::Session
                ) | (
                    mixtape_core::permission::Scope::Persistent,
                    mixtape_core::permission::Scope::Persistent
                )
            ),
            "Scope conversion failed for {:?}",
            agui_scope
        );
    }
}

#[test]
fn test_interrupt_request_empty_strings() {
    // Empty strings should be valid (though semantically wrong)
    let json = r#"{
        "interrupt_id": "",
        "tool_name": "",
        "response": {"action": "approve_once"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.interrupt_id, "");
    assert_eq!(request.tool_name, "");
}

#[test]
fn test_interrupt_request_with_special_characters() {
    let json = r#"{
        "interrupt_id": "int-\"123\"",
        "tool_name": "tool\nname",
        "params_hash": "hash\t123",
        "response": {"action": "approve_once"}
    }"#;

    let request: InterruptRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.interrupt_id, "int-\"123\"");
    assert_eq!(request.tool_name, "tool\nname");
    assert_eq!(request.params_hash, Some("hash\t123".to_string()));
}

#[test]
fn test_agent_request_large_message() {
    // Test with very large message
    let large_message = "x".repeat(100_000);
    let json = format!(r#"{{"message": "{}"}}"#, large_message);

    let request: AgentRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(request.message.len(), 100_000);
    assert_eq!(request.message, large_message);
}

#[test]
fn test_interrupt_request_deny_with_long_reason() {
    let long_reason = "r".repeat(10_000);
    let json = format!(
        r#"{{
            "interrupt_id": "int-1",
            "tool_name": "tool",
            "response": {{"action": "deny", "reason": "{}"}}
        }}"#,
        long_reason
    );

    let request: InterruptRequest = serde_json::from_str(&json).unwrap();
    if let InterruptResponse::Deny { reason } = request.response {
        assert_eq!(reason.unwrap().len(), 10_000);
    } else {
        panic!("Expected Deny response");
    }
}

#[test]
fn test_run_options_explicit_stream_false() {
    let json = r#"{"stream": false}"#;
    let options: RunOptions = serde_json::from_str(json).unwrap();
    assert!(!options.stream);
}

#[test]
fn test_run_options_explicit_stream_true() {
    let json = r#"{"stream": true}"#;
    let options: RunOptions = serde_json::from_str(json).unwrap();
    assert!(options.stream);
}

#[test]
fn test_run_options_empty_object() {
    let json = r#"{}"#;
    let options: RunOptions = serde_json::from_str(json).unwrap();
    assert!(options.stream); // default is true
}

#[test]
fn test_agent_request_null_optional_fields() {
    // Test with null for Option<String> fields (these work with null)
    let json = r#"{
        "message": "Hello",
        "thread_id": null,
        "run_id": null
    }"#;

    let request: AgentRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.message, "Hello");
    assert!(request.thread_id.is_none());
    assert!(request.run_id.is_none());
    // options should use default when omitted
    assert!(request.options.stream);
}
