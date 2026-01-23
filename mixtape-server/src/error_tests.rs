//! Tests for error handling and IntoResponse implementation.

use crate::error::*;
use axum::{http::StatusCode, response::IntoResponse};

#[test]
fn test_server_error_agent_variant() {
    // Create a mock agent error
    let agent_error = mixtape_core::AgentError::NoResponse;
    let server_error = ServerError::Agent(agent_error);

    let response = server_error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::INTERNAL_SERVER_ERROR);

    // Verify body format (need to consume body to check)
    // For now, just verify status code
}

#[test]
fn test_server_error_permission_variant() {
    let error = ServerError::Permission("Access denied".to_string());

    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::FORBIDDEN);
}

#[test]
fn test_server_error_invalid_request_variant() {
    let error = ServerError::InvalidRequest("Bad input".to_string());

    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::BAD_REQUEST);
}

#[test]
fn test_server_error_internal_variant() {
    let error = ServerError::Internal("Something went wrong".to_string());

    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn test_server_error_display() {
    let cases = [
        (
            ServerError::Permission("denied".to_string()),
            "Permission error: denied",
        ),
        (
            ServerError::InvalidRequest("bad".to_string()),
            "Invalid request: bad",
        ),
        (
            ServerError::Internal("oops".to_string()),
            "Internal error: oops",
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn test_server_error_from_agent_error() {
    let agent_error = mixtape_core::AgentError::EmptyResponse;
    let server_error: ServerError = agent_error.into();

    assert!(matches!(server_error, ServerError::Agent(_)));
}

#[test]
fn test_server_error_permission_with_empty_message() {
    let error = ServerError::Permission("".to_string());
    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::FORBIDDEN);
}

#[test]
fn test_server_error_invalid_request_with_special_characters() {
    let error = ServerError::InvalidRequest("Bad input: \"value\"\n\t".to_string());
    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::BAD_REQUEST);
    // The message should be properly escaped in the JSON response
}

#[test]
fn test_server_error_with_very_long_message() {
    let long_message = "x".repeat(10_000);
    let error = ServerError::Internal(long_message.clone());

    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::INTERNAL_SERVER_ERROR);
    // Should handle large error messages without panicking
}

#[test]
fn test_server_error_with_unicode() {
    let error = ServerError::InvalidRequest("错误的输入 🚫".to_string());
    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::BAD_REQUEST);
}

#[test]
fn test_status_code_correctness() {
    // Verify status codes match HTTP semantics
    let test_cases = [
        (
            ServerError::Permission("".to_string()),
            StatusCode::FORBIDDEN,
            403,
        ),
        (
            ServerError::InvalidRequest("".to_string()),
            StatusCode::BAD_REQUEST,
            400,
        ),
        (
            ServerError::Internal("".to_string()),
            StatusCode::INTERNAL_SERVER_ERROR,
            500,
        ),
    ];

    for (error, expected_status, expected_code) in test_cases {
        let response = error.into_response();
        let (parts, _body) = response.into_parts();

        assert_eq!(parts.status, expected_status);
        assert_eq!(parts.status.as_u16(), expected_code);
    }
}

#[test]
fn test_agent_error_conversion_preserves_message() {
    let agent_error = mixtape_core::AgentError::ToolDenied("Permission denied".to_string());
    let server_error: ServerError = agent_error.into();

    // The display should contain information about the error
    let display = server_error.to_string();
    assert!(
        display.contains("Tool execution denied") || display.contains("Permission denied"),
        "Error message should be preserved"
    );
}

#[test]
fn test_error_types_are_send_sync() {
    // Verify error types can be sent across threads
    fn is_send<T: Send>() {}
    fn is_sync<T: Sync>() {}

    is_send::<ServerError>();
    is_sync::<ServerError>();
}

#[test]
fn test_multiple_permission_errors() {
    // Verify consistent behavior across multiple instances
    let errors = vec![
        ServerError::Permission("Error 1".to_string()),
        ServerError::Permission("Error 2".to_string()),
        ServerError::Permission("Error 3".to_string()),
    ];

    for error in errors {
        let response = error.into_response();
        let (parts, _body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::FORBIDDEN);
    }
}

#[test]
fn test_error_debug_output() {
    let error = ServerError::InvalidRequest("test".to_string());
    let debug_str = format!("{:?}", error);

    // Should contain variant name and message
    assert!(debug_str.contains("InvalidRequest"));
    assert!(debug_str.contains("test"));
}

#[test]
fn test_error_nested_quotes() {
    let error = ServerError::InvalidRequest(r#"Field "name" has invalid value "test""#.to_string());
    let response = error.into_response();
    let (parts, _body) = response.into_parts();

    assert_eq!(parts.status, StatusCode::BAD_REQUEST);
    // JSON encoding should properly escape nested quotes
}
