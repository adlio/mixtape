use super::*;
use crate::types::{ResponsesReplay, ToolResultBlock, ToolResultStatus};
use crate::{Agent, ContentBlock, Tool, ToolError, ToolResult};
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
impl RuntimeClient for StubClient {
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
fn provider(responses: Vec<HttpResponse>) -> (BedrockResponsesProvider, Arc<StubClient>, Records) {
    let client = Arc::new(StubClient {
        responses: Mutex::new(responses.into()),
        ..StubClient::default()
    });
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = records.clone();
    let model = RuntimeBedrockModel::new("Astra", "openai.gpt-6-astra", 128_000, 4096).unwrap();
    let provider = BedrockResponsesProvider::with_transport(client.clone(), model)
        .unwrap()
        .with_inference_target("us.openai.gpt-6-astra")
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
    // Split UTF-8 and CRLF framing across transport chunks.
    let chunks: Vec<_> = bytes.chunks(3).map(|bytes| Ok(bytes.to_vec())).collect();
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
fn text_item(id: &str, text: &str, phase: &str) -> Value {
    json!({"id":id, "type":"message", "role":"assistant", "status":"completed", "phase":phase,
        "content":[{"type":"output_text", "text":text, "annotations":[], "logprobs":[]}]})
}
fn response(items: Vec<Value>) -> Value {
    json!({"id":"resp-1", "object":"response", "model":"provider-reported-model", "status":"completed", "error":null, "incomplete_details":null,
        "output":items, "usage":{"input_tokens":20, "output_tokens":5,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":2},"output_tokens_details":{"reasoning_tokens":3}}})
}
fn tool_response() -> Value {
    response(vec![
        json!({"id":"rs-1", "type":"reasoning", "status":"completed", "summary":[], "encrypted_content":"fixture-opaque-private", "provider_extension":{"value":"preserve-me"}}),
        text_item("msg-1", "Checking λ.", "commentary"),
        json!({"id":"fc-1", "type":"function_call", "status":"completed", "call_id":"call-1", "name":"evidence", "arguments":"{\"n\": 7}"}),
    ])
}
fn final_response() -> Value {
    response(vec![text_item(
        "msg-final",
        "The count is 7.",
        "final_answer",
    )])
}

fn stream_events(response: &Value) -> Vec<Value> {
    let mut events = vec![
        json!({"type":"response.created", "response":{"id":response["id"], "status":"in_progress", "model":response["model"], "output":[]}}),
    ];
    for (index, item) in response["output"].as_array().unwrap().iter().enumerate() {
        let mut added = item.clone();
        added["status"] = json!("in_progress");
        if item["type"] == "reasoning" {
            added["encrypted_content"] = json!("incomplete-must-not-replay");
        }
        if item["type"] == "function_call" {
            added["arguments"] = json!("");
        }
        if item["type"] == "message" {
            added["content"] = json!([]);
        }
        events
            .push(json!({"type":"response.output_item.added", "output_index":index, "item":added}));
        if item["type"] == "message" {
            let text = item["content"][0]["text"].as_str().unwrap();
            events.push(json!({"type":"response.content_part.added", "output_index":index, "item_id":item["id"], "content_index":0, "part":{"type":"output_text","text":"","annotations":[]}}));
            for character in text.chars() {
                events.push(json!({"type":"response.output_text.delta", "output_index":index,"item_id":item["id"],"content_index":0,"delta":character.to_string()}));
            }
            events.push(json!({"type":"response.output_text.done", "output_index":index,"item_id":item["id"],"content_index":0,"text":text}));
            events.push(json!({"type":"response.content_part.done", "output_index":index,"item_id":item["id"],"content_index":0,"part":item["content"][0]}));
        }
        if item["type"] == "function_call" {
            for character in item["arguments"].as_str().unwrap().chars() {
                events.push(json!({"type":"response.function_call_arguments.delta", "output_index":index,"item_id":item["id"],"delta":character.to_string()}));
            }
            events.push(json!({"type":"response.function_call_arguments.done", "output_index":index,"item_id":item["id"],"arguments":item["arguments"]}));
        }
        events.push(json!({"type":"response.output_item.done", "output_index":index,"item":item}));
    }
    events.push(json!({"type":format!("response.{}", response["status"].as_str().unwrap()), "response":response}));
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(index);
    }
    events
}
fn sse(events: Vec<Value>) -> HttpResponse {
    let mut text = String::from(": heartbeat\r\n\r\n");
    for event in events {
        text.push_str(&format!(
            "event: {}\r\ndata: {event}\r\n\r\n",
            event["type"].as_str().unwrap()
        ));
    }
    http(200, "text/event-stream", text.into_bytes())
}

#[tokio::test]
async fn normal_response_retains_items_and_records_only_reported_metadata() {
    let raw = tool_response();
    let (provider, client, records) = provider(vec![json_http(raw.clone())]);
    let response = provider
        .generate(
            vec![Message::user("fixture-private-prompt")],
            vec![],
            Some("fixture-system".into()),
        )
        .await
        .unwrap();
    assert_eq!(response.message.text(), "Checking λ.");
    assert_eq!(response.message.tool_uses()[0].id, "call-1");
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    let replay = response.message.content.last().unwrap();
    assert!(
        matches!(replay, ContentBlock::ResponsesReplay(value) if value.items == raw["output"].as_array().unwrap().clone())
    );
    assert!(!format!("{replay:?}").contains("fixture-opaque-private"));
    let requests = client.requests.lock().unwrap();
    assert_eq!(requests[0].0["store"], false);
    assert_eq!(
        requests[0].0["include"],
        json!(["reasoning.encrypted_content"])
    );
    assert_eq!(requests[0].0["input"][0]["role"], "developer");
    assert!(requests[0].0.get("previous_response_id").is_none());
    assert!(requests[0].0.get("background").is_none());
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].requested_model, "openai.gpt-6-astra");
    assert_eq!(records[0].dispatched_target, "us.openai.gpt-6-astra");
    assert_eq!(
        records[0].provider_reported_model.as_deref(),
        Some("provider-reported-model")
    );
    assert_eq!(
        records[0].usage.as_ref().unwrap().cache_read_input_tokens,
        Some(0)
    );
    assert_eq!(
        records[0].usage.as_ref().unwrap().cache_write_input_tokens,
        Some(2)
    );
    assert_eq!(records[0].sdk_retry_count, Some(0));
    let serialized = serde_json::to_string(&records[0]).unwrap();
    for private in [
        "fixture-private",
        "fixture-opaque",
        "Checking λ",
        "provider_extension",
    ] {
        assert!(!serialized.contains(private));
    }
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
async fn real_agent_replays_final_opaque_reasoning_phase_and_call_ids_exactly() {
    let raw = tool_response();
    let (provider, client, records) = provider(vec![
        sse(stream_events(&raw)),
        sse(stream_events(&final_response())),
    ]);
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
        .all(|(body, streaming)| *streaming && body["store"] == false));
    let items = requests[1].0["input"].as_array().unwrap();
    assert_eq!(&items[1..4], raw["output"].as_array().unwrap());
    assert_eq!(
        items[4],
        json!({"type":"function_call_output", "call_id":"call-1", "output":"7"})
    );
    assert!(!requests[1]
        .0
        .to_string()
        .contains("incomplete-must-not-replay"));
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert!(records
        .iter()
        .all(|record| record.outcome == InvocationOutcome::Completed
            && record.api == "responses_stream"));
}

#[tokio::test]
async fn replay_survives_serialization_and_rejects_edits_or_model_switches() {
    let (provider, _, _) = provider(vec![json_http(tool_response())]);
    let response = provider
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .unwrap();
    let ContentBlock::ResponsesReplay(replay) = response.message.content.last().unwrap() else {
        panic!("missing replay")
    };
    let serialized = serde_json::to_string(replay).unwrap();
    let roundtrip: ResponsesReplay = serde_json::from_str(&serialized).unwrap();
    assert_eq!(replay.items, roundtrip.items);
    assert!(conversion::input(
        std::slice::from_ref(&response.message),
        None,
        "moonshotai.kimi-k3"
    )
    .is_err());
    let mut changed = response.message.clone();
    changed.content[0] = ContentBlock::Text("changed".into());
    assert!(conversion::input(&[changed], None, "openai.gpt-6-astra").is_err());
    assert!(provider.estimate_message_tokens(&[response.message]) > 50);
}

#[tokio::test]
async fn unreported_identity_and_usage_stay_unknown() {
    let mut raw = final_response();
    raw.as_object_mut().unwrap().remove("model");
    raw.as_object_mut().unwrap().remove("usage");
    let (provider, _, records) = provider(vec![json_http(raw)]);
    let response = provider
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .unwrap();
    assert!(response.usage.is_none());
    let records = records.lock().unwrap();
    assert!(records[0].usage.is_none());
    assert!(records[0].provider_reported_model.is_none());
}

#[tokio::test]
async fn retry_is_bounded_preserves_target_and_does_not_echo_error_bodies() {
    let (provider, client, records) = provider(vec![
        http(429, "application/json", b"private-error".to_vec()),
        json_http(final_response()),
    ]);
    provider
        .generate(vec![Message::user("x")], vec![], None)
        .await
        .unwrap();
    let requests = client.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(records.lock().unwrap()[0].attempts, 2);
}

#[tokio::test]
async fn permanent_failure_and_bad_content_type_fail_once() {
    for response in [
        http(403, "application/json", b"private-error".to_vec()),
        http(200, "text/html", b"private-error".to_vec()),
    ] {
        let (provider, client, records) = provider(vec![response]);
        let error = provider
            .generate(vec![Message::user("x")], vec![], None)
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("private-error"));
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        let records = records.lock().unwrap();
        assert_eq!(records[0].outcome, InvocationOutcome::Failed);
        assert!(records[0].request_id.is_some());
    }
}

#[tokio::test]
async fn abandoned_streams_and_dispatch_emit_one_cancelled_record() {
    for consume_one in [false, true] {
        let (provider, _, records) = provider(vec![sse(stream_events(&tool_response()))]);
        let mut stream = provider
            .generate_stream(vec![Message::user("x")], vec![], None)
            .await
            .unwrap();
        if consume_one {
            assert!(stream.next().await.unwrap().is_ok());
        }
        drop(stream);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
        assert_eq!(records[0].attempts, 1);
    }
    struct Pending;
    #[async_trait::async_trait]
    impl RuntimeClient for Pending {
        fn region(&self) -> &str {
            "us-west-2"
        }
        async fn send(&self, _: Vec<u8>, _: bool) -> Result<HttpResponse, ProviderError> {
            futures::future::pending().await
        }
    }
    let (mut provider, _, records) = provider(vec![]);
    provider.client = Arc::new(Pending);
    let mut future = Box::pin(provider.generate(vec![Message::user("x")], vec![], None));
    assert!(futures::poll!(future.as_mut()).is_pending());
    drop(future);
    assert_eq!(
        records.lock().unwrap()[0].outcome,
        InvocationOutcome::Cancelled
    );
}

#[tokio::test]
async fn incomplete_or_filtered_responses_never_release_tool_calls() {
    for reason in ["max_output_tokens", "content_filter"] {
        let mut raw = tool_response();
        raw["status"] = json!("incomplete");
        raw["incomplete_details"] = json!({"reason":reason});
        raw["output"][2]["arguments"] = json!("{unfinished");
        let events = vec![
            json!({"type":"response.created", "response":{"id":"resp-1", "status":"in_progress"}}),
            json!({"type":"response.incomplete", "response":raw}),
        ];
        let (provider, _, records) = provider(vec![sse(events)]);
        let events: Vec<_> = provider
            .generate_stream(vec![Message::user("x")], vec![], None)
            .await
            .unwrap()
            .collect()
            .await;
        assert!(events.iter().all(Result::is_ok));
        assert!(!events
            .iter()
            .any(|event| matches!(event, Ok(StreamEvent::ToolUse(_)))));
        assert!(matches!(
            events.last().unwrap(),
            Ok(StreamEvent::Stop {
                stop_reason: StopReason::MaxTokens | StopReason::ContentFiltered,
                ..
            })
        ));
        assert_eq!(
            records.lock().unwrap()[0].outcome,
            InvocationOutcome::Rejected
        );
    }
}

#[tokio::test]
async fn truncated_streams_do_not_reconnect_or_execute_tools() {
    let mut events = stream_events(&tool_response());
    events.pop();
    let (provider, client, records) = provider(vec![sse(events)]);
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

#[test]
fn malformed_streams_and_inconsistent_terminal_snapshots_fail() {
    for mutation in 0..7 {
        let mut events = stream_events(&tool_response());
        match mutation {
            0 => {
                events[1]["output_index"] = json!(8);
            }
            1 => {
                events[2]["item"]["id"] = json!("different");
            }
            2 => {
                events[2]["sequence_number"] = json!(0);
            }
            3 => {
                events.last_mut().unwrap()["response"]["output"][0]["encrypted_content"] =
                    json!("different");
            }
            4 => {
                events.last_mut().unwrap()["response"]["id"] = json!("different");
            }
            5 => {
                events.last_mut().unwrap()["response"]["model"] = json!("different");
            }
            6 => {
                events.last_mut().unwrap()["response"]["status"] = json!("in_progress");
            }
            _ => unreachable!(),
        }
        let mut assembler = streaming::StreamAssembler::new("openai.gpt-6-astra".into());
        let mut record =
            BedrockInvocation::new("fixture", "fixture".into(), "responses_stream", None);
        assert!(
            events
                .iter()
                .try_for_each(|event| assembler
                    .push("", &event.to_string(), &mut record)
                    .map(|_| ()))
                .is_err(),
            "case {mutation}"
        );
    }
}

#[tokio::test]
async fn invalid_content_and_opaque_replay_never_become_successful_responses() {
    let mut cases = Vec::new();
    let mut raw = tool_response();
    raw["output"][2]["arguments"] = json!("[]");
    cases.push(raw);
    let mut raw = tool_response();
    raw["output"][0]["encrypted_content"] = Value::Null;
    cases.push(raw);
    let mut raw = tool_response();
    raw["output"][1]["status"] = json!("in_progress");
    cases.push(raw);
    let mut raw = tool_response();
    raw["output"][2]["type"] = json!("web_search_call");
    cases.push(raw);
    let mut raw = final_response();
    raw["usage"]["input_tokens"] = json!(-1);
    cases.push(raw);
    let mut raw = final_response();
    raw["status"] = json!("failed");
    raw["error"] = json!({"message":"private-error"});
    cases.push(raw);
    let mut raw = tool_response();
    let duplicate = raw["output"][2].clone();
    raw["output"].as_array_mut().unwrap().push(duplicate);
    cases.push(raw);
    for raw in cases {
        let (provider, _, records) = provider(vec![json_http(raw)]);
        let error = provider
            .generate(vec![Message::user("x")], vec![], None)
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("private-error"));
        assert_eq!(
            records.lock().unwrap()[0].outcome,
            InvocationOutcome::Failed
        );
    }
}

#[test]
fn tool_error_status_and_unsupported_cross_api_content_are_explicit() {
    let message = Message::tool_results(vec![ToolResultBlock {
        tool_use_id: "call-1".into(),
        content: ToolResult::text("fixture failure"),
        status: ToolResultStatus::Error,
    }]);
    let input = conversion::input(&[message], None, "openai.gpt-6-astra").unwrap();
    let output: Value = serde_json::from_str(input[0]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["is_error"], true);
    assert_eq!(input[0]["call_id"], "call-1");
    for block in [
        ContentBlock::Thinking {
            thinking: "private".into(),
            signature: "private".into(),
        },
        ContentBlock::RedactedThinking {
            data: "private".into(),
        },
    ] {
        assert!(conversion::input(
            &[Message::assistant_with_content(vec![block])],
            None,
            "openai.gpt-6-astra"
        )
        .is_err());
    }
}

#[test]
fn opus_stays_on_converse_and_native_schema_is_not_assumed_for_gpt() {
    let (provider, _, _) = provider(vec![]);
    assert!(provider
        .clone()
        .with_reasoning_effort("none")
        .validate_configuration()
        .is_err());
    assert!(provider
        .clone()
        .with_reasoning_effort("low")
        .validate_configuration()
        .is_ok());
    assert!(provider
        .with_output_schema(BedrockJsonSchema {
            name: "count".into(),
            schema: json!({"type":"object"})
        })
        .validate_configuration()
        .is_err());
}

#[test]
fn response_cache_uses_original_message_indexes() {
    let (mut provider, _, _) = provider(vec![]);
    provider.base_model_id = "openai.gpt-5.6-sol".into();
    provider.target = "us.openai.gpt-5.6-sol".into();
    let cache = BedrockResponsesCache {
        mode: BedrockResponsesCacheMode::Explicit,
        key: Some("fixture-prefix-v1".into()),
        system: true,
        messages: [0].into(),
    };
    let provider = provider.with_prompt_cache(cache);
    let bytes = provider
        .request(
            &[Message::user("stable evidence")],
            &[],
            Some("instructions"),
            true,
        )
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["prompt_cache_options"],
        json!({"mode":"explicit", "ttl":"30m"})
    );
    assert_eq!(body["prompt_cache_key"], "fixture-prefix-v1");
    for index in [0, 1] {
        assert_eq!(
            body["input"][index]["content"][0]["prompt_cache_breakpoint"]["mode"],
            "explicit"
        );
    }
    assert_eq!(body["store"], false);
    assert!(provider
        .request(&[Message::user("x")], &[], None, false)
        .is_err());
}

