//! Error types for the Anthropic SDK

use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;

// ============================================================================
// API Error Types
// ============================================================================

/// API error response wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorResponse {
    #[serde(rename = "type")]
    pub error_type: String,
    pub error: ApiError,
}

/// API error details
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

// ============================================================================
// SDK Error Types
// ============================================================================

/// Errors that can occur when using the Anthropic API
#[derive(Debug, Error)]
pub enum AnthropicError {
    /// Authentication failed (invalid API key)
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// Rate limited by the API
    #[error("Rate limited: {0}")]
    RateLimited(String),

    /// Service unavailable or overloaded
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Invalid request (bad parameters, etc.)
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Invalid response (failed to parse API response)
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Model error (content filter, context length, etc.)
    #[error("Model error: {0}")]
    Model(String),

    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Streaming error
    #[error("Stream error: {0}")]
    Stream(String),

    /// Configuration error (missing API key, etc.)
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Other/unknown error
    #[error("{0}")]
    Other(String),
}

impl AnthropicError {
    /// Returns true if this error is retryable
    ///
    /// Retryable errors include:
    /// - Rate limiting (429)
    /// - Service unavailable/overloaded (503, 529)
    /// - Network errors (connection, timeout)
    /// - Request timeout (408)
    /// - Conflict (409)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AnthropicError::RateLimited(_)
                | AnthropicError::ServiceUnavailable(_)
                | AnthropicError::Network(_)
        )
    }

    /// Returns true if this error is retryable based on HTTP status code
    ///
    /// Following Go SDK behavior: connection errors, 408, 409, 429, 5xx
    pub fn is_retryable_status(status_code: u16) -> bool {
        matches!(status_code, 408 | 409 | 429 | 500..=599)
    }

    /// Classify an API error response into an appropriate error variant
    pub fn from_api_error(error: &ApiError, status_code: u16) -> Self {
        let msg = error.message.clone();
        let error_type = error.error_type.as_str();

        match (status_code, error_type) {
            (401, _) | (_, "authentication_error") => AnthropicError::Authentication(msg),
            (429, _) | (_, "rate_limit_error") => AnthropicError::RateLimited(msg),
            (503, _) | (529, _) | (_, "overloaded_error") => {
                AnthropicError::ServiceUnavailable(msg)
            }
            (400, _) | (_, "invalid_request_error") => AnthropicError::InvalidRequest(msg),
            (_, "not_found_error") => AnthropicError::InvalidRequest(msg),
            _ => AnthropicError::Other(msg),
        }
    }

    /// Classify an HTTP error into an appropriate error variant
    pub fn from_reqwest_error(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            AnthropicError::Network(format!("Request timed out: {}", err))
        } else if err.is_connect() {
            AnthropicError::Network(format!("Connection failed: {}", err))
        } else if err.is_request() {
            AnthropicError::Network(format!("Request failed: {}", err))
        } else if let Some(status) = err.status() {
            match status.as_u16() {
                401 => AnthropicError::Authentication(err.to_string()),
                429 => AnthropicError::RateLimited(err.to_string()),
                500..=599 => AnthropicError::ServiceUnavailable(err.to_string()),
                _ => AnthropicError::Other(err.to_string()),
            }
        } else {
            AnthropicError::Other(err.to_string())
        }
    }
}

/// Configuration for automatic retry behavior
///
/// Follows Go SDK retry semantics:
/// - Exponential backoff: base_delay × 2^attempt with jitter
/// - Maximum delay capped at 8 seconds
/// - Respects Retry-After and Retry-After-Ms headers
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (default: 2)
    pub max_retries: u32,

    /// Base delay for exponential backoff (default: 500ms)
    pub base_delay: Duration,

    /// Maximum delay between retries (default: 8s)
    pub max_delay: Duration,

    /// Jitter factor (0.0-1.0) to add randomness to delays (default: 0.25)
    pub jitter: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
            jitter: 0.25,
        }
    }
}

