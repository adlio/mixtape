//! Integration tests for mixtape-server.
//!
//! These tests verify the full request→hook→agent→events→SSE flow.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mixtape_core::test_utils::MockProvider;
use mixtape_core::Agent;
use mixtape_server::MixtapeRouter;
use tower::ServiceExt;

/// Helper to build an agent with a mock provider.
async fn build_mock_agent(provider: MockProvider) -> Agent {
    Agent::builder()
        .provider(provider)
        .build()
        .await
        .expect("Failed to build agent")
}

/// Helper to create SSE request body.
fn sse_request(message: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/copilotkit")
        .header("Content-Type", "application/json")
        .body(Body::from(format!(r#"{{"message": "{}"}}"#, message)))
        .unwrap()
}

/// Collect SSE events from response body.
async fn collect_sse_events(body: Body) -> Vec<String> {
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .filter(|line| line.starts_with("data: "))
        .map(|line| line.strip_prefix("data: ").unwrap().to_string())
        .collect()
}

/// Extract event type names from SSE event JSON strings.
fn extract_event_types(events: &[String]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| {
            serde_json::from_str::<serde_json::Value>(e)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str().map(String::from)))
        })
        .collect()
}

// ============================================================================
// Hook Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_hooks_receive_events_during_request() {
    let provider = MockProvider::new().with_text("Hello!");
    let agent = build_mock_agent(provider).await;

    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let response = app.oneshot(sse_request("Hi")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let events = collect_sse_events(response.into_body()).await;
    let event_types = extract_event_types(&events);

    assert!(event_types.contains(&"RUN_STARTED".to_string()));
    assert!(event_types.contains(&"RUN_FINISHED".to_string()));
}

#[tokio::test]
async fn test_multiple_requests_produce_consistent_events() {
    let mut event_counts = Vec::new();

    for i in 0..3 {
        let provider = MockProvider::new().with_text(format!("Response {}", i));
        let agent = build_mock_agent(provider).await;
        let app = MixtapeRouter::new(agent)
            .with_agui("/api/copilotkit")
            .build()
            .unwrap();

        let response = app.oneshot(sse_request("Hi")).await.unwrap();
        let events = collect_sse_events(response.into_body()).await;
        event_counts.push(events.len());
    }

    // All requests should produce the same number of events
    assert!(
        event_counts.iter().all(|&c| c == event_counts[0]),
        "Event counts should be consistent: {:?}",
        event_counts
    );
}

// ============================================================================
// SSE Stream Tests
// ============================================================================

#[tokio::test]
async fn test_sse_stream_format() {
    let provider = MockProvider::new().with_text("Hello, world!");
    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let response = app.oneshot(sse_request("Hi")).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let events = collect_sse_events(response.into_body()).await;
    for event in &events {
        assert!(
            serde_json::from_str::<serde_json::Value>(event).is_ok(),
            "Event should be valid JSON: {}",
            event
        );
    }
}

#[tokio::test]
async fn test_sse_event_sequence() {
    let provider = MockProvider::new().with_text("Test response");
    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let response = app.oneshot(sse_request("Hello")).await.unwrap();
    let events = collect_sse_events(response.into_body()).await;
    let event_types = extract_event_types(&events);

    assert_eq!(event_types.first(), Some(&"RUN_STARTED".to_string()));
    assert_eq!(event_types.last(), Some(&"RUN_FINISHED".to_string()));
    assert!(event_types.contains(&"TEXT_MESSAGE_START".to_string()));
    assert!(event_types.contains(&"TEXT_MESSAGE_END".to_string()));
}

#[tokio::test]
async fn test_sse_tool_call_events() {
    let provider = MockProvider::new()
        .with_tool_use("calculator", serde_json::json!({"expression": "2+2"}))
        .with_text("The answer is 4");

    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let response = app.oneshot(sse_request("What is 2+2?")).await.unwrap();
    let events = collect_sse_events(response.into_body()).await;
    let event_types = extract_event_types(&events);

    assert!(event_types.contains(&"TOOL_CALL_START".to_string()));
    assert!(event_types.contains(&"TOOL_CALL_ARGS".to_string()));
    assert!(event_types.contains(&"TOOL_CALL_END".to_string()));
}

#[tokio::test]
async fn test_sse_uses_provided_thread_and_run_ids() {
    let provider = MockProvider::new().with_text("Hello!");
    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let request = Request::builder()
        .method("POST")
        .uri("/api/copilotkit")
        .header("Content-Type", "application/json")
        .body(Body::from(
            r#"{"message": "Hi", "thread_id": "thread-123", "run_id": "run-456"}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let events = collect_sse_events(response.into_body()).await;

    let run_started = events
        .iter()
        .find(|e| e.contains("RUN_STARTED"))
        .expect("Should have RUN_STARTED");

    let parsed: serde_json::Value = serde_json::from_str(run_started).unwrap();
    assert_eq!(parsed["thread_id"], "thread-123");
    assert_eq!(parsed["run_id"], "run-456");
}

#[tokio::test]
async fn test_sse_generates_ids_when_not_provided() {
    let provider = MockProvider::new().with_text("Hello!");
    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let response = app.oneshot(sse_request("Hi")).await.unwrap();
    let events = collect_sse_events(response.into_body()).await;

    let run_started = events
        .iter()
        .find(|e| e.contains("RUN_STARTED"))
        .expect("Should have RUN_STARTED");

    let parsed: serde_json::Value = serde_json::from_str(run_started).unwrap();
    let thread_id = parsed["thread_id"].as_str().unwrap();
    let run_id = parsed["run_id"].as_str().unwrap();

    assert!(
        uuid::Uuid::parse_str(thread_id).is_ok(),
        "thread_id should be valid UUID"
    );
    assert!(
        uuid::Uuid::parse_str(run_id).is_ok(),
        "run_id should be valid UUID"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_sse_error_event_on_provider_failure() {
    let provider = MockProvider::new(); // No responses = will error
    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let response = app.oneshot(sse_request("Hi")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK); // SSE streams errors as events

    let events = collect_sse_events(response.into_body()).await;
    assert!(
        events.iter().any(|e| e.contains("RUN_ERROR")),
        "Should have RUN_ERROR event: {:?}",
        events
    );
}

#[tokio::test]
async fn test_invalid_request_body_returns_error() {
    let provider = MockProvider::new().with_text("Hello!");
    let agent = build_mock_agent(provider).await;
    let app = MixtapeRouter::new(agent)
        .with_agui("/api/copilotkit")
        .build()
        .unwrap();

    let request = Request::builder()
        .method("POST")
        .uri("/api/copilotkit")
        .header("Content-Type", "application/json")
        .body(Body::from("not valid json"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert!(response.status().is_client_error());
}
