use super::*;
use crate::{Agent, ContentBlock, Tool, ToolError, ToolResult, ToolResultBlock, ToolResultStatus};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Default)]
struct StubClient {
    requests: Mutex<Vec<(Value, bool)>>,
    responses: Mutex<VecDeque<HttpResponse>>,
}

#[async_trait::async_trait]
impl ChatClient for StubClient {
    fn region(&self) -> &str {
        "us-west-2"
    }
    async fn send(&self, body: Vec<u8>, streaming: bool) -> Result<HttpResponse, ProviderError> {
        self.requests
            .lock()
            .unwrap()
            .push((serde_json::from_slice(&body).unwrap(), streaming));
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| protocol("Fixture exhausted"))
    }
}

type Records = Arc<Mutex<Vec<BedrockInvocation>>>;

fn provider(
    responses: Vec<HttpResponse>,
) -> (BedrockChatCompletionsProvider, Arc<StubClient>, Records) {
    let client = Arc::new(StubClient {
        responses: Mutex::new(responses.into()),
        ..StubClient::default()
    });
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = records.clone();
    let model = RuntimeBedrockModel::new("Kimi K3", "moonshotai.kimi-k3", 128_000, 4096).unwrap();
    let provider = BedrockChatCompletionsProvider::with_transport(client.clone(), model)
        .unwrap()
        .with_inference_target("us.moonshotai.kimi-k3")
        .unwrap()
        .with_retry_config(RetryConfig {
            max_attempts: 2,
            base_delay_ms: 0,
            max_delay_ms: 0,
        })
        .with_invocation_callback(move |record| captured.lock().unwrap().push(record));
    (provider, client, records)
}

fn http(status: u16, content_type: &str, bytes: Vec<u8>) -> HttpResponse {
    // Small chunks intentionally split both SSE framing and multibyte UTF-8.
    let chunks: Vec<_> = bytes.chunks(3).map(|part| Ok(part.to_vec())).collect();
    HttpResponse {
        status,
        request_id: Some(format!("fixture-http-{status}")),
        content_type: Some(content_type.into()),
        body: Box::pin(futures::stream::iter(chunks)),
    }
}

