use super::*;
use aws_sdk_bedrockruntime::types::{
    ContentBlockDelta, ContentBlockDeltaEvent, ContentBlockStopEvent, ConverseStreamOutput,
    MessageStopEvent,
};
use futures::StreamExt;
use std::sync::Mutex;

struct PendingClient;
#[async_trait::async_trait]
impl BedrockClient for PendingClient {
    async fn converse(&self, _: ConverseRequest) -> Result<ConverseOutput, ProviderError> {
        futures::future::pending().await
    }
    async fn converse_stream(
        &self,
        _: ConverseRequest,
    ) -> Result<StreamOutputResult, ProviderError> {
        futures::future::pending().await
    }
}

type Records = Arc<Mutex<Vec<BedrockInvocation>>>;
fn provider() -> (BedrockProvider, Records) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let captured = records.clone();
    let provider = BedrockProvider::with_bedrock_client(Arc::new(PendingClient), crate::NovaMicro)
        .with_invocation_callback(move |record| captured.lock().unwrap().push(record));
    (provider, records)
}

#[tokio::test]
async fn cancelled_converse_dispatch_is_measured() {
    let (provider, records) = provider();
    let mut future = Box::pin(provider.generate(vec![Message::user("fixture")], vec![], None));
    assert!(futures::poll!(future.as_mut()).is_pending());
    drop(future);
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
    assert_eq!(records[0].attempts, 1);
    assert_eq!(records[0].sdk_retry_count, None);
}

#[tokio::test]
async fn cancelled_converse_stream_dispatch_is_measured() {
    let (provider, records) = provider();
    let mut future =
        Box::pin(provider.generate_stream(vec![Message::user("fixture")], vec![], None));
    assert!(futures::poll!(future.as_mut()).is_pending());
    drop(future);
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
    assert_eq!(records[0].attempts, 1);
}

fn events() -> Vec<Result<ConverseStreamOutput, ProviderError>> {
    vec![
        Ok(ConverseStreamOutput::ContentBlockDelta(
            ContentBlockDeltaEvent::builder()
                .content_block_index(0)
                .delta(ContentBlockDelta::Text("fixture".into()))
                .build()
                .unwrap(),
        )),
        Ok(ConverseStreamOutput::ContentBlockStop(
            ContentBlockStopEvent::builder()
                .content_block_index(0)
                .build()
                .unwrap(),
        )),
        Ok(ConverseStreamOutput::MessageStop(
            MessageStopEvent::builder()
                .stop_reason(aws_sdk_bedrockruntime::types::StopReason::EndTurn)
                .build()
                .unwrap(),
        )),
    ]
}

#[tokio::test]
async fn unpolled_and_partially_consumed_converse_streams_are_cancelled() {
    for consume in [false, true] {
        let (provider, records) = provider();
        let mut guard = provider.guard("converse_stream");
        guard.attempts.fetch_add(1, Ordering::Relaxed);
        guard.record.request_id = Some("fixture-id".into());
        let mut stream =
            streaming::observe_stream(Box::pin(futures::stream::iter(events())), guard);
        if consume {
            assert!(matches!(
                stream.next().await.unwrap(),
                Ok(StreamEvent::TextDelta(_))
            ));
        }
        drop(stream);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, InvocationOutcome::Cancelled);
        assert_eq!(records[0].request_id.as_deref(), Some("fixture-id"));
    }
}

#[tokio::test]
async fn terminal_converse_stream_outcomes_are_emitted_exactly_once() {
    for truncate in [false, true] {
        let (provider, records) = provider();
        let guard = provider.guard("converse_stream");
        guard.attempts.fetch_add(1, Ordering::Relaxed);
        let mut events = events();
        if truncate {
            events.pop();
        }
        let stream = streaming::observe_stream(Box::pin(futures::stream::iter(events)), guard);
        let results: Vec<_> = stream.collect().await;
        assert_eq!(results.iter().any(Result::is_err), truncate);
        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].outcome,
            if truncate {
                InvocationOutcome::Failed
            } else {
                InvocationOutcome::Completed
            }
        );
    }
}
