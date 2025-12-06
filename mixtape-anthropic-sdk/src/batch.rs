//! Batch API types for the Anthropic Messages API
//!
//! This module contains types for the Message Batches API, which allows
//! processing large volumes of message requests asynchronously.
//!
//! # Example
//!
//! ```no_run
//! // Requires ANTHROPIC_API_KEY environment variable
//! use mixtape_anthropic_sdk::{Anthropic, BatchCreateParams, BatchRequest, MessageCreateParams};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Anthropic::from_env()?;
//!
//! let requests = vec![
//!     BatchRequest {
//!         custom_id: "request-1".to_string(),
//!         params: MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
//!             .user("Hello!")
//!             .build(),
//!     },
//! ];
//!
//! let batch = client.batches().create(BatchCreateParams { requests }).await?;
//! println!("Batch ID: {}", batch.id);
//! # Ok(())
//! # }
//! ```

use crate::messages::{Message, MessageCreateParams};
use serde::{Deserialize, Serialize};

/// A single request within a batch
#[derive(Debug, Clone, Serialize)]
pub struct BatchRequest {
    /// Developer-provided ID for matching results to requests
    pub custom_id: String,

    /// The message creation parameters
    pub params: MessageCreateParams,
}

impl BatchRequest {
    /// Create a new batch request
    ///
    /// # Example
    ///
    /// ```
    /// use mixtape_anthropic_sdk::{BatchRequest, MessageCreateParams};
    ///
    /// let request = BatchRequest::new(
    ///     "request-1",
    ///     MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    ///         .user("Hello!")
    ///         .build(),
    /// );
    /// ```
    pub fn new(custom_id: impl Into<String>, params: MessageCreateParams) -> Self {
        Self {
            custom_id: custom_id.into(),
            params,
        }
    }
}

/// Parameters for creating a message batch
#[derive(Debug, Clone, Serialize)]
pub struct BatchCreateParams {
    /// List of requests to process
    pub requests: Vec<BatchRequest>,
}

/// A message batch
#[derive(Debug, Clone, Deserialize)]
pub struct MessageBatch {
    /// Unique batch identifier
    pub id: String,

    /// Object type (always "message_batch")
    #[serde(rename = "type")]
    pub batch_type: String,

    /// Processing status
    pub processing_status: BatchStatus,

    /// Request counts by status
    pub request_counts: BatchRequestCounts,

    /// URL to results file (available when ended)
    pub results_url: Option<String>,

    /// When the batch was created (RFC 3339)
    pub created_at: String,

    /// When the batch will expire (RFC 3339)
    pub expires_at: String,

    /// When processing ended (RFC 3339, if ended)
    pub ended_at: Option<String>,

    /// When the batch was archived (RFC 3339, if archived)
    pub archived_at: Option<String>,

    /// When cancellation was initiated (RFC 3339, if canceling)
    pub cancel_initiated_at: Option<String>,
}

/// Processing status of a batch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    /// Batch is being processed
    InProgress,

    /// Cancellation has been initiated
    Canceling,

    /// Processing has ended
    Ended,
}

/// Request counts within a batch
#[derive(Debug, Clone, Deserialize)]
pub struct BatchRequestCounts {
    /// Requests still processing
    pub processing: u32,

    /// Requests that succeeded
    pub succeeded: u32,

    /// Requests that errored
    pub errored: u32,

    /// Requests that were canceled
    pub canceled: u32,

    /// Requests that expired
    pub expired: u32,
}

/// Result of an individual batch request
#[derive(Debug, Clone, Deserialize)]
pub struct BatchResult {
    /// The custom_id from the request
    pub custom_id: String,

    /// The result details
    pub result: BatchResultType,
}

/// Type of batch result
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BatchResultType {
    /// Request succeeded
    Succeeded {
        /// The message response
        message: Message,
    },

    /// Request errored
    Errored {
        /// Error details
        error: BatchError,
    },

    /// Request was canceled
    Canceled,

    /// Request expired
    Expired,
}

/// Error details for a failed batch request
#[derive(Debug, Clone, Deserialize)]
pub struct BatchError {
    /// Error type
    #[serde(rename = "type")]
    pub error_type: String,

    /// Error message
    pub message: String,
}

/// Response from listing batches
#[derive(Debug, Clone, Deserialize)]
pub struct BatchListResponse {
    /// List of batches
    pub data: Vec<MessageBatch>,

    /// Whether there are more results
    pub has_more: bool,

    /// ID of the first item (for pagination)
    pub first_id: Option<String>,

    /// ID of the last item (for pagination)
    pub last_id: Option<String>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_status_deserialization() {
        let json = r#""in_progress""#;
        let status: BatchStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, BatchStatus::InProgress);
    }

    #[test]
    fn test_batch_result_type_succeeded() {
        let json = r#"{
            "type": "succeeded",
            "message": {
                "id": "msg_123",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-20250514",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 20}
            }
        }"#;
        let result: BatchResultType = serde_json::from_str(json).unwrap();
        assert!(matches!(result, BatchResultType::Succeeded { .. }));
    }

    #[test]
    fn test_batch_result_type_errored() {
        let json = r#"{
            "type": "errored",
            "error": {
                "type": "invalid_request_error",
                "message": "Bad request"
            }
        }"#;
        let result: BatchResultType = serde_json::from_str(json).unwrap();
        assert!(matches!(result, BatchResultType::Errored { .. }));
    }

    #[test]
    fn test_batch_request_new_with_str() {
        let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
            .user("Hello!")
            .build();

        let request = BatchRequest::new("request-1", params.clone());

        assert_eq!(request.custom_id, "request-1");
        assert_eq!(request.params.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_batch_request_new_with_string() {
        let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
            .user("Hello!")
            .build();

        let custom_id = String::from("request-2");
        let request = BatchRequest::new(custom_id, params);

        assert_eq!(request.custom_id, "request-2");
    }

    #[test]
    fn test_batch_request_new_serialization() {
        let params = MessageCreateParams::builder("test-model", 100)
            .user("test message")
            .build();

        let request = BatchRequest::new("test-id", params);
        let json = serde_json::to_string(&request).unwrap();

        assert!(json.contains("\"custom_id\":\"test-id\""));
        assert!(json.contains("\"model\":\"test-model\""));
        assert!(json.contains("\"max_tokens\":100"));
    }
}