fn json_http(value: Value) -> HttpResponse {
    http(
        200,
        "application/json; charset=utf-8",
        serde_json::to_vec(&value).unwrap(),
    )
}
fn final_response() -> Value {
    json!({"model":"provider-reported-model", "choices":[{"index":0, "finish_reason":"stop", "message":{"role":"assistant", "content":"The count is 7."}}],
        "usage":{"prompt_tokens":20,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":2}}})
}
fn chunk(delta: Value, finish: Value) -> Value {
    json!({"model":"provider-reported-model", "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}
fn sse(values: Vec<Value>, done: bool) -> HttpResponse {
    let mut text = String::from(": heartbeat\r\n\r\n");
    for value in values {
        text.push_str(&format!("data: {value}\r\n\r\n"));
    }
    if done {
        text.push_str("data: [DONE]\r\n\r\n");
    }
    http(200, "text/event-stream", text.into_bytes())
}
fn tool_stream() -> HttpResponse {
    sse(
        vec![
            chunk(
                json!({"role":"assistant", "reasoning_content":"Count λ "}),
                Value::Null,
            ),
            chunk(json!({"reasoning_content":"carefully."}), Value::Null),
            chunk(json!({"content":"Checking."}), Value::Null),
            chunk(
                json!({"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"evidence","arguments":"{\"n\":"}}]}),
                Value::Null,
            ),
            chunk(
                json!({"tool_calls":[{"index":0,"function":{"arguments":"7}"}}]}),
                json!("tool_calls"),
            ),
            json!({"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":8,"prompt_tokens_details":{"cached_tokens":4,"cache_write_tokens":0},"completion_tokens_details":{"reasoning_tokens":3}}}),
        ],
        true,
    )
}
fn final_stream() -> HttpResponse {
    sse(
        vec![chunk(
            json!({"role":"assistant","content":"The count is 7."}),
            json!("stop"),
        )],
        true,
    )
}

#[tokio::test]
async fn nonstream_records_exact_request_and_actual_provider_metadata() {
    let (provider, client, records) = provider(vec![json_http(final_response())]);
    let response = provider
        .generate(vec![Message::user("fixture-private-prompt")], vec![], None)
        .await
        .unwrap();
    assert_eq!(response.message.text(), "The count is 7.");
    assert_eq!(response.usage.unwrap().input_tokens, 20);
    let requests = client.requests.lock().unwrap();
    assert_eq!(requests[0].0["model"], "us.moonshotai.kimi-k3");
    assert_eq!(requests[0].0["stream"], false);
    assert!(requests[0].0.get("stream_options").is_none());
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.requested_model, "moonshotai.kimi-k3");
    assert_eq!(record.dispatched_target, "us.moonshotai.kimi-k3");
    assert_eq!(
        record.provider_reported_model.as_deref(),
        Some("provider-reported-model")
    );
    assert_eq!(
        record.usage.as_ref().unwrap().cache_read_input_tokens,
        Some(0)
    );
    assert_eq!(
        record.usage.as_ref().unwrap().cache_write_input_tokens,
        None
    );
    assert_eq!(record.usage.as_ref().unwrap().reasoning_tokens, Some(2));
    assert_eq!(record.outcome, InvocationOutcome::Completed);
    assert_eq!(record.attempts, 1);
    assert_eq!(record.sdk_retry_count, Some(0));
    let serialized = serde_json::to_string(record).unwrap();
    assert!(!serialized.contains("fixture-private-prompt"));
    assert!(!serialized.contains("The count is 7."));
}

#[tokio::test]
async fn missing_usage_and_model_identity_are_not_invented() {
    let mut response = final_response();
    response.as_object_mut().unwrap().remove("usage");
    response.as_object_mut().unwrap().remove("model");
    let (provider, _, records) = provider(vec![json_http(response)]);
    assert!(provider
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .unwrap()
        .usage
        .is_none());
    let records = records.lock().unwrap();
    assert!(records[0].usage.is_none());
    assert!(records[0].provider_reported_model.is_none());
}

#[tokio::test]
async fn streaming_preserves_reasoning_tools_order_and_final_usage() {
    let (provider, _, records) = provider(vec![tool_stream()]);
    let events: Vec<_> = provider
        .generate_stream(vec![Message::user("x")], vec![], None)
        .await
        .unwrap()
        .collect()
        .await;
    let events: Vec<_> = events.into_iter().collect::<Result<_, _>>().unwrap();
    let blocks: Vec<_> = events
        .iter()
        .filter_map(|event| {
            if let StreamEvent::ContentBlock(block) = event {
                Some(block)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(blocks.len(), 3);
    assert!(
        matches!(blocks[0], ContentBlock::Thinking { thinking, signature } if thinking == "Count λ carefully." && signature.is_empty())
    );
    assert!(matches!(blocks[1], ContentBlock::Text(text) if text == "Checking."));
    assert!(
        matches!(blocks[2], ContentBlock::ToolUse(tool) if tool.id == "call-1" && tool.input == json!({"n":7}))
    );
    assert!(
        matches!(events.last().unwrap(), StreamEvent::Stop { stop_reason: StopReason::ToolUse, usage: Some(usage) } if usage.output_tokens == 8)
    );
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].api, "chat_completions_stream");
    assert_eq!(
        records[0].usage.as_ref().unwrap().cache_read_input_tokens,
        Some(4)
    );
    assert_eq!(records[0].outcome, InvocationOutcome::Completed);
}

struct EvidenceTool;
#[derive(Deserialize, JsonSchema)]
struct EvidenceInput {
    n: u32,
}
impl Tool for EvidenceTool {
    type Input = EvidenceInput;
    fn name(&self) -> &str {
        "evidence"
    }
    fn description(&self) -> &str {
        "Read a synthetic count"
    }
    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(input.n.to_string()))
    }
}

#[tokio::test]
async fn real_agent_loop_replays_reasoning_and_matching_tool_result() {
    let (provider, client, records) = provider(vec![tool_stream(), final_stream()]);
    let agent = Agent::builder()
        .provider(provider)
        .add_trusted_tool(EvidenceTool)
        .build()
        .await
        .unwrap();
    let result = agent.run("Read the synthetic count.").await.unwrap();
    assert_eq!(result.text(), "The count is 7.");
    assert_eq!(result.model_calls, 2);
    assert_eq!(result.tool_calls.len(), 1);
    let requests = client.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|(body, streaming)| *streaming && body["stream_options"]["include_usage"] == true));
    let messages = requests[1].0["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["reasoning_content"], "Count λ carefully.");
    assert_eq!(assistant["content"], "Checking.");
    assert_eq!(
        assistant["tool_calls"][0]["function"]["arguments"],
        "{\"n\":7}"
    );
    let tool = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    assert_eq!(tool["tool_call_id"], "call-1");
    assert_eq!(tool["content"], "7");
    assert!(records
        .lock()
        .unwrap()
        .iter()
        .all(|record| record.outcome == InvocationOutcome::Completed));
}

