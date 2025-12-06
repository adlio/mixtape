//! Shared retry logic for model providers
//!
//! This module provides exponential backoff with jitter for retrying
//! transient errors like rate limiting, service unavailability, and
//! network issues.

use super::ProviderError;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Configuration for retry behavior on transient errors (throttling, rate limits)
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (default: 8)
    pub max_attempts: usize,
    /// Base delay in milliseconds for exponential backoff (default: 500ms)
    pub base_delay_ms: u64,
    /// Maximum delay cap in milliseconds (default: 30000ms)
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 8,
            base_delay_ms: 500,
            max_delay_ms: 30_000,
        }
    }
}

/// Information about a retry attempt
#[derive(Debug, Clone)]
pub struct RetryInfo {
    /// Which attempt this is (1-based)
    pub attempt: usize,
    /// Maximum attempts configured
    pub max_attempts: usize,
    /// How long we'll wait before retrying
    pub delay: Duration,
    /// The error that triggered the retry
    pub error: String,
}

/// Callback type for retry events
pub type RetryCallback = Arc<dyn Fn(RetryInfo) + Send + Sync>;

/// Determine if an error is transient and should be retried
pub fn is_retryable_error(err: &ProviderError) -> bool {
    match err {
        // These are transient and should be retried
        ProviderError::RateLimited(_) => true,
        ProviderError::ServiceUnavailable(_) => true,
        ProviderError::Network(_) => true,
        ProviderError::Communication(_) => true,

        // These are permanent and should not be retried
        ProviderError::Authentication(_) => false,
        ProviderError::Configuration(_) => false,
        ProviderError::Model(_) => false,
        ProviderError::Other(_) => false,
    }
}

/// Calculate backoff delay for a given attempt using exponential backoff with jitter
pub fn backoff_delay(attempt: usize, config: &RetryConfig) -> Duration {
    let shift = (attempt.saturating_sub(1)).min(10) as u32;
    let exp = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
    let base = config.base_delay_ms.saturating_mul(exp);
    let capped = base.min(config.max_delay_ms);
    let jittered = jitter_ms(capped);
    Duration::from_millis(jittered)
}

/// Apply ±20% jitter to a base delay
fn jitter_ms(base_ms: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as i64;
    let jitter_pct = (nanos % 41) - 20; // -20..20
    let base = base_ms as i64;
    let jittered = base + (base * jitter_pct / 100);
    jittered.max(0) as u64
}

