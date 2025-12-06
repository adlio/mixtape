//! Vacuum database tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;
use std::path::Path;

/// Input for vacuum operation
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VacuumDatabaseInput {
    /// Database file path. If not specified, uses the default database.
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Tool for optimizing database storage (DESTRUCTIVE)
///
/// Rebuilds the database file, reclaiming unused space and optimizing storage.
/// This can take time for large databases and temporarily locks the database.
pub struct VacuumDatabaseTool;

impl Tool for VacuumDatabaseTool {
    type Input = VacuumDatabaseInput;

    fn name(&self) -> &str {
        "sqlite_vacuum"
    }

    fn description(&self) -> &str {
        "Optimize database storage by rebuilding the database file. Reclaims unused space and defragments the database."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let (size_before, size_after) = with_connection(input.db_path, |conn| {
            // Get database path and size before vacuum
            let db_path: String = conn
                .query_row("PRAGMA database_list", [], |row| row.get(2))
                .unwrap_or_else(|_| "unknown".to_string());

            let size_before = Path::new(&db_path).metadata().map(|m| m.len()).unwrap_or(0);

            // Perform vacuum
            conn.execute("VACUUM", [])?;

            // Get size after vacuum
            let size_after = Path::new(&db_path).metadata().map(|m| m.len()).unwrap_or(0);

            Ok((size_before, size_after))
        })
        .await?;

        let saved = size_before.saturating_sub(size_after);
        let response = serde_json::json!({
            "status": "success",
            "size_before_bytes": size_before,
            "size_after_bytes": size_after,
            "space_reclaimed_bytes": saved,
            "message": format!(
                "Database vacuumed. Size: {} -> {} ({} reclaimed)",
                format_bytes(size_before),
                format_bytes(size_after),
                format_bytes(saved)
            )
        });
        Ok(ToolResult::Json(response))
    }
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_vacuum_database() {
        let db = TestDatabase::with_schema("CREATE TABLE test (id INTEGER, data TEXT);").await;

        // Insert and delete data to create free space
        for i in 0..100 {
            db.execute(&format!(
                "INSERT INTO test VALUES ({}, '{}')",
                i,
                "test data ".repeat(10)
            ));
        }
        db.execute("DELETE FROM test");

        // Vacuum
        let tool = VacuumDatabaseTool;
        let input = VacuumDatabaseInput {
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["status"].as_str().unwrap(), "success");
        assert!(json["size_before_bytes"].as_u64().is_some());
        assert!(json["size_after_bytes"].as_u64().is_some());
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_tool_metadata() {
        let tool = VacuumDatabaseTool;
        assert_eq!(tool.name(), "sqlite_vacuum");
        assert!(!tool.description().is_empty());
    }
}
