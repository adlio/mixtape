//! Tests for AgentCore HTTP handler request/response types.

use super::*;

#[test]
fn test_invocation_request_deserialize() {
    let json = r#"{"prompt": "Hello, agent!"}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "Hello, agent!");
}

#[test]
fn test_invocation_request_missing_prompt() {
    let json = r#"{}"#;
    let result: Result<InvocationRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn test_invocation_request_extra_fields_ignored() {
    // AgentCore may forward additional payload fields
    let json = r#"{"prompt": "Hello", "model": "claude", "temperature": 0.7}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "Hello");
}

#[test]
fn test_invocation_request_with_special_characters() {
    let json = r#"{"prompt": "Hello\n\"World\"\t\r"}"#;
    let request: InvocationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.prompt, "Hello\n\"World\"\t\r");
}

#[test]
fn test_session_id_header_constant() {
    assert_eq!(
        SESSION_ID_HEADER,
        "x-amzn-bedrock-agentcore-runtime-session-id"
    );
}

#[test]
fn test_user_id_header_constant() {
    assert_eq!(USER_ID_HEADER, "x-amzn-bedrock-agentcore-runtime-user-id");
}