/// Retry an async operation with exponential backoff
///
/// Only retries on transient errors (rate limiting, service unavailable, network).
/// Permanent errors (authentication, configuration, model) fail immediately.
///
/// # Example
///
/// ```ignore
/// let result = retry_with_backoff(
///     || async { provider.generate(messages.clone(), tools.clone(), system.clone()).await },
///     &config,
///     &Some(Arc::new(|info| eprintln!("Retry {}: {}", info.attempt, info.error))),
/// ).await?;
/// ```
pub async fn retry_with_backoff<F, Fut, T>(
    mut op: F,
    config: &RetryConfig,
    on_retry: &Option<RetryCallback>,
) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ProviderError>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match op().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                if attempt >= config.max_attempts || !is_retryable_error(&err) {
                    return Err(err);
                }
                let delay = backoff_delay(attempt, config);

                // Notify callback if set
                if let Some(callback) = on_retry {
                    callback(RetryInfo {
                        attempt,
                        max_attempts: config.max_attempts,
                        delay,
                        error: err.to_string(),
                    });
                }

                tokio::time::sleep(delay).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 8);
        assert_eq!(config.base_delay_ms, 500);
        assert_eq!(config.max_delay_ms, 30_000);
    }

    #[test]
    fn test_is_retryable_error_rate_limited() {
        assert!(is_retryable_error(&ProviderError::RateLimited(
            "too many requests".into()
        )));
    }

    #[test]
    fn test_is_retryable_error_service_unavailable() {
        assert!(is_retryable_error(&ProviderError::ServiceUnavailable(
            "503".into()
        )));
    }

    #[test]
    fn test_is_retryable_error_network() {
        assert!(is_retryable_error(&ProviderError::Network(
            "connection refused".into()
        )));
    }

    #[test]
    fn test_is_retryable_error_communication() {
        assert!(is_retryable_error(&ProviderError::Communication(
            "timeout".into()
        )));
    }

    #[test]
    fn test_is_retryable_error_not_retryable() {
        // Authentication errors should not be retried
        assert!(!is_retryable_error(&ProviderError::Authentication(
            "bad creds".into()
        )));

        // Configuration errors should not be retried
        assert!(!is_retryable_error(&ProviderError::Configuration(
            "invalid model".into()
        )));

        // Model errors should not be retried
        assert!(!is_retryable_error(&ProviderError::Model(
            "content filtered".into()
        )));

        // Generic errors should not be retried
        assert!(!is_retryable_error(&ProviderError::Other("unknown".into())));
    }

    #[test]
    fn test_backoff_delay_first_attempt() {
        let config = RetryConfig::default();
        let delay = backoff_delay(1, &config);

        // First attempt: base_delay (500ms) * 2^0 = 500ms, with jitter
        // Allow for ±20% jitter
        assert!(delay.as_millis() >= 400);
        assert!(delay.as_millis() <= 600);
    }

    #[test]
    fn test_backoff_delay_exponential_growth() {
        let config = RetryConfig {
            base_delay_ms: 100,
            max_delay_ms: 10_000,
            max_attempts: 10,
        };

        let delay1 = backoff_delay(1, &config);
        let delay2 = backoff_delay(2, &config);
        let delay3 = backoff_delay(3, &config);

        // Each delay should roughly double (accounting for jitter)
        // delay1 ~ 100ms, delay2 ~ 200ms, delay3 ~ 400ms
        assert!(delay2.as_millis() > delay1.as_millis());
        assert!(delay3.as_millis() > delay2.as_millis());
    }

    #[test]
    fn test_backoff_delay_respects_max() {
        let config = RetryConfig {
            base_delay_ms: 1000,
            max_delay_ms: 2000,
            max_attempts: 10,
        };

        // After several attempts, should cap at max_delay_ms
        let delay = backoff_delay(10, &config);
        // With jitter, should be around 2000ms ± 20%
        assert!(delay.as_millis() <= 2400);
    }

    #[test]
    fn test_jitter_ms_produces_variation() {
        // Jitter should produce values within ±20% of base
        let base = 1000u64;

        // Call multiple times and verify range
        // Due to deterministic time-based jitter, we just verify it's in range
        let jittered = jitter_ms(base);
        assert!(jittered >= 800); // base - 20%
        assert!(jittered <= 1200); // base + 20%
    }

    #[tokio::test]
    async fn test_retry_with_backoff_success_first_try() {
        let config = RetryConfig {
            max_attempts: 3,
            base_delay_ms: 10,
            max_delay_ms: 100,
        };

        let mut call_count = 0;
        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Ok::<_, ProviderError>("success") }
            },
            &config,
            &None,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    async fn test_retry_with_backoff_retries_on_transient_error() {
        let config = RetryConfig {
            max_attempts: 3,
            base_delay_ms: 1, // Very short for testing
            max_delay_ms: 10,
        };

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let result = retry_with_backoff(
            || {
                let count = count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if count < 2 {
                        Err(ProviderError::RateLimited("throttled".into()))
                    } else {
                        Ok("success after retry")
                    }
                }
            },
            &config,
            &None,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success after retry");
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_with_backoff_gives_up_after_max_attempts() {
        let config = RetryConfig {
            max_attempts: 2,
            base_delay_ms: 1,
            max_delay_ms: 10,
        };

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let result: Result<(), ProviderError> = retry_with_backoff(
            || {
                count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Err(ProviderError::RateLimited("always throttled".into())) }
            },
            &config,
            &None,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_with_backoff_no_retry_on_permanent_error() {
        let config = RetryConfig {
            max_attempts: 5,
            base_delay_ms: 1,
            max_delay_ms: 10,
        };

        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = call_count.clone();

        let result: Result<(), ProviderError> = retry_with_backoff(
            || {
                count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Err(ProviderError::Authentication("bad credentials".into())) }
            },
            &config,
            &None,
        )
        .await;

        assert!(result.is_err());
        // Should only try once since auth errors are not retryable
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_with_backoff_callback_invoked() {
        let config = RetryConfig {
            max_attempts: 3,
            base_delay_ms: 1,
            max_delay_ms: 10,
        };

        let callback_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callback_count_clone = callback_count.clone();

        let callback: RetryCallback = Arc::new(move |info: RetryInfo| {
            callback_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(info.attempt > 0);
            assert_eq!(info.max_attempts, 3);
        });

        let attempt = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempt_clone = attempt.clone();

        let _result: Result<(), ProviderError> = retry_with_backoff(
            || {
                let count = attempt_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if count < 2 {
                        Err(ProviderError::ServiceUnavailable("503".into()))
                    } else {
                        Ok(())
                    }
                }
            },
            &config,
            &Some(callback),
        )
        .await;

        // Callback should be invoked for each retry (not the initial attempt)
        assert_eq!(callback_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
