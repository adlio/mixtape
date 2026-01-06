use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mixtape_core::session::{
    MessageRole, Session, SessionError, SessionMessage, SessionStore, SessionSummary, ToolCall,
    ToolResult,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// SQLite-based session storage
///
/// Sessions are stored in a local SQLite database, scoped to the
/// current working directory.
///
/// # Example
/// ```no_run
/// use mixtape_cli::SqliteStore;
/// use mixtape_core::Agent;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let store = SqliteStore::new(".mixtape/sessions.db")?;
/// // Use with agent
/// # Ok(())
/// # }
/// ```
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Create new SQLite store at path
    ///
    /// Creates database file and tables if they don't exist.
    /// Path can be relative or absolute.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, SessionError> {
        let path = path.into();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SessionError::Storage(format!("Failed to create directory: {}", e)))?;
        }

        let conn = Connection::open(&path)
            .map_err(|e| SessionError::Storage(format!("Failed to open database: {}", e)))?;

        // Initialize schema
        conn.execute_batch(include_str!("schema.sql"))
            .map_err(|e| SessionError::Storage(format!("Failed to initialize schema: {}", e)))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Use default location (.mixtape/sessions.db in current directory)
    pub fn default_location() -> Result<Self, SessionError> {
        Self::new(".mixtape/sessions.db")
    }
}

