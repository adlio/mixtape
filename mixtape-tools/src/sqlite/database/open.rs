//! Open database tool

use crate::prelude::*;
use crate::sqlite::manager::DATABASE_MANAGER;
use std::path::PathBuf;

/// Input for opening a database
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenDatabaseInput {
    /// Database file path
    pub db_path: PathBuf,

    /// Whether to create the database if it doesn't exist (default: true)
    #[serde(default = "default_create")]
    pub create: bool,
}

fn default_create() -> bool {
    true
}

/// Tool for opening or creating a SQLite database connection
///
/// This tool opens a database file and makes it available for subsequent operations.
/// If `create` is true (default), the database will be created if it doesn't exist.
/// The first opened database becomes the default for operations that don't specify one.
pub struct OpenDatabaseTool;

impl Tool for OpenDatabaseTool {
    type Input = OpenDatabaseInput;

    fn name(&self) -> &str {
        "sqlite_open_database"
    }

    fn description(&self) -> &str {
        "Open or create a SQLite database file. The database becomes available for subsequent operations. If create=true (default), creates the database if it doesn't exist."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let result = tokio::task::spawn_blocking(move || {
            DATABASE_MANAGER.open(&input.db_path, input.create)
        })
        .await
        .map_err(|e| ToolError::Custom(format!("Task join error: {}", e)))?;

        match result {
            Ok(db_name) => {
                let response = serde_json::json!({
                    "status": "success",
                    "database": db_name,
                    "message": format!("Database opened successfully: {}", db_name)
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
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_open_database_create() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("new_test.db");

        let tool = OpenDatabaseTool;
        let input = OpenDatabaseInput {
            db_path: db_path.clone(),
            create: true,
        };

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        // Verify file was created
        assert!(db_path.exists());

        // Clean up only this database
        let key = db_path
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let _ = DATABASE_MANAGER.close(&key);
    }

    #[tokio::test]
    async fn test_open_database_no_create_missing() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("nonexistent.db");

        let tool = OpenDatabaseTool;
        let input = OpenDatabaseInput {
            db_path,
            create: false,
        };

        let result = tool.execute(input).await;
        assert!(result.is_err());
        // No cleanup needed - database was never opened
    }

    #[test]
    fn test_tool_metadata() {
        let tool = OpenDatabaseTool;
        assert_eq!(tool.name(), "sqlite_open_database");
        assert!(!tool.description().is_empty());
    }
}
