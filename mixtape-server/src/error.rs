//! Error types for the mixtape server.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// Errors that can occur when building a router.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// No endpoints were configured.
    #[error("No endpoints configured. Call .with_agui() before .build()")]
    NoEndpoints,
}

/// Errors that can occur in the mixtape server.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// Error from the agent during execution.
    #[error("Agent error: {0}")]
    Agent(#[from] mixtape_core::AgentError),

    /// Permission-related error.
    #[error("Permission error: {0}")]
    Permission(String),

    /// Invalid request from client.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ServerError::Agent(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            ServerError::Permission(e) => (StatusCode::FORBIDDEN, e.clone()),
            ServerError::InvalidRequest(e) => (StatusCode::BAD_REQUEST, e.clone()),
            ServerError::Internal(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.clone()),
        };

        let body = Json(serde_json::json!({
            "error": message,
            "code": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

/// Result type alias for server operations.
pub type ServerResult<T> = Result<T, ServerError>;

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
