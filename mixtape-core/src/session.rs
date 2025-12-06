//! Session management for conversation memory
//!
//! Sessions track conversation history across multiple `agent.run()` calls,
//! allowing agents to maintain context and memory. Sessions are automatically
//! scoped to the current working directory in CLI usage.
//!
//! # Example
//! ```ignore
//! use mixtape_core::{Agent, BedrockProvider};
//!
//! // Sessions must be provided by implementing SessionStore
//! // (e.g., SqliteStore from mixtape-cli)
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[cfg(feature = "session")]
use chrono::{DateTime, Utc};

/// A conversation session
///
/// Sessions track conversation history across multiple agent.run() calls.
/// They are automatically scoped to the current working directory.
#[cfg(feature = "session")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID (auto-generated)
    pub id: String,
    /// When session was created
    pub created_at: DateTime<Utc>,
    /// Last update time
    pub updated_at: DateTime<Utc>,
    /// Directory where session is active
    pub directory: String,
    /// Conversation messages
    pub messages: Vec<SessionMessage>,
}

/// A message in a session
#[cfg(feature = "session")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// Message role
    pub role: MessageRole,
    /// Message content
    pub content: String,
    /// Tool calls made (if any)
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    /// Tool results (if any)
    #[serde(default)]
    pub tool_results: Vec<ToolResult>,
    /// When message was created
    pub timestamp: DateTime<Utc>,
}

/// Role of a message in the conversation
#[cfg(feature = "session")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    /// User message
    User,
    /// Assistant (model) message
    Assistant,
    /// System prompt
    System,
}

/// A tool call made by the assistant
#[cfg(feature = "session")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique ID for this tool use
    pub id: String,
    /// Name of the tool being called
    pub name: String,
    /// JSON-encoded input to the tool
    pub input: String,
}

/// Result from a tool execution
#[cfg(feature = "session")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// ID of the tool call this is a result for
    pub tool_use_id: String,
    /// Whether the tool succeeded
    pub success: bool,
    /// Output content (text or JSON)
    pub content: String,
}

/// Trait for session storage backends
///
/// Implement this to provide custom storage (though SQLite is recommended).
///
/// # Example
/// ```ignore
/// use mixtape_core::session::{SessionStore, Session, SessionSummary, SessionError};
/// use async_trait::async_trait;
///
/// # #[cfg(feature = "session")]
/// struct MyStore;
///
/// # #[cfg(feature = "session")]
/// #[async_trait]
/// impl SessionStore for MyStore {
///     async fn get_or_create_session(&self) -> Result<Session, SessionError> {
///         // Implementation
/// #       unimplemented!()
///     }
///
///     async fn get_session(&self, id: &str) -> Result<Option<Session>, SessionError> {
///         // Implementation
/// #       unimplemented!()
///     }
///
///     async fn save_session(&self, session: &Session) -> Result<(), SessionError> {
///         // Implementation
/// #       unimplemented!()
///     }
///
///     async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
///         // Implementation
/// #       unimplemented!()
///     }
///
///     async fn delete_session(&self, id: &str) -> Result<(), SessionError> {
///         // Implementation
/// #       unimplemented!()
///     }
/// }
/// ```
#[cfg(feature = "session")]
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Get or create session for current directory
    ///
    /// Returns existing session if one exists for this directory,
    /// otherwise creates a new one.
    async fn get_or_create_session(&self) -> Result<Session, SessionError>;

    /// Get session by ID
    async fn get_session(&self, id: &str) -> Result<Option<Session>, SessionError>;

    /// Save session
    async fn save_session(&self, session: &Session) -> Result<(), SessionError>;

    /// List all sessions
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError>;

    /// Delete session
    async fn delete_session(&self, id: &str) -> Result<(), SessionError>;
}

/// Summary of a session (for listing)
#[cfg(feature = "session")]
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// Session ID
    pub id: String,
    /// Directory where session is active
    pub directory: String,
    /// Number of messages in session
    pub message_count: usize,
    /// When session was created
    pub created_at: DateTime<Utc>,
    /// Last update time
    pub updated_at: DateTime<Utc>,
}

/// Errors that can occur during session operations
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(String),
    /// Session not found
    #[error("Session not found: {0}")]
    NotFound(String),
    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
