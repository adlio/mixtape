use mixtape_core::agent::{AgentResponse, TokenUsageStats, ToolCallInfo};
use std::time::Duration;

// ===== AgentResponse Tests =====

fn make_test_response(text: &str) -> AgentResponse {
    AgentResponse {
        text: text.to_string(),
        tool_calls: vec![],
        token_usage: None,
        duration: Duration::from_millis(100),
        model_calls: 1,
    }
}

#[test]
fn test_agent_response_text_method() {
    let response = make_test_response("Hello, world!");
    assert_eq!(response.text(), "Hello, world!");
}

#[test]
fn test_agent_response_display() {
    let response = make_test_response("Display test");
    assert_eq!(format!("{}", response), "Display test");
}

#[test]
fn test_agent_response_into_string() {
    let response = make_test_response("Into string");
    let s: String = response.into();
    assert_eq!(s, "Into string");
}

#[test]
fn test_agent_response_partial_eq_str() {
    let response = make_test_response("Compare me");
    assert!(response == "Compare me");
    assert!(!(response == "Other"));
}

#[test]
fn test_agent_response_with_tool_calls() {
    let response = AgentResponse {
        text: "Done".to_string(),
        tool_calls: vec![
            ToolCallInfo {
                name: "read_file".to_string(),
                input: serde_json::json!({"path": "/tmp/test"}),
                output: "file contents".to_string(),
                success: true,
                duration: Duration::from_millis(50),
            },
            ToolCallInfo {
                name: "write_file".to_string(),
                input: serde_json::json!({"path": "/tmp/out", "content": "data"}),
                output: "Error: permission denied".to_string(),
                success: false,
                duration: Duration::from_millis(10),
            },
        ],
        token_usage: Some(TokenUsageStats {
            input_tokens: 100,
            output_tokens: 50,
        }),
        duration: Duration::from_secs(1),
        model_calls: 2,
    };

    assert_eq!(response.tool_calls.len(), 2);
    assert!(response.tool_calls[0].success);
    assert!(!response.tool_calls[1].success);
    assert_eq!(response.token_usage.unwrap().total(), 150);
}

// ===== TokenUsageStats Tests =====

#[test]
fn test_token_usage_stats_total() {
    let stats = TokenUsageStats {
        input_tokens: 1000,
        output_tokens: 500,
    };
    assert_eq!(stats.total(), 1500);
}

#[test]
fn test_token_usage_stats_default() {
    let stats = TokenUsageStats::default();
    assert_eq!(stats.input_tokens, 0);
    assert_eq!(stats.output_tokens, 0);
    assert_eq!(stats.total(), 0);
}
