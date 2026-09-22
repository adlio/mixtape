use std::sync::{Arc, Mutex};

use mixtape_core::{
    Agent, ContentBlock, Message, ModelProvider, ModelResponse, ProviderError, StopReason, Tool,
    ToolDefinition, ToolError, ToolResult, ToolUseBlock,
};
use schemars::JsonSchema;
use serde::Deserialize;

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
        "Return a synthetic count"
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(input.n.to_string()))
    }
}

#[derive(Clone)]
struct ReplayProvider {
    requests: Arc<Mutex<Vec<Vec<Message>>>>,
}

#[async_trait::async_trait]
impl ModelProvider for ReplayProvider {
    fn name(&self) -> &str {
        "Offline replay fixture"
    }
    fn max_context_tokens(&self) -> usize {
        128_000
    }
    fn max_output_tokens(&self) -> usize {
        4096
    }

    async fn generate(
        &self,
        messages: Vec<Message>,
        _tools: Vec<ToolDefinition>,
        _system_prompt: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        let mut requests = self.requests.lock().unwrap();
        requests.push(messages);
        if requests.len() == 1 {
            Ok(ModelResponse {
                message: Message::assistant_with_content(vec![
                    ContentBlock::Thinking {
                        thinking: "fixture reasoning".into(),
                        signature: "fixture signature".into(),
                    },
                    ContentBlock::Text("Checking the evidence.".into()),
                    ContentBlock::RedactedThinking {
                        data: "AP+A".into(),
                    },
                    ContentBlock::ToolUse(ToolUseBlock {
                        id: "tool-1".into(),
                        name: "evidence".into(),
                        input: serde_json::json!({"n": 7}),
                    }),
                ]),
                stop_reason: StopReason::ToolUse,
                usage: None,
            })
        } else {
            Ok(ModelResponse {
                message: Message::assistant("The count is 7."),
                stop_reason: StopReason::EndTurn,
                usage: None,
            })
        }
    }
}

#[tokio::test]
async fn reasoning_and_tool_results_survive_the_real_agent_loop() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = ReplayProvider {
        requests: requests.clone(),
    };
    let agent = Agent::builder()
        .provider(provider)
        .add_trusted_tool(EvidenceTool)
        .build()
        .await
        .unwrap();
    let result = agent.run("Get the synthetic count.").await.unwrap();
    assert_eq!(result.text(), "The count is 7.");
    assert_eq!(result.model_calls, 2);
    assert_eq!(result.tool_calls.len(), 1);
    let requests = requests.lock().unwrap();
    let replay = &requests[1][1].content;
    assert_eq!(
        replay.len(),
        4,
        "Display deltas must not duplicate replay blocks"
    );
    assert!(
        matches!(&replay[0], ContentBlock::Thinking { thinking, signature }
        if thinking == "fixture reasoning" && signature == "fixture signature")
    );
    assert!(matches!(&replay[1], ContentBlock::Text(text) if text == "Checking the evidence."));
    assert!(matches!(&replay[2], ContentBlock::RedactedThinking { data } if data == "AP+A"));
    assert!(matches!(&replay[3], ContentBlock::ToolUse(tool) if tool.id == "tool-1"));
    assert!(
        matches!(&requests[1][2].content[0], ContentBlock::ToolResult(result) if result.tool_use_id == "tool-1")
    );
}

struct TruncatedProvider;

#[async_trait::async_trait]
impl ModelProvider for TruncatedProvider {
    fn name(&self) -> &str {
        "Truncated fixture"
    }
    fn max_context_tokens(&self) -> usize {
        128_000
    }
    fn max_output_tokens(&self) -> usize {
        4096
    }
    async fn generate(
        &self,
        _: Vec<Message>,
        _: Vec<ToolDefinition>,
        _: Option<String>,
    ) -> Result<ModelResponse, ProviderError> {
        Err(ProviderError::Other("Use fixture stream".into()))
    }
    async fn generate_stream(
        &self,
        _: Vec<Message>,
        _: Vec<ToolDefinition>,
        _: Option<String>,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<mixtape_core::StreamEvent, ProviderError>>,
        ProviderError,
    > {
        Ok(Box::pin(futures::stream::iter([Ok(
            mixtape_core::StreamEvent::TextDelta("partial answer".into()),
        )])))
    }
}

#[tokio::test]
async fn partial_text_without_a_stop_event_is_not_success() {
    let agent = Agent::builder()
        .provider(TruncatedProvider)
        .build()
        .await
        .unwrap();
    let result = agent.run("A synthetic request").await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("without a stop event"));
}
