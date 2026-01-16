//! SQLite-specific error types

use mixtape_core::ToolError;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during SQLite tool operations
#[derive(Debug, Error)]
pub enum SqliteToolError {
    /// Database not found or not opened
    #[error("Database not found: {0}")]
    DatabaseNotFound(String),

    /// No default database is set
    #[error("No default database set. Open a database first or specify one explicitly.")]
    NoDefaultDatabase,

    /// Failed to open/create database connection
    #[error("Failed to connect to database '{path}': {message}")]
    ConnectionFailed { path: PathBuf, message: String },

    /// Database file already exists when create=false
    #[error("Database does not exist: {0}")]
    DatabaseDoesNotExist(PathBuf),

    /// SQLite query execution error
    #[error("Query error: {0}")]
    QueryError(String),

    /// Invalid query for the operation type
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// Transaction-related errors
    #[error("Transaction error: {0}")]
    TransactionError(String),

    /// Permission denied for table operation
    #[error("Permission denied: cannot {operation} table '{table}'")]
    PermissionDenied { operation: String, table: String },

    /// Path validation or filesystem error
    #[error("Path error: {0}")]
    PathError(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Table not found
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// Migration not found
    #[error("Migration not found: {0}")]
    MigrationNotFound(String),

    /// Migration checksum mismatch (integrity violation)
    #[error("Migration checksum mismatch for '{version}': expected {expected}, got {actual}")]
    MigrationChecksumMismatch {
        version: String,
        expected: String,
        actual: String,
    },

    /// Generic SQLite error wrapper
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<SqliteToolError> for ToolError {
    fn from(err: SqliteToolError) -> Self {
        ToolError::Custom(err.to_string())
    }
}

impl From<serde_json::Error> for SqliteToolError {
    fn from(err: serde_json::Error) -> Self {
        SqliteToolError::SerializationError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_not_found_display() {
        let err = SqliteToolError::DatabaseNotFound("test.db".to_string());
        assert_eq!(err.to_string(), "Database not found: test.db");
    }

    #[test]
    fn test_no_default_database_display() {
        let err = SqliteToolError::NoDefaultDatabase;
        assert!(err.to_string().contains("No default database set"));
    }

    #[test]
    fn test_connection_failed_display() {
        let err = SqliteToolError::ConnectionFailed {
            path: PathBuf::from("/tmp/test.db"),
            message: "permission denied".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("/tmp/test.db"));
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn test_database_does_not_exist_display() {
        let err = SqliteToolError::DatabaseDoesNotExist(PathBuf::from("/missing.db"));
        assert!(err.to_string().contains("/missing.db"));
    }

    #[test]
    fn test_query_error_display() {
        let err = SqliteToolError::QueryError("syntax error".to_string());
        assert_eq!(err.to_string(), "Query error: syntax error");
    }

    #[test]
    fn test_invalid_query_display() {
        let err = SqliteToolError::InvalidQuery("SELECT not allowed".to_string());
        assert_eq!(err.to_string(), "Invalid query: SELECT not allowed");
    }

    #[test]
    fn test_transaction_error_display() {
        let err = SqliteToolError::TransactionError("no active transaction".to_string());
        assert_eq!(err.to_string(), "Transaction error: no active transaction");
    }

    #[test]
    fn test_permission_denied_display() {
        let err = SqliteToolError::PermissionDenied {
            operation: "write".to_string(),
            table: "secrets".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Permission denied: cannot write table 'secrets'"
        );
    }

    #[test]
    fn test_path_error_display() {
        let err = SqliteToolError::PathError("invalid path".to_string());
        assert_eq!(err.to_string(), "Path error: invalid path");
    }

    #[test]
    fn test_serialization_error_display() {
        let err = SqliteToolError::SerializationError("invalid JSON".to_string());
        assert_eq!(err.to_string(), "Serialization error: invalid JSON");
    }

    #[test]
    fn test_table_not_found_display() {
        let err = SqliteToolError::TableNotFound("users".to_string());
        assert_eq!(err.to_string(), "Table not found: users");
    }

    #[test]
    fn test_from_sqlite_error() {
        // Create a rusqlite error by trying to prepare an invalid statement
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let sqlite_err = conn.prepare("INVALID SQL SYNTAX").unwrap_err();
        let err: SqliteToolError = sqlite_err.into();
        assert!(err.to_string().contains("SQLite error"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: SqliteToolError = io_err.into();
        assert!(err.to_string().contains("IO error"));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let err: SqliteToolError = json_err.into();
        assert!(err.to_string().contains("Serialization error"));
    }

    #[test]
    fn test_into_tool_error() {
        let err = SqliteToolError::DatabaseNotFound("test.db".to_string());
        let tool_err: ToolError = err.into();
        match tool_err {
            ToolError::Custom(msg) => assert!(msg.contains("test.db")),
            _ => panic!("Expected ToolError::Custom"),
        }
    }
}