impl RetryConfig {
    /// Create a new retry config with the specified max retries
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Default::default()
        }
    }

    /// Disable retries
    pub fn disabled() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Calculate the delay for a given retry attempt (0-indexed)
    ///
    /// Uses exponential backoff with jitter: base_delay × 2^attempt × (1 ± jitter)
    pub(crate) fn delay_for_attempt(&self, attempt: u32) -> Duration {
        use rand::Rng;

        // Calculate base exponential delay
        let base = self.base_delay.as_secs_f64() * 2_f64.powi(attempt as i32);

        // Apply jitter
        let jitter_range = base * self.jitter;
        let jitter = rand::thread_rng().gen_range(-jitter_range..=jitter_range);
        let delay_secs = (base + jitter).max(0.0);

        // Cap at max delay
        let delay = Duration::from_secs_f64(delay_secs);
        delay.min(self.max_delay)
    }

    /// Parse retry delay from response headers
    ///
    /// Checks for:
    /// - `retry-after-ms`: milliseconds (Anthropic-specific)
    /// - `retry-after`: seconds or HTTP date
    pub(crate) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
        // Check for retry-after-ms first (Anthropic-specific header)
        if let Some(value) = headers.get("retry-after-ms") {
            if let Ok(s) = value.to_str() {
                if let Ok(ms) = s.parse::<u64>() {
                    return Some(Duration::from_millis(ms));
                }
            }
        }

        // Check for standard retry-after header
        if let Some(value) = headers.get(reqwest::header::RETRY_AFTER) {
            if let Ok(s) = value.to_str() {
                // Try parsing as seconds
                if let Ok(secs) = s.parse::<u64>() {
                    return Some(Duration::from_secs(secs));
                }
                // Note: We don't parse HTTP dates for simplicity
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== is_retryable Tests =====

    #[test]
    fn test_is_retryable_rate_limited() {
        let err = AnthropicError::RateLimited("Too many requests".to_string());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_service_unavailable() {
        let err = AnthropicError::ServiceUnavailable("503 Service Unavailable".to_string());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_retryable_network() {
        let err = AnthropicError::Network("Connection refused".to_string());
        assert!(err.is_retryable());
    }

    #[test]
    fn test_is_not_retryable_authentication() {
        let err = AnthropicError::Authentication("Invalid API key".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_not_retryable_invalid_request() {
        let err = AnthropicError::InvalidRequest("Bad parameters".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_not_retryable_model_error() {
        let err = AnthropicError::Model("Context length exceeded".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_not_retryable_configuration() {
        let err = AnthropicError::Configuration("Missing API key".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_not_retryable_stream() {
        let err = AnthropicError::Stream("Stream interrupted".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_is_not_retryable_other() {
        let err = AnthropicError::Other("Unknown error".to_string());
        assert!(!err.is_retryable());
    }

    // ===== from_api_error Tests =====

    #[test]
    fn test_from_api_error_authentication_by_status() {
        let api_error = ApiError {
            error_type: "some_error".to_string(),
            message: "Unauthorized".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 401);
        assert!(matches!(err, AnthropicError::Authentication(_)));
    }

    #[test]
    fn test_from_api_error_authentication_by_type() {
        let api_error = ApiError {
            error_type: "authentication_error".to_string(),
            message: "Invalid key".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 403);
        assert!(matches!(err, AnthropicError::Authentication(_)));
    }

    #[test]
    fn test_from_api_error_rate_limited_by_status() {
        let api_error = ApiError {
            error_type: "some_error".to_string(),
            message: "Too many requests".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 429);
        assert!(matches!(err, AnthropicError::RateLimited(_)));
    }

    #[test]
    fn test_from_api_error_rate_limited_by_type() {
        let api_error = ApiError {
            error_type: "rate_limit_error".to_string(),
            message: "Slow down".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 200);
        assert!(matches!(err, AnthropicError::RateLimited(_)));
    }

    #[test]
    fn test_from_api_error_service_unavailable_503() {
        let api_error = ApiError {
            error_type: "some_error".to_string(),
            message: "Service unavailable".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 503);
        assert!(matches!(err, AnthropicError::ServiceUnavailable(_)));
    }

    #[test]
    fn test_from_api_error_service_unavailable_529() {
        let api_error = ApiError {
            error_type: "some_error".to_string(),
            message: "Overloaded".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 529);
        assert!(matches!(err, AnthropicError::ServiceUnavailable(_)));
    }

    #[test]
    fn test_from_api_error_overloaded_by_type() {
        let api_error = ApiError {
            error_type: "overloaded_error".to_string(),
            message: "System overloaded".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 200);
        assert!(matches!(err, AnthropicError::ServiceUnavailable(_)));
    }

    #[test]
    fn test_from_api_error_invalid_request_by_status() {
        let api_error = ApiError {
            error_type: "some_error".to_string(),
            message: "Bad request".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 400);
        assert!(matches!(err, AnthropicError::InvalidRequest(_)));
    }

    #[test]
    fn test_from_api_error_invalid_request_by_type() {
        let api_error = ApiError {
            error_type: "invalid_request_error".to_string(),
            message: "Invalid params".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 200);
        assert!(matches!(err, AnthropicError::InvalidRequest(_)));
    }

    #[test]
    fn test_from_api_error_not_found() {
        let api_error = ApiError {
            error_type: "not_found_error".to_string(),
            message: "Resource not found".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 404);
        assert!(matches!(err, AnthropicError::InvalidRequest(_)));
    }

    #[test]
    fn test_from_api_error_unknown() {
        let api_error = ApiError {
            error_type: "mystery_error".to_string(),
            message: "Something weird".to_string(),
        };
        let err = AnthropicError::from_api_error(&api_error, 418);
        assert!(matches!(err, AnthropicError::Other(_)));
    }

    // ===== RetryConfig Tests =====

    #[test]
    fn test_retry_config_new() {
        let config = RetryConfig::new(5);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.base_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(8));
    }

    #[test]
    fn test_retry_config_disabled() {
        let config = RetryConfig::disabled();
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn test_delay_for_attempt_respects_max() {
        let config = RetryConfig {
            max_retries: 10,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            jitter: 0.0, // No jitter for predictable test
        };

        // Attempt 10 would be 1s * 2^10 = 1024s, but capped at 5s
        let delay = config.delay_for_attempt(10);
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn test_delay_for_attempt_with_jitter() {
        let config = RetryConfig {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            jitter: 0.25,
        };

        // First attempt should be around 100ms ± 25%
        let delay = config.delay_for_attempt(0);
        assert!(delay.as_millis() >= 75);
        assert!(delay.as_millis() <= 125);
    }

    // ===== Error Display Tests =====

    #[test]
    fn test_error_display_authentication() {
        let err = AnthropicError::Authentication("Invalid key".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Authentication failed"));
        assert!(display.contains("Invalid key"));
    }

    #[test]
    fn test_error_display_rate_limited() {
        let err = AnthropicError::RateLimited("Slow down".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Rate limited"));
    }

    #[test]
    fn test_error_display_network() {
        let err = AnthropicError::Network("Connection failed".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Network error"));
    }

    #[test]
    fn test_error_from_json_error() {
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let err: AnthropicError = json_err.into();
        assert!(matches!(err, AnthropicError::Json(_)));
    }

    // ===== InvalidResponse Tests =====

    #[test]
    fn test_invalid_response_construction() {
        let err = AnthropicError::InvalidResponse("Failed to parse response".to_string());
        assert!(matches!(err, AnthropicError::InvalidResponse(_)));
    }

    #[test]
    fn test_invalid_response_display() {
        let err = AnthropicError::InvalidResponse("JSON parse error".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Invalid response"));
        assert!(display.contains("JSON parse error"));
    }

    #[test]
    fn test_invalid_response_is_not_retryable() {
        // InvalidResponse errors should not be retryable since they indicate
        // a parsing issue, not a transient server error
        let err = AnthropicError::InvalidResponse("Parse failed".to_string());
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_invalid_response_with_parse_error_message() {
        // Simulate the format used in client.rs line 254
        let parse_error = "expected value at line 1 column 1";
        let err =
            AnthropicError::InvalidResponse(format!("Failed to parse response: {}", parse_error));

        let display = format!("{}", err);
        assert!(display.contains("Failed to parse response"));
        assert!(display.contains("expected value"));
    }
}
