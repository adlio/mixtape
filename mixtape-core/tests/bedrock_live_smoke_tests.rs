//! Opt-in live tests. The normal suite skips every networked case.
//!
//! Run only after authorizing live inference. Basic tool-loop cases make at most
//! two requests with 1,024 output tokens each. Extended contracts use fixed call
//! counts and a 4,096-token ceiling; all calls have timeouts and retries disabled.
//! Set MIXTAPE_RUN_BEDROCK_SMOKE=1 and MIXTAPE_SMOKE_ACCOUNT to the expected
//! 12-digit account. Credentials must come from the normal SDK provider; never
//! place credentials in these variables.
//! `cargo test -p mixtape-core --features bedrock --test bedrock_live_smoke_tests -- --ignored --nocapture --test-threads=1`

#![cfg(feature = "bedrock")]

use async_trait::async_trait;
use aws_sdk_bedrockruntime::{config::Region, Client};
use futures::stream::BoxStream;
use mixtape_core::provider::bedrock::BedrockInvocation;
use mixtape_core::{
    Agent, BedrockChatCompletionsProvider, BedrockProvider, BedrockResponsesProvider, Message,
    ModelProvider, ModelResponse, ProviderError, RuntimeBedrockModel, StreamEvent, Tool,
    ToolDefinition, ToolError, ToolResult,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

struct BoundedProvider {
    first: Option<Arc<dyn ModelProvider>>,
    inner: Arc<dyn ModelProvider>,
    calls: AtomicUsize,
}

impl BoundedProvider {
    fn admit(&self) -> Result<&dyn ModelProvider, ProviderError> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        if index >= 2 {
            return Err(ProviderError::Configuration(
                "Synthetic smoke test exhausted its two-call budget".into(),
            ));
        }
        Ok(if index == 0 {
            self.first.as_deref().unwrap_or(self.inner.as_ref())
        } else {
            self.inner.as_ref()
        })
    }
}

#[async_trait]
impl ModelProvider for BoundedProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn max_context_tokens(&self) -> usize {
        self.inner.max_context_tokens()
    }
    fn max_output_tokens(&self) -> usize {
        self.inner.max_output_tokens()
    }
    fn estimate_message_tokens(&self, messages: &[Message]) -> usize {
        self.inner.estimate_message_tokens(messages)
    }
    async fn generate(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        self.admit()?.generate(messages, tools, system).await
    }
    async fn generate_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system: Option<String>,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        self.admit()?.generate_stream(messages, tools, system).await
    }
}

struct AddNumbers;
#[derive(Deserialize, JsonSchema)]
struct AddInput {
    a: i32,
    b: i32,
}
impl Tool for AddNumbers {
    type Input = AddInput;
    fn name(&self) -> &str {
        "add_numbers"
    }
    fn description(&self) -> &str {
        "Add exactly two synthetic integers"
    }
    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::Json(
            json!({"sum": i64::from(input.a) + i64::from(input.b)}),
        ))
    }
}

async fn config() -> aws_config::SdkConfig {
    config_in_region("us-west-2").await
}

async fn config_in_region(region: &str) -> aws_config::SdkConfig {
    assert_eq!(
        std::env::var("MIXTAPE_RUN_BEDROCK_SMOKE").as_deref(),
        Ok("1"),
        "Live inference requires explicit opt-in"
    );
    let account = std::env::var("MIXTAPE_SMOKE_ACCOUNT")
        .expect("Expected account must be supplied explicitly");
    assert!(account.len() == 12 && account.bytes().all(|byte| byte.is_ascii_digit()));
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(region.to_owned()))
        .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(1))
        .load()
        .await;
    assert!(
        config.endpoint_url().is_none(),
        "Smoke tests do not accept endpoint overrides"
    );
    let identity = aws_sdk_sts::Client::new(&config)
        .get_caller_identity()
        .send()
        .await
        .unwrap_or_else(|_| panic!("Could not verify the smoke-test AWS identity"));
    assert_eq!(
        identity.account(),
        Some(account.as_str()),
        "Wrong account; no inference was dispatched"
    );
    assert!(
        identity.arn().is_some_and(|arn| arn.starts_with(&format!(
            "arn:aws:sts::{account}:assumed-role/BedrockInference/"
        ))),
        "Wrong role; no inference was dispatched"
    );
    config
}

