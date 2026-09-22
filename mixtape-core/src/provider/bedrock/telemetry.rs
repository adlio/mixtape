//! Request-boundary measurements. These records contain no prompt, response text,
//! tool arguments, reasoning, credentials, or raw error messages.

use std::time::{Duration, Instant};

use aws_sdk_bedrockruntime::types::TokenUsage;
use serde::Serialize;

/// Outcome of the provider protocol, not validation of an application's summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationOutcome {
    Completed,
    Rejected,
    Failed,
    /// The caller dropped the request or stream before its terminal outcome.
    Cancelled,
}

/// Optional counters are unknown when the provider did not report them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvocationUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_write_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

impl InvocationUsage {
    pub(super) fn from_bedrock(value: &TokenUsage) -> Self {
        Self {
            input_tokens: u64::try_from(value.input_tokens).ok(),
            output_tokens: u64::try_from(value.output_tokens).ok(),
            cache_read_input_tokens: value
                .cache_read_input_tokens
                .and_then(|n| u64::try_from(n).ok()),
            cache_write_input_tokens: value
                .cache_write_input_tokens
                .and_then(|n| u64::try_from(n).ok()),
            // Converse does not expose a separate reasoning-token counter here.
            reasoning_tokens: None,
        }
    }
}

/// One completed or failed provider call, including Mixtape's retry attempts.
#[derive(Debug, Clone, Serialize)]
pub struct BedrockInvocation {
    /// Model ID supplied to Mixtape, not a claim about provider resolution.
    pub requested_model: String,
    /// Exact `modelId` sent at the request boundary; retries use the same target.
    pub dispatched_target: String,
    pub api: &'static str,
    pub endpoint: &'static str,
    pub region: Option<String>,
    /// Converse does not report a resolved model identity. Never fill this from
    /// requested_model or dispatched_target.
    pub provider_reported_model: Option<String>,
    pub request_id: Option<String>,
    pub provider_stop_reason: Option<String>,
    pub outcome: InvocationOutcome,
    /// Calls into the SDK, including Mixtape retries. SDK-internal HTTP retry
    /// counts are not exposed and are reported separately as unknown.
    pub attempts: usize,
    pub sdk_retry_count: Option<usize>,
    pub elapsed_ms: u64,
    pub provider_latency_ms: Option<u64>,
    pub usage: Option<InvocationUsage>,
}

impl BedrockInvocation {
    pub(super) fn new(
        requested_model: &str,
        target: String,
        api: &'static str,
        region: Option<String>,
    ) -> Self {
        Self {
            requested_model: requested_model.into(),
            dispatched_target: target,
            api,
            endpoint: "bedrock-runtime",
            region,
            provider_reported_model: None,
            request_id: None,
            provider_stop_reason: None,
            outcome: InvocationOutcome::Failed,
            attempts: 0,
            sdk_retry_count: None,
            elapsed_ms: 0,
            provider_latency_ms: None,
            usage: None,
        }
    }

    pub(super) fn finish(&mut self, started: Instant, attempts: usize, outcome: InvocationOutcome) {
        self.elapsed_ms = millis(started.elapsed());
        self.attempts = attempts;
        self.outcome = outcome;
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

pub(super) fn stop_outcome(reason: &str) -> InvocationOutcome {
    match reason {
        "end_turn" | "stop_sequence" | "tool_use" | "pause_turn" => InvocationOutcome::Completed,
        _ => InvocationOutcome::Rejected,
    }
}

/// Owned by an in-flight request and then by its returned stream. Creating this
/// outside the stream body also measures abandonment before the first poll.
pub(super) struct InvocationGuard {
    pub record: BedrockInvocation,
    pub attempts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub last_request_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    started: Instant,
    callback: Option<std::sync::Arc<dyn Fn(BedrockInvocation) + Send + Sync>>,
    emitted: bool,
}

impl InvocationGuard {
    pub fn new(
        mut record: BedrockInvocation,
        callback: Option<std::sync::Arc<dyn Fn(BedrockInvocation) + Send + Sync>>,
        sdk_retry_count: Option<usize>,
    ) -> Self {
        record.sdk_retry_count = sdk_retry_count;
        Self {
            record,
            attempts: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            last_request_id: std::sync::Arc::new(std::sync::Mutex::new(None)),
            started: Instant::now(),
            callback,
            emitted: false,
        }
    }

    pub fn finish(&mut self, outcome: InvocationOutcome) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        self.record.request_id = self.record.request_id.clone().or_else(|| {
            self.last_request_id
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        });
        self.record.finish(
            self.started,
            self.attempts.load(std::sync::atomic::Ordering::Relaxed),
            outcome,
        );
        if let Some(callback) = &self.callback {
            callback(self.record.clone());
        }
    }
}

impl Drop for InvocationGuard {
    fn drop(&mut self) {
        if self.attempts.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            self.finish(InvocationOutcome::Cancelled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_usage_and_provider_identity_are_not_fabricated() {
        let record = BedrockInvocation::new(
            "anthropic.claude-opus-5",
            "us.anthropic.claude-opus-5".into(),
            "converse",
            None,
        );
        assert!(record.provider_reported_model.is_none());
        assert!(record.usage.is_none());
        assert!(record.sdk_retry_count.is_none());
        let usage = TokenUsage::builder()
            .input_tokens(10)
            .output_tokens(2)
            .total_tokens(12)
            .build()
            .unwrap();
        let usage = InvocationUsage::from_bedrock(&usage);
        assert_eq!(usage.input_tokens, Some(10));
        assert!(usage.cache_read_input_tokens.is_none());
        assert!(usage.cache_write_input_tokens.is_none());
        assert!(usage.reasoning_tokens.is_none());
    }

    #[test]
    fn explicit_zero_usage_is_preserved() {
        let usage = TokenUsage::builder()
            .input_tokens(0)
            .output_tokens(0)
            .total_tokens(0)
            .cache_read_input_tokens(0)
            .cache_write_input_tokens(0)
            .build()
            .unwrap();
        let usage = InvocationUsage::from_bedrock(&usage);
        assert_eq!(usage.input_tokens, Some(0));
        assert_eq!(usage.cache_read_input_tokens, Some(0));
        assert_eq!(usage.cache_write_input_tokens, Some(0));
        assert_eq!(stop_outcome("refusal"), InvocationOutcome::Rejected);
        assert_eq!(
            stop_outcome("guardrail_intervened"),
            InvocationOutcome::Rejected
        );
        assert_eq!(stop_outcome("max_tokens"), InvocationOutcome::Rejected);
    }
}