#[tokio::test]
async fn retries_only_initial_http_and_keeps_the_exact_target() {
    let (provider, client, records) = provider(vec![
        http(429, "application/json", b"private-error-body".to_vec()),
        json_http(final_response()),
    ]);
    provider
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .unwrap();
    let requests = client.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, requests[1].0);
    let records = records.lock().unwrap();
    assert_eq!(records[0].attempts, 2);
    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn permanent_errors_do_not_retry_or_echo_error_bodies() {
    let (provider, client, records) = provider(vec![http(
        403,
        "application/json",
        b"private-error-body".to_vec(),
    )]);
    let error = provider
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .unwrap_err();
    assert!(matches!(error, ProviderError::Authentication(_)));
    assert!(!error.to_string().contains("private-error-body"));
    assert_eq!(client.requests.lock().unwrap().len(), 1);
    let records = records.lock().unwrap();
    assert_eq!(records[0].request_id.as_deref(), Some("fixture-http-403"));
    assert_eq!(records[0].outcome, InvocationOutcome::Failed);
}

#[tokio::test]
async fn dropped_unpolled_and_partially_consumed_streams_are_cancelled_once() {
    for consume_one in [false, true] {
        let (provider, _, records) = provider(vec![tool_stream()]);
        let mut stream = provider
            .generate_stream(vec![Message::user("x")], vec![], None)
            .await
            .unwrap();
        if consume_one {
            assert!(stream.next().await.unwrap().is_ok());
        }
        assert!(records.lock().unwrap().is_empty());
        drop(stream);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
        assert_eq!(records[0].attempts, 1);
    }
}

#[tokio::test]
async fn cancellation_during_dispatch_is_measured() {
    struct PendingClient;
    #[async_trait::async_trait]
    impl ChatClient for PendingClient {
        fn region(&self) -> &str {
            "us-west-2"
        }
        async fn send(&self, _: Vec<u8>, _: bool) -> Result<HttpResponse, ProviderError> {
            futures::future::pending().await
        }
    }
    let (mut provider, _, records) = provider(vec![]);
    provider.client = Arc::new(PendingClient);
    let mut future = Box::pin(provider.generate(vec![Message::user("x")], vec![], None));
    assert!(futures::poll!(future.as_mut()).is_pending());
    drop(future);
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
    assert_eq!(records[0].attempts, 1);
}