#[test]
fn response_cache_rejects_unknown_contracts_and_invalid_positions() {
    let (provider, _, _) = provider(vec![]);
    assert!(provider
        .clone()
        .with_prompt_cache(BedrockResponsesCache::default())
        .validate_configuration()
        .is_err());
    let mut provider = provider;
    provider.base_model_id = "openai.gpt-5.6-terra".into();
    provider.target = "us.openai.gpt-5.6-terra".into();
    for positions in [[1].into(), [0, 1, 2, 3, 4].into()] {
        let provider = provider.clone().with_prompt_cache(BedrockResponsesCache {
            messages: positions,
            ..Default::default()
        });
        assert!(provider
            .request(&[Message::user("x")], &[], None, false)
            .is_err());
    }
    let provider = provider.with_prompt_cache(BedrockResponsesCache {
        messages: [0].into(),
        ..Default::default()
    });
    assert!(provider
        .request(&[Message::assistant("x")], &[], None, false)
        .is_err());
}

#[test]
fn kimi_responses_schema_and_tool_choice_reach_the_wire() {
    let (mut provider, _, _) = provider(vec![]);
    provider.base_model_id = "moonshotai.kimi-k3".into();
    provider.target = "us.moonshotai.kimi-k3".into();
    let provider = provider.with_output_schema(BedrockJsonSchema { name:"count".into(), schema:json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"],"additionalProperties":false}) })
        .with_tool_choice(BedrockToolChoice::Tool("evidence".into()));
    let tools = [ToolDefinition {
        name: "evidence".into(),
        description: "fixture".into(),
        input_schema: json!({"type":"object"}),
    }];
    for streaming in [false, true] {
        let bytes = provider
            .request(&[Message::user("x")], &tools, None, streaming)
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(
            body["tool_choice"],
            json!({"type":"function","name":"evidence"})
        );
        assert_eq!(body["tools"][0]["strict"], false);
    }
}

// Kimi Runtime emits response.reasoning.delta/done (not reasoning_text.*).
// Fixture shape observed 2026-09-22; all payload content and IDs are synthetic.
fn kimi_reasoning_events() -> (Vec<Value>, Value) {
    let raw = response(vec![
        json!({"id":"rs-kimi", "type":"reasoning", "summary":[], "content":[{"type":"reasoning_text","text":"Check λ."}]}),
        json!({"id":"fc-kimi", "type":"function_call", "status":"completed", "call_id":"call-kimi", "name":"evidence", "arguments":"{\"n\":7}"}),
    ]);
    let mut events = stream_events(&raw);
    let before_done = events
        .iter()
        .position(|event| event["type"] == "response.output_item.done")
        .unwrap();
    events.splice(before_done..before_done, [
        json!({"type":"response.reasoning.delta","output_index":0,"item_id":"rs-kimi","content_index":0,"delta":"Check ","obfuscation":"fixture"}),
        json!({"type":"response.reasoning.delta","output_index":0,"item_id":"rs-kimi","content_index":0,"delta":"λ."}),
        json!({"type":"response.reasoning.done","output_index":0,"item_id":"rs-kimi","content_index":0,"text":"Check λ."}),
    ]);
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(index);
    }
    (events, raw)
}

#[tokio::test]
async fn kimi_reasoning_events_replay_losslessly_through_real_agent() {
    let (events, raw) = kimi_reasoning_events();
    let (mut provider, client, records) =
        provider(vec![sse(events), sse(stream_events(&final_response()))]);
    provider.base_model_id = "moonshotai.kimi-k3".into();
    provider.target = "us.moonshotai.kimi-k3".into();
    let agent = Agent::builder()
        .provider(provider)
        .add_trusted_tool(EvidenceTool)
        .build()
        .await
        .unwrap();
    let answer = agent.run("Read the synthetic count.").await.unwrap();
    assert_eq!(answer.text(), "The count is 7.");
    assert_eq!(answer.tool_calls.len(), 1);
    let requests = client.requests.lock().unwrap();
    assert_eq!(
        &requests[1].0["input"].as_array().unwrap()[1..3],
        raw["output"].as_array().unwrap()
    );
    assert!(records
        .lock()
        .unwrap()
        .iter()
        .all(|record| record.outcome == InvocationOutcome::Completed));
}

#[tokio::test]
async fn kimi_reasoning_mismatch_or_truncation_never_releases_tools() {
    for failure in ["mismatch", "wrong_item", "truncated"] {
        let (mut events, _) = kimi_reasoning_events();
        match failure {
            "mismatch" => {
                let done = events
                    .iter_mut()
                    .find(|event| event["type"] == "response.reasoning.done")
                    .unwrap();
                done["text"] = json!("different");
            }
            "wrong_item" => {
                let delta = events
                    .iter_mut()
                    .find(|event| event["type"] == "response.reasoning.delta")
                    .unwrap();
                delta["item_id"] = json!("other-item");
            }
            "truncated" => {
                events.pop();
            }
            _ => unreachable!(),
        }
        let (provider, _, records) = provider(vec![sse(events)]);
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
        assert_eq!(
            records.lock().unwrap()[0].outcome,
            InvocationOutcome::Failed
        );
    }
}

#[test]
fn kimi_response_cache_matches_the_published_runtime_contract() {
    let (mut provider, _, _) = provider(vec![]);
    provider.base_model_id = "moonshotai.kimi-k3".into();
    provider.target = "us.moonshotai.kimi-k3".into();
    let provider = provider.with_prompt_cache(BedrockResponsesCache {
        mode: BedrockResponsesCacheMode::Explicit,
        system: true,
        messages: [0].into(),
        ..Default::default()
    });
    let request = provider
        .request(
            &[Message::user("synthetic records")],
            &[],
            Some("instructions"),
            true,
        )
        .unwrap();
    let body: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(
        body["prompt_cache_options"],
        json!({"mode":"explicit", "ttl":"30m"})
    );
    for item in body["input"].as_array().unwrap() {
        assert_eq!(
            item["content"][0]["prompt_cache_breakpoint"],
            json!({"mode":"explicit"})
        );
    }
    assert_eq!(body["store"], false);
    assert!(body.get("prompt_cache_key").is_none());
}

#[test]
fn kimi_empty_explicit_cache_cannot_claim_to_disable_caching() {
    let (mut provider, _, _) = provider(vec![]);
    provider.base_model_id = "moonshotai.kimi-k3".into();
    provider.target = "us.moonshotai.kimi-k3".into();
    let provider = provider.with_prompt_cache(BedrockResponsesCache {
        mode: BedrockResponsesCacheMode::Explicit,
        ..Default::default()
    });
    assert!(provider.validate_configuration().is_err());
}
