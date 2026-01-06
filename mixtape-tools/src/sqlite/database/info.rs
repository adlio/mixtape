//! Database info tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;
use crate::sqlite::types::DatabaseInfo;
use std::path::Path;

/// Input for getting database info
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DatabaseInfoInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Tool for retrieving database metadata and statistics
///
/// Returns comprehensive information about a database including:
/// - File size
/// - Table, index, view, and trigger counts
/// - SQLite version and configuration
pub struct DatabaseInfoTool;

impl Tool for DatabaseInfoTool {
    type Input = DatabaseInfoInput;

    fn name(&self) -> &str {
        "sqlite_database_info"
    }

    fn description(&self) -> &str {
        "Get comprehensive metadata and statistics about a SQLite database including file size, table counts, indexes, and configuration."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let result = with_connection(input.db_path, |conn| {
            // Get database file path
            let path: String = conn
                .query_row("PRAGMA database_list", [], |row| row.get(2))
                .unwrap_or_else(|_| "unknown".to_string());

            // Get file size
            let size_bytes = Path::new(&path)
                .metadata()
                .map(|m| m.len())
                .unwrap_or(0);

            // Count tables
            let table_count: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get::<_, i64>(0).map(|v| v as usize),
                )
                .unwrap_or(0);

            // Count indexes
            let index_count: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index'",
                    [],
                    |row| row.get::<_, i64>(0).map(|v| v as usize),
                )
                .unwrap_or(0);

            // Count views
            let view_count: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view'",
                    [],
                    |row| row.get::<_, i64>(0).map(|v| v as usize),
                )
                .unwrap_or(0);

            // Count triggers
            let trigger_count: usize = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger'",
                    [],
                    |row| row.get::<_, i64>(0).map(|v| v as usize),
                )
                .unwrap_or(0);

            // Get SQLite version
            let sqlite_version: String = conn
                .query_row("SELECT sqlite_version()", [], |row| row.get(0))
                .unwrap_or_else(|_| "unknown".to_string());

            // Get page size
            let page_size: i64 = conn
                .query_row("PRAGMA page_size", [], |row| row.get(0))
                .unwrap_or(0);

            // Get page count
            let page_count: i64 = conn
                .query_row("PRAGMA page_count", [], |row| row.get(0))
                .unwrap_or(0);

            // Check if WAL mode
            let journal_mode: String = conn
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap_or_else(|_| "delete".to_string());
            let wal_mode = journal_mode.to_lowercase() == "wal";

            Ok(DatabaseInfo {
                path,
                size_bytes,
                table_count,
                index_count,
                view_count,
                trigger_count,
                sqlite_version,
                page_size,
                page_count,
                wal_mode,
            })
        })
        .await?;

        Ok(ToolResult::Json(serde_json::to_value(result)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_database_info() {
        let db =
            TestDatabase::with_schema("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);")
                .await;

        // Get database info with explicit reference
        let info_tool = DatabaseInfoTool;
        let info_input = DatabaseInfoInput {
            db_path: Some(db.key()),
        };

        let result = info_tool.execute(info_input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["table_count"].as_i64().unwrap(), 1);
        assert!(json["sqlite_version"].as_str().is_some());
    }

    #[test]
    fn test_tool_metadata() {
        let tool = DatabaseInfoTool;
        assert_eq!(tool.name(), "sqlite_database_info");
        assert!(!tool.description().is_empty());
    }
}
