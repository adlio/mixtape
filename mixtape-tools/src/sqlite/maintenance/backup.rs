//! Backup database tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;
use chrono::Local;
use std::path::PathBuf;

/// Input for database backup
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BackupDatabaseInput {
    /// Path to the source database file. If not specified, uses the default database.
    #[serde(default)]
    pub source_db_path: Option<String>,

    /// Destination path for the backup. If not specified, creates a timestamped
    /// backup in the same directory as the source.
    #[serde(default)]
    pub backup_path: Option<PathBuf>,
}

/// Tool for creating database backups (SAFE)
///
/// Creates a backup copy of the database. If no backup path is specified,
/// creates a timestamped backup in the same directory.
pub struct BackupDatabaseTool;

impl Tool for BackupDatabaseTool {
    type Input = BackupDatabaseInput;

    fn name(&self) -> &str {
        "sqlite_backup"
    }

    fn description(&self) -> &str {
        "Create a backup copy of the database. Optionally specify a destination path, or let it create a timestamped backup automatically."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let backup_path = input.backup_path;

        let (path, size) = with_connection(input.source_db_path, move |conn| {
            // Get source database path
            let source_db_path: String = conn
                .query_row("PRAGMA database_list", [], |row| row.get(2))
                .map_err(|_| {
                    SqliteToolError::QueryError("Could not get database path".to_string())
                })?;

            let source_db_pathbuf = PathBuf::from(&source_db_path);

            // Determine backup path
            let dest_path = match backup_path {
                Some(p) => p,
                None => {
                    // Create timestamped backup in same directory
                    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
                    let stem = source_db_pathbuf
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("database");
                    let ext = source_db_pathbuf
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("db");

                    let backup_name = format!("{}_{}.{}", stem, timestamp, ext);
                    source_db_pathbuf
                        .parent()
                        .map(|p| p.join(backup_name))
                        .unwrap_or_else(|| PathBuf::from(format!("backup_{}.db", timestamp)))
                }
            };

            // Use SQLite's backup API through VACUUM INTO (SQLite 3.27+)
            // This creates a consistent backup even while the database is in use
            let backup_sql = format!("VACUUM INTO '{}'", dest_path.to_string_lossy());

            conn.execute(&backup_sql, [])
                .map_err(|e| SqliteToolError::QueryError(format!("Backup failed: {}", e)))?;

            // Get backup file size
            let size = std::fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);

            Ok((dest_path.to_string_lossy().to_string(), size))
        })
        .await?;

        let response = serde_json::json!({
            "status": "success",
            "backup_path": path,
            "size_bytes": size,
            "message": format!("Database backed up to: {}", path)
        });
        Ok(ToolResult::Json(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_backup_database() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE test (id INTEGER);
             INSERT INTO test VALUES (1);",
        )
        .await;

        // Create backup with explicit path
        let backup_path = db.path().parent().unwrap().join("backup.db");
        let tool = BackupDatabaseTool;
        let input = BackupDatabaseInput {
            source_db_path: Some(db.key()),
            backup_path: Some(backup_path.clone()),
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["status"].as_str().unwrap(), "success");
        assert!(backup_path.exists());
    }

    #[tokio::test]
    async fn test_backup_auto_path() {
        let db = TestDatabase::new().await;

        let tool = BackupDatabaseTool;
        let input = BackupDatabaseInput {
            source_db_path: Some(db.key()),
            backup_path: None,
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["status"].as_str().unwrap(), "success");
        let backup_path = json["backup_path"].as_str().unwrap();
        assert!(backup_path.contains("test_"));
        assert!(std::path::Path::new(backup_path).exists());
    }

    #[test]
    fn test_tool_metadata() {
        let tool = BackupDatabaseTool;
        assert_eq!(tool.name(), "sqlite_backup");
        assert!(!tool.description().is_empty());
    }
}