#[tokio::test]
async fn eof_before_done_fails_without_reconnect_or_tool_execution() {
    let response = sse(
        vec![chunk(
            json!({"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"evidence","arguments":"{\"n\":7}"}}]}),
            json!("tool_calls"),
        )],
        false,
    );
    let (provider, client, records) = provider(vec![response]);
    let events: Vec<_> = provider
        .generate_stream(vec![Message::user("x")], vec![], None)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(events.iter().any(Result::is_err));
    assert!(!events.iter().any(|event| matches!(
        event,
        Ok(StreamEvent::ToolUse(_) | StreamEvent::Stop { .. })
    )));
    assert_eq!(client.requests.lock().unwrap().len(), 1);
    assert_eq!(
        records.lock().unwrap()[0].outcome,
        InvocationOutcome::Failed
    );
}

#[tokio::test]
async fn invalid_configuration_and_lossy_replay_never_dispatch() {
    let (provider, client, records) = provider(vec![]);
    assert!(provider
        .clone()
        .with_max_tokens(0)
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .is_err());
    assert!(provider
        .clone()
        .with_reasoning_effort("medium")
        .validate_configuration()
        .is_err());
    assert!(provider
        .clone()
        .with_reasoning_effort("low")
        .validate_configuration()
        .is_ok());
    for block in [
        ContentBlock::Thinking {
            thinking: "private-thinking".into(),
            signature: "private-signature".into(),
        },
        ContentBlock::RedactedThinking {
            data: "private-opaque-data".into(),
        },
    ] {
        let error = provider
            .generate(
                vec![Message::assistant_with_content(vec![block])],
                vec![],
                None,
            )
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("private"));
    }
    assert!(client.requests.lock().unwrap().is_empty());
    assert!(records.lock().unwrap().is_empty());
}

