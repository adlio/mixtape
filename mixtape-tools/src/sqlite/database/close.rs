//! Close database tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::DATABASE_MANAGER;

/// Input for closing a database
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CloseDatabaseInput {
    /// Database file path to close. If not specified, closes the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Tool for closing a SQLite database connection
///
/// Closes an open database connection and releases resources.
/// If the closed database was the default, another open database
/// becomes the new default (if any).
pub struct CloseDatabaseTool;

impl Tool for CloseDatabaseTool {
    type Input = CloseDatabaseInput;

    fn name(&self) -> &str {
        "sqlite_close_database"
    }

    fn description(&self) -> &str {
        "Close an open SQLite database connection. Specify the database name/path, or omit to close the default database."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let db_name = input.db_path.clone();

        let result = tokio::task::spawn_blocking(move || {
            let name = match &db_name {
                Some(n) => n.as_str(),
                None => {
                    DATABASE_MANAGER
                        .get_default()
                        .ok_or(SqliteToolError::NoDefaultDatabase)?
                        .as_str()
                        .to_string();
                    // Get the default and close it
                    let default = DATABASE_MANAGER
                        .get_default()
                        .ok_or(SqliteToolError::NoDefaultDatabase)?;
                    return DATABASE_MANAGER.close(&default);
                }
            };
            DATABASE_MANAGER.close(name)
        })
        .await
        .map_err(|e| ToolError::Custom(format!("Task join error: {}", e)))?;

        match result {
            Ok(()) => {
                let closed_name = input
                    .db_path
                    .unwrap_or_else(|| "default database".to_string());
                let response = serde_json::json!({
                    "status": "success",
                    "message": format!("Database closed: {}", closed_name)
                });
                Ok(ToolResult::Json(response))
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::TestDatabase;

    #[tokio::test]
    async fn test_close_database() {
        // Create an isolated test database
        let db = TestDatabase::new().await;
        let db_key = db.key();

        // Close it explicitly by key
        let close_tool = CloseDatabaseTool;
        let close_input = CloseDatabaseInput {
            db_path: Some(db_key.clone()),
        };

        let result = close_tool.execute(close_input).await;
        assert!(result.is_ok());

        // Verify it's closed
        assert!(!DATABASE_MANAGER.is_open(&db_key));

        // Prevent Drop from trying to close again
        std::mem::forget(db);
    }

    #[test]
    fn test_tool_metadata() {
        let tool = CloseDatabaseTool;
        assert_eq!(tool.name(), "sqlite_close_database");
        assert!(!tool.description().is_empty());
    }
}