#[async_trait]
impl SessionStore for SqliteStore {
    async fn get_or_create_session(&self) -> Result<Session, SessionError> {
        let current_dir = std::env::current_dir()
            .map_err(|e| SessionError::Storage(format!("Failed to get current directory: {}", e)))?
            .display()
            .to_string();

        let existing_id: Option<String> = {
            let conn = self.conn.lock().unwrap();

            // Try to find existing session for this directory
            conn.query_row(
                "SELECT id FROM sessions WHERE directory = ? ORDER BY updated_at DESC LIMIT 1",
                params![current_dir],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| SessionError::Storage(e.to_string()))?
        };

        if let Some(id) = existing_id {
            // Load existing session
            self.get_session(&id)
                .await?
                .ok_or_else(|| SessionError::NotFound(id.clone()))
        } else {
            // Create new session
            let now = Utc::now();
            let id = uuid::Uuid::new_v4().to_string();

            {
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "INSERT INTO sessions (id, directory, created_at, updated_at) VALUES (?, ?, ?, ?)",
                    params![id, current_dir, now.timestamp(), now.timestamp()],
                )
                .map_err(|e| SessionError::Storage(e.to_string()))?;
            }

            Ok(Session {
                id,
                created_at: now,
                updated_at: now,
                directory: current_dir,
                messages: Vec::new(),
            })
        }
    }

    async fn get_session(&self, id: &str) -> Result<Option<Session>, SessionError> {
        let conn = self.conn.lock().unwrap();

        // Get session metadata
        let session_row = conn
            .query_row(
                "SELECT id, directory, created_at, updated_at FROM sessions WHERE id = ?",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| SessionError::Storage(e.to_string()))?;

        let Some((id, directory, created_at, updated_at)) = session_row else {
            return Ok(None);
        };

        // Get messages
        let mut stmt = conn
            .prepare(
                "SELECT role, content, tool_calls, tool_results, timestamp
                 FROM messages WHERE session_id = ? ORDER BY idx",
            )
            .map_err(|e| SessionError::Storage(e.to_string()))?;

        let messages = stmt
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| SessionError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SessionError::Storage(e.to_string()))?
            .into_iter()
            .map(
                |(role, content, tool_calls_json, tool_results_json, timestamp)| {
                    let role = match role.as_str() {
                        "User" => MessageRole::User,
                        "Assistant" => MessageRole::Assistant,
                        "System" => MessageRole::System,
                        _ => MessageRole::User,
                    };

                    let tool_calls: Vec<ToolCall> =
                        serde_json::from_str(&tool_calls_json).unwrap_or_default();
                    let tool_results: Vec<ToolResult> =
                        serde_json::from_str(&tool_results_json).unwrap_or_default();

                    SessionMessage {
                        role,
                        content,
                        tool_calls,
                        tool_results,
                        timestamp: DateTime::from_timestamp(timestamp, 0).unwrap_or(Utc::now()),
                    }
                },
            )
            .collect();

        Ok(Some(Session {
            id,
            created_at: DateTime::from_timestamp(created_at, 0).unwrap_or(Utc::now()),
            updated_at: DateTime::from_timestamp(updated_at, 0).unwrap_or(Utc::now()),
            directory,
            messages,
        }))
    }

    async fn save_session(&self, session: &Session) -> Result<(), SessionError> {
        let mut conn = self.conn.lock().unwrap();

        // Use a transaction for atomic save operation
        // This ensures all-or-nothing: either everything saves or nothing does
        let tx = conn
            .transaction()
            .map_err(|e| SessionError::Storage(format!("Failed to begin transaction: {}", e)))?;

        // Update session timestamp
        let now = Utc::now();
        let rows = tx
            .execute(
                "UPDATE sessions SET updated_at = ? WHERE id = ?",
                params![now.timestamp(), session.id],
            )
            .map_err(|e| SessionError::Storage(format!("Failed to update session: {}", e)))?;

        // If session doesn't exist, fail
        if rows == 0 {
            return Err(SessionError::NotFound(session.id.clone()));
        }

        // Delete old messages
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?",
            params![session.id],
        )
        .map_err(|e| SessionError::Storage(format!("Failed to delete old messages: {}", e)))?;

        // Insert new messages
        for (idx, msg) in session.messages.iter().enumerate() {
            let tool_calls_json =
                serde_json::to_string(&msg.tool_calls).map_err(SessionError::Serialization)?;
            let tool_results_json =
                serde_json::to_string(&msg.tool_results).map_err(SessionError::Serialization)?;

            tx.execute(
                "INSERT INTO messages (session_id, idx, role, content, tool_calls, tool_results, timestamp)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![
                    session.id,
                    idx as i64,
                    format!("{:?}", msg.role),
                    msg.content,
                    tool_calls_json,
                    tool_results_json,
                    msg.timestamp.timestamp(),
                ],
            )
            .map_err(|e| SessionError::Storage(format!("Failed to insert message {}: {}", idx, e)))?;
        }

        // Commit transaction - if this fails, all changes are rolled back
        tx.commit()
            .map_err(|e| SessionError::Storage(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT s.id, s.directory, s.created_at, s.updated_at, COUNT(m.id) as msg_count
                 FROM sessions s
                 LEFT JOIN messages m ON s.id = m.session_id
                 GROUP BY s.id
                 ORDER BY s.updated_at DESC",
            )
            .map_err(|e| SessionError::Storage(e.to_string()))?;

        let sessions = stmt
            .query_map(params![], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)? as usize,
                ))
            })
            .map_err(|e| SessionError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SessionError::Storage(e.to_string()))?
            .into_iter()
            .map(
                |(id, directory, created_at, updated_at, message_count)| SessionSummary {
                    id,
                    directory,
                    message_count,
                    created_at: DateTime::from_timestamp(created_at, 0).unwrap_or(Utc::now()),
                    updated_at: DateTime::from_timestamp(updated_at, 0).unwrap_or(Utc::now()),
                },
            )
            .collect();

        Ok(sessions)
    }

    async fn delete_session(&self, id: &str) -> Result<(), SessionError> {
        let conn = self.conn.lock().unwrap();

        let rows = conn
            .execute("DELETE FROM sessions WHERE id = ?", params![id])
            .map_err(|e| SessionError::Storage(e.to_string()))?;

        if rows == 0 {
            Err(SessionError::NotFound(id.to_string()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_and_retrieve_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // Create a session
        let session = store.get_or_create_session().await.unwrap();
        assert!(!session.id.is_empty());
        assert_eq!(session.messages.len(), 0);

        // Retrieve same session
        let retrieved = store.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, session.id);
    }

    #[tokio::test]
    async fn test_save_and_load_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Add a message
        session.messages.push(SessionMessage {
            role: MessageRole::User,
            content: "Hello".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });

        // Save it
        store.save_session(&session).await.unwrap();

        // Load it back
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].content, "Hello");
        assert_eq!(loaded.messages[0].role, MessageRole::User);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // Create a session
        let session = store.get_or_create_session().await.unwrap();

        // List sessions
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, session.id);
    }

    #[tokio::test]
    async fn test_delete_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let session = store.get_or_create_session().await.unwrap();

        // Delete it
        store.delete_session(&session.id).await.unwrap();

        // Should not find it
        let retrieved = store.get_session(&session.id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let result = store.delete_session("nonexistent-id").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SessionError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_save_nonexistent_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // Create a fake session that doesn't exist in the DB
        let fake_session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            directory: "/fake/dir".to_string(),
            messages: vec![],
        };

        // Should fail because session doesn't exist
        let result = store.save_session(&fake_session).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SessionError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_large_session_with_many_messages() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Add 100 messages
        for i in 0..100 {
            session.messages.push(SessionMessage {
                role: if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                content: format!("Message number {}", i),
                tool_calls: vec![],
                tool_results: vec![],
                timestamp: Utc::now(),
            });
        }

        // Save it
        store.save_session(&session).await.unwrap();

        // Load it back
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 100);
        assert_eq!(loaded.messages[0].content, "Message number 0");
        assert_eq!(loaded.messages[99].content, "Message number 99");
    }

    #[tokio::test]
    async fn test_session_with_tool_calls() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Add message with tool calls and results
        session.messages.push(SessionMessage {
            role: MessageRole::Assistant,
            content: "Let me call a tool".to_string(),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "calculator".to_string(),
                input: r#"{"operation": "add", "a": 5, "b": 3}"#.to_string(),
            }],
            tool_results: vec![ToolResult {
                tool_use_id: "call_1".to_string(),
                success: true,
                content: "8".to_string(),
            }],
            timestamp: Utc::now(),
        });

        // Save and reload
        store.save_session(&session).await.unwrap();
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();

        assert_eq!(loaded.messages.len(), 1);
        assert_eq!(loaded.messages[0].tool_calls.len(), 1);
        assert_eq!(loaded.messages[0].tool_calls[0].name, "calculator");
        assert_eq!(loaded.messages[0].tool_results.len(), 1);
        assert_eq!(loaded.messages[0].tool_results[0].content, "8");
    }

    #[tokio::test]
    async fn test_update_existing_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // Create session with one message
        let mut session = store.get_or_create_session().await.unwrap();
        session.messages.push(SessionMessage {
            role: MessageRole::User,
            content: "First message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });
        store.save_session(&session).await.unwrap();

        // Add another message
        session.messages.push(SessionMessage {
            role: MessageRole::Assistant,
            content: "Second message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });
        store.save_session(&session).await.unwrap();

        // Load and verify both messages
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].content, "First message");
        assert_eq!(loaded.messages[1].content, "Second message");
    }

    #[tokio::test]
    async fn test_get_or_create_returns_existing() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // Create first session
        let session1 = store.get_or_create_session().await.unwrap();

        // Get same session again (should not create new)
        let session2 = store.get_or_create_session().await.unwrap();

        assert_eq!(session1.id, session2.id);

        // Verify only one session exists
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_default_location() {
        let store = SqliteStore::default_location().unwrap();

        // Should be able to create a session
        let session = store.get_or_create_session().await.unwrap();
        assert!(!session.id.is_empty());

        // Cleanup
        std::fs::remove_dir_all(".mixtape").ok();
    }

    #[tokio::test]
    async fn test_create_nested_directory() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("deeply/nested/path/test.db");

        // Should create all parent directories
        let store = SqliteStore::new(&db_path).unwrap();
        assert!(db_path.exists());

        // Should work normally
        let session = store.get_or_create_session().await.unwrap();
        assert!(!session.id.is_empty());
    }

    #[tokio::test]
    async fn test_get_nonexistent_session() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // Query for a session that doesn't exist
        let result = store.get_session("nonexistent-id").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_message_roles() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Test all message role types
        session.messages.push(SessionMessage {
            role: MessageRole::User,
            content: "User message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });
        session.messages.push(SessionMessage {
            role: MessageRole::Assistant,
            content: "Assistant message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });
        session.messages.push(SessionMessage {
            role: MessageRole::System,
            content: "System message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });

        store.save_session(&session).await.unwrap();
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();

        assert_eq!(loaded.messages[0].role, MessageRole::User);
        assert_eq!(loaded.messages[1].role, MessageRole::Assistant);
        assert_eq!(loaded.messages[2].role, MessageRole::System);
    }

    #[tokio::test]
    async fn test_session_summary_message_count() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Add 5 messages
        for i in 0..5 {
            session.messages.push(SessionMessage {
                role: MessageRole::User,
                content: format!("Message {}", i),
                tool_calls: vec![],
                tool_results: vec![],
                timestamp: Utc::now(),
            });
        }
        store.save_session(&session).await.unwrap();

        // List sessions should show correct message count
        let sessions = store.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 5);
    }

    #[tokio::test]
    async fn test_session_with_multiple_tool_calls() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Add message with multiple tool calls
        session.messages.push(SessionMessage {
            role: MessageRole::Assistant,
            content: "Using multiple tools".to_string(),
            tool_calls: vec![
                ToolCall {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                    input: r#"{"query": "hello"}"#.to_string(),
                },
                ToolCall {
                    id: "call_2".to_string(),
                    name: "read_file".to_string(),
                    input: r#"{"path": "/tmp/file.txt"}"#.to_string(),
                },
                ToolCall {
                    id: "call_3".to_string(),
                    name: "write_file".to_string(),
                    input: r#"{"path": "/tmp/out.txt", "content": "data"}"#.to_string(),
                },
            ],
            tool_results: vec![
                ToolResult {
                    tool_use_id: "call_1".to_string(),
                    success: true,
                    content: "Search results".to_string(),
                },
                ToolResult {
                    tool_use_id: "call_2".to_string(),
                    success: true,
                    content: "File content".to_string(),
                },
                ToolResult {
                    tool_use_id: "call_3".to_string(),
                    success: false,
                    content: "Permission denied".to_string(),
                },
            ],
            timestamp: Utc::now(),
        });

        store.save_session(&session).await.unwrap();
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();

        assert_eq!(loaded.messages[0].tool_calls.len(), 3);
        assert_eq!(loaded.messages[0].tool_results.len(), 3);
        assert!(!loaded.messages[0].tool_results[2].success);
    }

    #[tokio::test]
    async fn test_session_preserves_timestamps() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Use a specific timestamp
        let specific_time = DateTime::from_timestamp(1700000000, 0).unwrap();
        session.messages.push(SessionMessage {
            role: MessageRole::User,
            content: "Timed message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: specific_time,
        });

        store.save_session(&session).await.unwrap();
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();

        // Timestamp should be preserved (at second precision)
        assert_eq!(
            loaded.messages[0].timestamp.timestamp(),
            specific_time.timestamp()
        );
    }

    #[tokio::test]
    async fn test_empty_tool_calls_and_results() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Message with empty tool arrays
        session.messages.push(SessionMessage {
            role: MessageRole::User,
            content: "Regular message".to_string(),
            tool_calls: vec![],
            tool_results: vec![],
            timestamp: Utc::now(),
        });

        store.save_session(&session).await.unwrap();
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();

        assert!(loaded.messages[0].tool_calls.is_empty());
        assert!(loaded.messages[0].tool_results.is_empty());
    }

    #[tokio::test]
    async fn test_session_directory_matches_current() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let session = store.get_or_create_session().await.unwrap();

        // Directory should match current working directory
        let current_dir = std::env::current_dir().unwrap().display().to_string();
        assert_eq!(session.directory, current_dir);
    }

    #[tokio::test]
    async fn test_list_empty_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        // List sessions on empty store
        let sessions = store.list_sessions().await.unwrap();
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_unicode_content() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = SqliteStore::new(db_path).unwrap();

        let mut session = store.get_or_create_session().await.unwrap();

        // Test unicode content
        session.messages.push(SessionMessage {
            role: MessageRole::User,
            content: "Hello 世界! 🌍 Привет مرحبا".to_string(),
            tool_calls: vec![ToolCall {
                id: "unicode_call".to_string(),
                name: "工具".to_string(),
                input: r#"{"text": "日本語"}"#.to_string(),
            }],
            tool_results: vec![ToolResult {
                tool_use_id: "unicode_call".to_string(),
                success: true,
                content: "Ελληνικά".to_string(),
            }],
            timestamp: Utc::now(),
        });

        store.save_session(&session).await.unwrap();
        let loaded = store.get_session(&session.id).await.unwrap().unwrap();

        assert_eq!(loaded.messages[0].content, "Hello 世界! 🌍 Привет مرحبا");
        assert_eq!(loaded.messages[0].tool_calls[0].name, "工具");
        assert_eq!(loaded.messages[0].tool_results[0].content, "Ελληνικά");
    }
}