#[test]
fn tool_errors_keep_their_status_and_binary_results_are_rejected() {
    let result = Message::tool_results(vec![ToolResultBlock {
        tool_use_id: "call-1".into(),
        content: ToolResult::text("missing"),
        status: ToolResultStatus::Error,
    }]);
    let messages = conversion::messages(&[result], None).unwrap();
    let content: Value = serde_json::from_str(messages[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["is_error"], true);
    let binary = Message::tool_results(vec![ToolResultBlock {
        tool_use_id: "call-1".into(),
        content: ToolResult::image(crate::ImageFormat::Png, vec![1, 2, 3]),
        status: ToolResultStatus::Success,
    }]);
    assert!(conversion::messages(&[binary], None).is_err());
}

#[tokio::test]
async fn rejects_malformed_responses_instead_of_reporting_success() {
    let mut cases = Vec::new();
    let mut response = final_response();
    response["choices"][0]["finish_reason"] = Value::Null;
    cases.push(response);
    let mut response = final_response();
    response["choices"][0]["index"] = json!(1);
    cases.push(response);
    let mut response = final_response();
    response["choices"][0]["message"]["reasoning_details"] = json!([{"opaque":"secret"}]);
    cases.push(response);
    let mut response = final_response();
    response["usage"]["prompt_tokens"] = json!(-1);
    cases.push(response);
    let mut response = final_response();
    response["choices"][0]["finish_reason"] = json!("unexpected");
    cases.push(response);
    cases.push(json!({"error":{"message":"private-error-body"}}));
    for response in cases {
        let (provider, _, records) = provider(vec![json_http(response)]);
        assert!(provider
            .generate(vec![Message::user("x")], vec![], None)
            .await
            .is_err());
        assert_eq!(
            records.lock().unwrap()[0].outcome,
            InvocationOutcome::Failed
        );
    }
}

#[test]
fn malformed_streams_never_produce_a_successful_terminal_event() {
    let cases = vec![
        vec![json!({"choices":[]})],
        vec![chunk(json!({"role":"user"}), Value::Null)],
        vec![
            chunk(json!({"content":"x"}), json!("stop")),
            chunk(json!({}), json!("stop")),
        ],
        vec![chunk(
            json!({"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"evidence","arguments":"not-json"}}]}),
            json!("tool_calls"),
        )],
        vec![chunk(
            json!({"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"evidence","arguments":"[]"}}]}),
            json!("tool_calls"),
        )],
        vec![chunk(json!({"content":"x"}), Value::Null)],
        vec![chunk(json!({"content":"x"}), json!("tool_calls"))],
        vec![chunk(json!({"reasoning_details":[{}]}), json!("stop"))],
    ];
    for case in cases {
        let mut assembler = streaming::StreamAssembler::default();
        let error = case
            .iter()
            .try_for_each(|value| assembler.push(&value.to_string()).map(|_| ()))
            .and_then(|_| assembler.push("[DONE]").map(|_| ()));
        assert!(error.is_err());
    }
}

#[tokio::test]
async fn refusal_and_length_are_rejected_outcomes_not_completed() {
    for refusal in [false, true] {
        let mut response = final_response();
        if refusal {
            response["choices"][0]["message"]["refusal"] = json!("fixture refusal");
        } else {
            response["choices"][0]["finish_reason"] = json!("length");
        }
        let (provider, _, records) = provider(vec![json_http(response)]);
        let response = provider
            .generate(vec![Message::user("x")], vec![], None)
            .await
            .unwrap();
        assert!(matches!(
            response.stop_reason,
            StopReason::MaxTokens | StopReason::ContentFiltered
        ));
        assert_eq!(
            records.lock().unwrap()[0].outcome,
            InvocationOutcome::Rejected
        );
    }
}

#[tokio::test]
async fn unexpected_content_type_and_transport_errors_fail() {
    let mut interrupted = final_stream();
    interrupted.body = Box::pin(futures::stream::iter(vec![Err(ProviderError::Network(
        "fixture interruption".into(),
    ))]));
    for response in [http(200, "text/html", b"not-json".to_vec()), interrupted] {
        let (provider, client, records) = provider(vec![response]);
        match provider
            .generate_stream(vec![Message::user("x")], vec![], None)
            .await
        {
            Err(_) => {}
            Ok(stream) => assert!(stream.collect::<Vec<_>>().await.iter().any(Result::is_err)),
        }
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        assert_eq!(
            records.lock().unwrap()[0].outcome,
            InvocationOutcome::Failed
        );
    }
}

#[test]
fn kimi_chat_schema_and_forced_tools_reach_both_request_modes() {
    let (provider, _, _) = provider(vec![]);
    let provider = provider.with_output_schema(BedrockJsonSchema {
        name:"count".into(), schema:json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"],"additionalProperties":false}),
    }).with_tool_choice(BedrockToolChoice::Tool("evidence".into()));
    let tools = [ToolDefinition {
        name: "evidence".into(),
        description: "fixture".into(),
        input_schema: json!({"type":"object"}),
    }];
    for streaming in [false, true] {
        let body: Value = serde_json::from_slice(
            &provider
                .request(&[Message::user("x")], &tools, None, streaming)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        assert_eq!(
            body["tool_choice"],
            json!({"type":"function","function":{"name":"evidence"}})
        );
    }
    assert!(provider
        .request(&[Message::user("x")], &[], None, false)
        .is_err());
}

#[test]
fn known_claude_and_astra_chat_tool_combinations_are_rejected() {
    let (mut provider, _, _) = provider(vec![]);
    provider.base_model_id = "anthropic.claude-sonnet-5".into();
    provider.target = "us.anthropic.claude-sonnet-5".into();
    assert!(provider.validate_configuration().is_err());
    provider.base_model_id = "openai.gpt-6-astra".into();
    provider.target = "us.openai.gpt-6-astra".into();
    let tools = [ToolDefinition {
        name: "evidence".into(),
        description: "fixture".into(),
        input_schema: json!({"type":"object"}),
    }];
    assert!(provider
        .request(&[Message::user("x")], &tools, None, true)
        .is_err());
    assert!(provider
        .request(&[Message::user("x")], &[], None, true)
        .is_ok());
}