async fn run(api: &str) {
    let config = config().await;
    let records = Arc::new(Mutex::new(Vec::<BedrockInvocation>::new()));
    let captured = records.clone();
    let observe = move |record| captured.lock().unwrap().push(record);
    let force = mixtape_core::provider::bedrock::BedrockToolChoice::Tool("add_numbers".into());
    let (first, inner): (Arc<dyn ModelProvider>, Arc<dyn ModelProvider>) = match api {
        "converse" => {
            let provider = BedrockProvider::with_client(
                Client::new(&config),
                RuntimeBedrockModel::new(
                    "Claude Opus 5.0",
                    "anthropic.claude-opus-5",
                    128_000,
                    1024,
                )
                .unwrap(),
            )
            .with_inference_target("us.anthropic.claude-opus-5")
            .unwrap()
            .with_max_tokens(1024)
            .with_max_retries(1)
            .with_invocation_callback(observe);
            (
                Arc::new(
                    provider
                        .clone()
                        .with_disabled_thinking()
                        .with_tool_choice(force),
                ),
                Arc::new(provider),
            )
        }
        "chat" => {
            let provider = BedrockChatCompletionsProvider::from_sdk_config(
                &config,
                RuntimeBedrockModel::new("Kimi K3", "moonshotai.kimi-k3", 128_000, 1024).unwrap(),
            )
            .unwrap()
            .with_inference_target("us.moonshotai.kimi-k3")
            .unwrap()
            .with_reasoning_effort("low")
            .with_max_tokens(1024)
            .with_max_retries(1)
            .with_invocation_callback(observe);
            (
                Arc::new(provider.clone().with_tool_choice(force)),
                Arc::new(provider),
            )
        }
        "responses" => {
            let provider = BedrockResponsesProvider::from_sdk_config(
                &config,
                RuntimeBedrockModel::new("GPT-6 Astra", "openai.gpt-6-astra", 128_000, 1024)
                    .unwrap(),
            )
            .unwrap()
            .with_inference_target("us.openai.gpt-6-astra")
            .unwrap()
            .with_reasoning_effort("low")
            .with_max_tokens(1024)
            .with_max_retries(1)
            .with_invocation_callback(observe);
            (
                Arc::new(provider.clone().with_tool_choice(force)),
                Arc::new(provider),
            )
        }
        _ => panic!("Unknown smoke-test API"),
    };
    let agent = Agent::builder()
        .provider(BoundedProvider {
            first: Some(first),
            inner,
            calls: AtomicUsize::new(0),
        })
        .add_trusted_tool(AddNumbers)
        .build()
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(180), agent.run(
        "Use add_numbers exactly once with a=17 and b=25. After its result, respond with exactly the JSON {\"sum\":42,\"status\":\"ok\"}."
    )).await;
    // Metadata only, also when a call fails or times out. Never print reasoning.
    let records = records.lock().unwrap();
    println!("{}", serde_json::to_string(&*records).unwrap());
    assert!(records.len() <= 2 && records.iter().all(|record| record.attempts == 1));
    let result = result
        .expect("Synthetic smoke test exceeded its timeout")
        .expect("Mixtape tool loop failed");
    assert_eq!(result.model_calls, 2);
    assert_eq!(result.tool_calls.len(), 1);
    let answer: serde_json::Value =
        serde_json::from_str(result.text()).expect("Expected synthetic JSON result");
    assert_eq!(answer, json!({"sum":42,"status":"ok"}));
}

#[tokio::test]
#[ignore = "Requires explicit paid-test approval, expected account and temporary BedrockInference credentials"]
async fn opus_converse_tool_loop() {
    run("converse").await;
}

#[tokio::test]
#[ignore = "Requires explicit paid-test approval, expected account and temporary BedrockInference credentials"]
async fn kimi_chat_tool_loop() {
    run("chat").await;
}

#[tokio::test]
#[ignore = "Requires explicit paid-test approval, expected account and temporary BedrockInference credentials"]
async fn astra_responses_tool_loop() {
    run("responses").await;
}

#[test]
fn smoke_request_budget_cannot_exceed_two_calls() {
    struct UnusedProvider(&'static str);
    #[async_trait]
    impl ModelProvider for UnusedProvider {
        fn name(&self) -> &str {
            self.0
        }
        fn max_context_tokens(&self) -> usize {
            128_000
        }
        fn max_output_tokens(&self) -> usize {
            1024
        }
        async fn generate(
            &self,
            _: Vec<Message>,
            _: Vec<ToolDefinition>,
            _: Option<String>,
        ) -> Result<ModelResponse, ProviderError> {
            unreachable!("No network")
        }
    }
    let provider = BoundedProvider {
        first: Some(Arc::new(UnusedProvider("forced first turn"))),
        inner: Arc::new(UnusedProvider("automatic following turn")),
        calls: AtomicUsize::new(0),
    };
    assert_eq!(provider.admit().unwrap().name(), "forced first turn");
    assert_eq!(provider.admit().unwrap().name(), "automatic following turn");
    assert!(provider.admit().is_err());
}

// Extended contracts run only with the same explicit account/role opt-in. Each
// case makes a fixed number of calls, uses synthetic inputs, and disables retries.
mod contracts {
    use super::*;
    use futures::StreamExt;
    use mixtape_core::provider::bedrock::{
        BedrockCacheTtl, BedrockJsonSchema, BedrockPromptCache, BedrockToolChoice,
        InvocationOutcome,
    };
    use mixtape_core::types::{ContentBlock, StopReason, ToolResultBlock, ToolResultStatus};

    const OPUS: &str = "anthropic.claude-opus-5";
    const KIMI: &str = "moonshotai.kimi-k3";
    const ASTRA: &str = "openai.gpt-6-astra";
    const LIMIT: usize = 4096;

    struct Case {
        name: &'static str,
        records: Arc<Mutex<Vec<BedrockInvocation>>>,
    }

    impl Case {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                records: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn observer(&self) -> impl Fn(BedrockInvocation) + Send + Sync + 'static {
            let records = self.records.clone();
            move |record| records.lock().unwrap().push(record)
        }

        fn target(model: &str) -> String {
            let account = std::env::var("MIXTAPE_SMOKE_ACCOUNT").unwrap();
            format!("arn:aws:bedrock:us-west-2:{account}:inference-profile/us.{model}")
        }

        fn model(id: &str) -> RuntimeBedrockModel {
            RuntimeBedrockModel::new(id, id, 128_000, LIMIT).unwrap()
        }

        fn opus(&self, config: &aws_config::SdkConfig) -> BedrockProvider {
            BedrockProvider::with_client(Client::new(config), Self::model(OPUS))
                .with_inference_target(Self::target(OPUS))
                .unwrap()
                .with_max_tokens(LIMIT as i32)
                .with_max_retries(1)
                .with_invocation_callback(self.observer())
        }

        fn chat(&self, config: &aws_config::SdkConfig) -> BedrockChatCompletionsProvider {
            BedrockChatCompletionsProvider::from_sdk_config(config, Self::model(KIMI))
                .unwrap()
                .with_inference_target(Self::target(KIMI))
                .unwrap()
                .with_max_tokens(LIMIT as i32)
                .with_max_retries(1)
                .with_invocation_callback(self.observer())
        }

        fn responses(
            &self,
            config: &aws_config::SdkConfig,
            model: &str,
        ) -> BedrockResponsesProvider {
            BedrockResponsesProvider::from_sdk_config(config, Self::model(model))
                .unwrap()
                .with_inference_target(Self::target(model))
                .unwrap()
                .with_max_tokens(LIMIT as i32)
                .with_max_retries(1)
                .with_invocation_callback(self.observer())
        }

        fn verify(&self, calls: usize, model: &str, outcome: InvocationOutcome) {
            let records = self.records.lock().unwrap();
            assert_eq!(records.len(), calls);
            for record in records.iter() {
                assert_eq!(record.requested_model, model);
                assert_eq!(record.dispatched_target, Self::target(model));
                assert_eq!(record.endpoint, "bedrock-runtime");
                assert_eq!(record.region.as_deref(), Some("us-west-2"));
                assert_eq!(record.attempts, 1);
                assert_eq!(record.outcome, outcome);
                assert!(record.request_id.as_ref().is_some_and(|id| !id.is_empty()));
                if model == OPUS {
                    // Converse does not return provider-resolved model identity.
                    assert!(record.provider_reported_model.is_none());
                } else {
                    let reported = record.provider_reported_model.as_deref().unwrap();
                    assert!(
                        reported == model
                            || reported == format!("us.{model}")
                            || reported == Self::target(model)
                    );
                    assert_eq!(record.sdk_retry_count, Some(0));
                }
                let usage = record.usage.as_ref().expect("Live usage must be captured");
                assert!(usage.input_tokens.is_some_and(|tokens| tokens > 0));
                assert!(usage.output_tokens.is_some());
            }
        }
    }

    impl Drop for Case {
        fn drop(&mut self) {
            // Also report metadata on assertion failure. Never print messages,
            // tool payloads, reasoning, authorization headers, or credentials.
            let records = self
                .records
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            println!("{}", json!({"case":self.name,"invocations":&*records}));
        }
    }

    async fn complete(
        provider: &dyn ModelProvider,
        streaming: bool,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
        system: Option<String>,
    ) -> ModelResponse {
        tokio::time::timeout(Duration::from_secs(180), async {
            if !streaming {
                return provider.generate(messages, tools, system).await;
            }
            let mut stream = provider.generate_stream(messages, tools, system).await?;
            let mut content = Vec::new();
            let mut text = String::new();
            let mut terminal = None;
            while let Some(event) = stream.next().await {
                assert!(terminal.is_none(), "Unexpected event after Stop");
                match event? {
                    StreamEvent::ContentBlock(block) => content.push(block),
                    StreamEvent::TextDelta(delta) => text.push_str(&delta),
                    StreamEvent::Stop { stop_reason, usage } => {
                        terminal = Some((stop_reason, usage));
                    }
                    StreamEvent::ThinkingDelta(_) | StreamEvent::ToolUse(_) => {}
                }
            }
            let (stop_reason, usage) = terminal.expect("Missing final stream event");
            let message = Message::assistant_with_content(content);
            assert!(
                message.text() == text,
                "Final text disagrees with display deltas"
            );
            Ok(ModelResponse {
                message,
                stop_reason,
                usage,
            })
        })
        .await
        .expect("Live contract timed out")
        .expect("Live provider contract failed")
    }

    fn tool() -> ToolDefinition {
        ToolDefinition {
            name: "add_numbers".into(),
            description: "Add two synthetic integers".into(),
            input_schema: json!({
                "type":"object", "properties":{"a":{"type":"integer"},"b":{"type":"integer"}},
                "required":["a","b"], "additionalProperties":false
            }),
        }
    }

    /// Run one explicitly selected mapped model, never an inferred alias or preview.
    /// Each mode makes two requests: tool call, then serialized tool-result replay.
    #[tokio::test]
    #[ignore = "Requires synthetic inference approval and MIXTAPE_SMOKE_MODEL"]
    async fn catalog_model_tool_replay() {
        let id = std::env::var("MIXTAPE_SMOKE_MODEL")
            .expect("Select one mapped model explicitly with MIXTAPE_SMOKE_MODEL");
        let descriptor = mixtape_core::models::bedrock_model_descriptor(&id)
            .filter(|model| model.model_id == id && !id.contains("fable"))
            .expect("Use a mapped, non-preview base model ID");
        let region = if id == "qwen.qwen3-coder-next" {
            "us-east-1"
        } else {
            "us-west-2"
        };
        let config = config_in_region(region).await;
        let target = if descriptor.requires_inference_profile {
            format!("us.{id}")
        } else {
            id.clone()
        };
        for streaming in [false, true] {
            let case = Case::new(if streaming {
                "catalog_stream_tool_replay"
            } else {
                "catalog_tool_replay"
            });
            let model = descriptor.with_token_limits(128_000, LIMIT).unwrap();
            let (first, following): (Box<dyn ModelProvider>, Box<dyn ModelProvider>) =
                if id.starts_with("openai.") {
                    let provider = BedrockResponsesProvider::from_sdk_config(&config, model)
                        .unwrap()
                        .with_inference_target(&target)
                        .unwrap()
                        .with_max_tokens(LIMIT as i32)
                        .with_max_retries(1)
                        .with_invocation_callback(case.observer());
                    (
                        Box::new(
                            provider
                                .clone()
                                .with_tool_choice(BedrockToolChoice::Tool("add_numbers".into())),
                        ),
                        Box::new(provider),
                    )
                } else if id == KIMI {
                    let provider = BedrockChatCompletionsProvider::from_sdk_config(&config, model)
                        .unwrap()
                        .with_inference_target(&target)
                        .unwrap()
                        .with_max_tokens(LIMIT as i32)
                        .with_max_retries(1)
                        .with_invocation_callback(case.observer());
                    (
                        Box::new(
                            provider
                                .clone()
                                .with_tool_choice(BedrockToolChoice::Tool("add_numbers".into())),
                        ),
                        Box::new(provider),
                    )
                } else {
                    let provider = BedrockProvider::with_client(Client::new(&config), model)
                        .with_inference_target(&target)
                        .unwrap()
                        .with_max_tokens(LIMIT as i32)
                        .with_max_retries(1)
                        .with_invocation_callback(case.observer());
                    let first = if id.starts_with("anthropic.") {
                        provider
                            .clone()
                            .with_disabled_thinking()
                            .with_tool_choice(BedrockToolChoice::Tool("add_numbers".into()))
                    } else {
                        // Named tool forcing is not universal. Do not pretend a
                        // model choosing not to call a tool is a transport defect.
                        provider.clone()
                    };
                    (Box::new(first), Box::new(provider))
                };
            let mut messages = vec![Message::user(
                "Call add_numbers exactly once with a=17 and b=25. After its result, respond only with the JSON {\"sum\":42,\"status\":\"ok\"}. Do not use markdown.",
            )];
            let response = complete(&*first, streaming, messages.clone(), vec![tool()], None).await;
            assert_eq!(
                response.stop_reason,
                StopReason::ToolUse,
                "Model did not choose the requested tool"
            );
            let calls = response.message.tool_uses();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].name, "add_numbers");
            assert_eq!(calls[0].input, json!({"a":17,"b":25}));
            let call_id = calls[0].id.clone();
            messages.push(response.message);
            messages.push(Message::tool_results(vec![ToolResultBlock {
                tool_use_id: call_id,
                content: ToolResult::Json(json!({"sum":42})),
                status: ToolResultStatus::Success,
            }]));
            let serialized = serde_json::to_vec(&messages).unwrap();
            let replayed: Vec<Message> = serde_json::from_slice(&serialized).unwrap();
            let response = complete(&*following, streaming, replayed, vec![tool()], None).await;
            assert_answer(&response);
            let records = case.records.lock().unwrap();
            assert_eq!(records.len(), 2);
            for record in &*records {
                assert_eq!(record.requested_model, id);
                assert_eq!(record.dispatched_target, target);
                assert_eq!(record.region.as_deref(), Some(region));
                assert_eq!(record.attempts, 1);
                assert_eq!(record.outcome, InvocationOutcome::Completed);
                assert!(record.request_id.as_ref().is_some_and(|id| !id.is_empty()));
            }
        }
    }

    fn schema() -> BedrockJsonSchema {
        BedrockJsonSchema {
            name: "arithmetic_result".into(),
            schema: json!({
                "type":"object", "properties":{"sum":{"type":"integer"},"status":{"type":"string","enum":["ok"]}},
                "required":["sum","status"], "additionalProperties":false
            }),
        }
    }

    fn assert_answer(response: &ModelResponse) {
        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert!(response.message.tool_uses().is_empty());
        let answer: serde_json::Value = serde_json::from_str(&response.message.text())
            .expect("Expected a JSON answer, not reasoning or markdown");
        assert_eq!(answer, json!({"sum":42,"status":"ok"}));
    }

    async fn replay(api: &str, streaming: bool) {
        let config = config().await;
        let case = Case::new(match (api, streaming) {
            ("opus", false) => "opus_reasoning_replay",
            ("opus", true) => "opus_stream_reasoning_replay",
            ("chat", false) => "kimi_chat_reasoning_replay",
            ("chat", true) => "kimi_chat_stream_reasoning_replay",
            ("kimi", false) => "kimi_responses_reasoning_replay",
            ("kimi", true) => "kimi_responses_stream_reasoning_replay",
            ("astra", false) => "astra_responses_reasoning_replay",
            ("astra", true) => "astra_responses_stream_reasoning_replay",
            _ => unreachable!(),
        });
        let (provider, model): (Box<dyn ModelProvider>, _) = match api {
            "opus" => (
                Box::new(
                    case.opus(&config)
                        .with_adaptive_thinking()
                        .with_thinking_effort("high"),
                ),
                OPUS,
            ),
            "chat" => (
                Box::new(case.chat(&config).with_reasoning_effort("high")),
                KIMI,
            ),
            "kimi" => (
                Box::new(case.responses(&config, KIMI).with_reasoning_effort("high")),
                KIMI,
            ),
            "astra" => (
                Box::new(case.responses(&config, ASTRA).with_reasoning_effort("high")),
                ASTRA,
            ),
            _ => unreachable!(),
        };
        let mut messages = vec![Message::user(
            "Find integers a,b satisfying 3*a+2*b=101 and 5*a-2*b=35. Call add_numbers exactly once with those values. After the tool returns, reply only with JSON containing sum and status=ok. Do not include markdown."
        )];
        let first = complete(&*provider, streaming, messages.clone(), vec![tool()], None).await;
        assert_eq!(first.stop_reason, StopReason::ToolUse);
        let tools = first.message.tool_uses();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "add_numbers");
        assert_eq!(tools[0].input, json!({"a":17,"b":25}));
        let call_id = tools[0].id.clone();
        let has_reasoning = first.message.content.iter().any(|block| match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                // Opus 5 returns signature-only reasoning (empty visible text).
                // The signature is the replay authority and must not be dropped.
                if model == OPUS {
                    !signature.is_empty()
                } else {
                    !thinking.is_empty()
                }
            }
            ContentBlock::RedactedThinking { data } => !data.is_empty(),
            ContentBlock::ResponsesReplay(replay) => replay.items.iter().any(|item| {
                item["type"] == "reasoning"
                    && (item["encrypted_content"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty())
                        || item["content"]
                            .as_array()
                            .is_some_and(|parts| !parts.is_empty()))
            }),
            _ => false,
        });
        assert!(
            has_reasoning,
            "This run did not exercise replayable reasoning"
        );
        // Serialize/deserialize as persisted conversations do. Compare without a
        // debug assertion that could leak reasoning if the assertion fails.
        let encoded = serde_json::to_vec(&first.message).unwrap();
        let restored: Message = serde_json::from_slice(&encoded).unwrap();
        assert!(serde_json::to_vec(&restored).unwrap() == encoded);
        messages.push(restored);
        messages.push(Message::tool_results(vec![ToolResultBlock {
            tool_use_id: call_id,
            content: ToolResult::Json(json!({"sum":42})),
            status: ToolResultStatus::Success,
        }]));
        let final_response = complete(&*provider, streaming, messages, vec![tool()], None).await;
        assert_answer(&final_response);
        case.verify(2, model, InvocationOutcome::Completed);
    }

    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn opus_reasoning_replay() {
        replay("opus", false).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn opus_stream_reasoning_replay() {
        replay("opus", true).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn kimi_chat_reasoning_replay() {
        replay("chat", false).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn kimi_chat_stream_reasoning_replay() {
        replay("chat", true).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn kimi_responses_reasoning_replay() {
        replay("kimi", false).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn kimi_responses_stream_reasoning_replay() {
        replay("kimi", true).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn astra_responses_reasoning_replay() {
        replay("astra", false).await;
    }
    #[tokio::test]
    #[ignore = "Live synthetic Bedrock contract; two requests"]
    async fn astra_responses_stream_reasoning_replay() {
        replay("astra", true).await;
    }

    async fn output_schema(responses: bool, streaming: bool) {
        let config = config().await;
        let case = Case::new("kimi_native_schema");
        let provider: Box<dyn ModelProvider> = if responses {
            Box::new(
                case.responses(&config, KIMI)
                    .with_reasoning_effort("low")
                    .with_output_schema(schema()),
            )
        } else {
            Box::new(
                case.chat(&config)
                    .with_reasoning_effort("low")
                    .with_output_schema(schema()),
            )
        };
        let answer = complete(
            &*provider,
            streaming,
            vec![Message::user("Compute 17+25. Return sum and status ok.")],
            vec![],
            None,
        )
        .await;
        assert_answer(&answer);
        case.verify(1, KIMI, InvocationOutcome::Completed);
    }

    #[tokio::test]
    #[ignore = "Live native schema contract; one request"]
    async fn kimi_chat_schema() {
        output_schema(false, false).await;
    }
    #[tokio::test]
    #[ignore = "Live native schema contract; one request"]
    async fn kimi_chat_stream_schema() {
        output_schema(false, true).await;
    }
    #[tokio::test]
    #[ignore = "Live native schema contract; one request"]
    async fn kimi_responses_schema() {
        output_schema(true, false).await;
    }
    #[tokio::test]
    #[ignore = "Live native schema contract; one request"]
    async fn kimi_responses_stream_schema() {
        output_schema(true, true).await;
    }

    #[tokio::test]
    #[ignore = "Live forced-tool contracts; four requests"]
    async fn named_tool_choice() {
        let config = config().await;
        for api in ["opus", "chat", "kimi", "astra"] {
            let case = Case::new("named_tool_choice");
            let choice = BedrockToolChoice::Tool("add_numbers".into());
            let (provider, model): (Box<dyn ModelProvider>, _) = match api {
                "opus" => (
                    Box::new(
                        case.opus(&config)
                            .with_disabled_thinking()
                            .with_tool_choice(choice),
                    ),
                    OPUS,
                ),
                "chat" => (
                    Box::new(
                        case.chat(&config)
                            .with_reasoning_effort("low")
                            .with_tool_choice(choice),
                    ),
                    KIMI,
                ),
                "kimi" => (
                    Box::new(
                        case.responses(&config, KIMI)
                            .with_reasoning_effort("low")
                            .with_tool_choice(choice),
                    ),
                    KIMI,
                ),
                "astra" => (
                    Box::new(
                        case.responses(&config, ASTRA)
                            .with_reasoning_effort("low")
                            .with_tool_choice(choice),
                    ),
                    ASTRA,
                ),
                _ => unreachable!(),
            };
            let answer = complete(
                &*provider,
                true,
                vec![Message::user("Add 17 and 25.")],
                vec![tool()],
                None,
            )
            .await;
            assert_eq!(answer.stop_reason, StopReason::ToolUse);
            let calls = answer.message.tool_uses();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].name, "add_numbers");
            assert_eq!(calls[0].input, json!({"a":17,"b":25}));
            case.verify(1, model, InvocationOutcome::Completed);
        }
    }

    fn prefix() -> String {
        // No repository, user, or employee data. Exceeds both 512- and 1024-token
        // cache thresholds without relying on Mixtape's approximate tokenizer.
        (0..160).map(|index| format!("Synthetic record {index}: the blue container holds seventeen counters and the green container holds twenty-five counters.\n")).collect()
    }

    #[tokio::test]
    #[ignore = "Live explicit cache placement and TTL contracts; four requests"]
    async fn opus_cache_checkpoints() {
        let config = config().await;
        for mixed in [false, true] {
            let case = Case::new(if mixed {
                "opus_mixed_ttl_cache"
            } else {
                "opus_five_minute_cache"
            });
            let cache = if mixed {
                BedrockPromptCache {
                    tools: Some(BedrockCacheTtl::OneHour),
                    system: Some(BedrockCacheTtl::OneHour),
                    messages: [(0, BedrockCacheTtl::FiveMinutes)].into(),
                }
            } else {
                BedrockPromptCache {
                    system: Some(BedrockCacheTtl::FiveMinutes),
                    ..Default::default()
                }
            };
            let provider = case
                .opus(&config)
                .with_disabled_thinking()
                .with_prompt_cache(cache);
            let system = format!("Return only JSON with sum and status=ok. These are synthetic reference records:\n{}", prefix());
            let mut definition = tool();
            definition.description.push_str(&prefix());
            let tools = if mixed { vec![definition] } else { vec![] };
            let prompt = "Without using tools, add 17+25. Return only JSON with sum and status=ok.";
            for streaming in [false, true] {
                let answer = complete(
                    &provider,
                    streaming,
                    vec![Message::user(prompt)],
                    tools.clone(),
                    Some(system.clone()),
                )
                .await;
                assert_answer(&answer);
            }
            case.verify(2, OPUS, InvocationOutcome::Completed);
            let records = case.records.lock().unwrap();
            for record in records.iter() {
                let usage = record.usage.as_ref().unwrap();
                assert!(usage.cache_read_input_tokens.is_some());
                assert!(usage.cache_write_input_tokens.is_some());
            }
            // Routing makes hits best-effort. Accepted configuration and actual
            // cache usage are separate evidence; never turn no-hit into a hit.
            println!(
                "{}",
                json!({"case":case.name,"observed_cache_hit":records.iter().any(|record| record.usage.as_ref().unwrap().cache_read_input_tokens.unwrap_or(0)>0)})
            );
        }
    }

    #[tokio::test]
    #[ignore = "Live implicit cache usage; four requests"]
    async fn openai_compatible_implicit_cache() {
        let config = config().await;
        for responses in [false, true] {
            let case = Case::new(if responses {
                "astra_implicit_cache"
            } else {
                "kimi_chat_implicit_cache"
            });
            let (provider, model): (Box<dyn ModelProvider>, _) = if responses {
                (
                    Box::new(case.responses(&config, ASTRA).with_reasoning_effort("low")),
                    ASTRA,
                )
            } else {
                (
                    Box::new(case.chat(&config).with_reasoning_effort("low")),
                    KIMI,
                )
            };
            let system = format!(
                "Reply only with JSON containing sum and status=ok. Synthetic records:\n{}",
                prefix()
            );
            for streaming in [false, true] {
                let answer = complete(
                    &*provider,
                    streaming,
                    vec![Message::user("Add 17 and 25.")],
                    vec![],
                    Some(system.clone()),
                )
                .await;
                assert_answer(&answer);
            }
            case.verify(2, model, InvocationOutcome::Completed);
            let records = case.records.lock().unwrap();
            println!(
                "{}",
                json!({"case":case.name,"observed_cache_hit":records.iter().any(|record| record.usage.as_ref().unwrap().cache_read_input_tokens.unwrap_or(0)>0)})
            );
        }
    }

    #[tokio::test]
    #[ignore = "Live Kimi explicit-cache contract; two requests"]
    async fn kimi_explicit_responses_cache() {
        use mixtape_core::provider::bedrock::{BedrockResponsesCache, BedrockResponsesCacheMode};
        let config = config().await;
        let case = Case::new("kimi_explicit_cache");
        let cache = BedrockResponsesCache {
            mode: BedrockResponsesCacheMode::Explicit,
            system: true,
            ..Default::default()
        };
        let provider = case
            .responses(&config, KIMI)
            .with_reasoning_effort("low")
            .with_prompt_cache(cache);
        let system = format!(
            "Return only JSON containing sum and status=ok. Synthetic explicit cache fixture:\n{}",
            prefix()
        );
        for streaming in [false, true] {
            let answer = complete(
                &provider,
                streaming,
                vec![Message::user("Add 17+25.")],
                vec![],
                Some(system.clone()),
            )
            .await;
            assert_answer(&answer);
        }
        case.verify(2, KIMI, InvocationOutcome::Completed);
        let records = case.records.lock().unwrap();
        for record in records.iter() {
            let usage = record.usage.as_ref().unwrap();
            assert!(usage.cache_read_input_tokens.is_some());
            assert!(usage.cache_write_input_tokens.is_some());
        }
        println!(
            "{}",
            json!({"case":case.name,"observed_cache_hit":records.iter().any(|record| record.usage.as_ref().unwrap().cache_read_input_tokens.unwrap_or(0)>0)})
        );
    }

    #[tokio::test]
    #[ignore = "Live caller-cancellation telemetry; four requests"]
    async fn dropping_live_stream_reports_cancelled_once() {
        let config = config().await;
        for api in ["opus", "chat", "kimi", "astra"] {
            let case = Case::new("dropped_live_stream");
            let (provider, model): (Box<dyn ModelProvider>, _) = match api {
                "opus" => (Box::new(case.opus(&config).with_disabled_thinking()), OPUS),
                "chat" => (
                    Box::new(case.chat(&config).with_reasoning_effort("low")),
                    KIMI,
                ),
                "kimi" => (
                    Box::new(case.responses(&config, KIMI).with_reasoning_effort("low")),
                    KIMI,
                ),
                "astra" => (
                    Box::new(case.responses(&config, ASTRA).with_reasoning_effort("low")),
                    ASTRA,
                ),
                _ => unreachable!(),
            };
            let stream = tokio::time::timeout(
                Duration::from_secs(180),
                provider.generate_stream(
                    vec![Message::user("Return the integers 1 through 40.")],
                    vec![],
                    None,
                ),
            )
            .await
            .expect("Live stream timed out")
            .expect("Live stream dispatch failed");
            drop(stream);
            // This proves caller-side abandonment accounting, not that remote
            // execution stopped or that Bedrock waived in-flight token charges.
            let records = case.records.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].requested_model, model);
            assert_eq!(records[0].dispatched_target, Case::target(model));
            assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
            assert_eq!(records[0].attempts, 1);
            assert!(records[0].request_id.is_some());
        }
    }

    #[tokio::test]
    #[ignore = "Live output truncation contracts; three requests"]
    async fn output_limit_is_not_success() {
        let config = config().await;
        for api in ["opus", "chat", "astra"] {
            let case = Case::new("output_limit_is_not_success");
            let (provider, model): (Box<dyn ModelProvider>, _) = match api {
                "opus" => (
                    Box::new(
                        case.opus(&config)
                            .with_disabled_thinking()
                            .with_max_tokens(16),
                    ),
                    OPUS,
                ),
                "chat" => (
                    Box::new(
                        case.chat(&config)
                            .with_reasoning_effort("low")
                            .with_max_tokens(16),
                    ),
                    KIMI,
                ),
                "astra" => (
                    Box::new(
                        case.responses(&config, ASTRA)
                            .with_reasoning_effort("low")
                            .with_max_tokens(16),
                    ),
                    ASTRA,
                ),
                _ => unreachable!(),
            };
            let answer = complete(
                &*provider,
                true,
                vec![Message::user(
                    "List every integer from 1 to 200, with no abbreviation.",
                )],
                vec![],
                None,
            )
            .await;
            assert_eq!(answer.stop_reason, StopReason::MaxTokens);
            assert!(answer.message.tool_uses().is_empty());
            case.verify(1, model, InvocationOutcome::Rejected);
        }
    }
}
