//! Add migration tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

use super::{compute_checksum, ensure_migrations_table, generate_version, MIGRATIONS_TABLE};

/// Input for adding a new migration
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddMigrationInput {
    /// Human-readable description of what this migration does
    /// Example: "add users table", "add email column to users"
    pub name: String,

    /// The SQL DDL statement(s) to execute
    /// Example: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"
    pub sql: String,

    /// Database to add the migration to (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Adds a new pending migration to the database
///
/// The migration is stored but NOT executed. Use `sqlite_run_migrations` to apply it.
/// A unique version identifier is automatically generated based on the current timestamp.
pub struct AddMigrationTool;

impl Tool for AddMigrationTool {
    type Input = AddMigrationInput;

    fn name(&self) -> &str {
        "sqlite_add_migration"
    }

    fn description(&self) -> &str {
        "Add a new pending schema migration to the database. The migration is stored but not \
         executed until sqlite_run_migrations is called. Version is auto-generated from timestamp."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let name = input.name;
        let sql = input.sql;

        let (version, checksum) = with_connection(input.db_path, move |conn| {
            // Ensure migrations table exists
            ensure_migrations_table(conn)?;

            // Generate version and checksum
            let version = generate_version();
            let checksum = compute_checksum(&sql);

            // Insert the migration as pending (applied_at = NULL)
            conn.execute(
                &format!(
                    "INSERT INTO {MIGRATIONS_TABLE} (version, name, sql, applied_at, checksum) \
                     VALUES (?1, ?2, ?3, NULL, ?4)"
                ),
                rusqlite::params![version, name, sql, checksum],
            )?;

            Ok((version, checksum))
        })
        .await?;

        Ok(ToolResult::Json(serde_json::json!({
            "status": "success",
            "version": version,
            "checksum": checksum,
            "migration_status": "pending",
            "message": "Migration added. Use sqlite_run_migrations to apply it."
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_add_migration() {
        let db = TestDatabase::new().await;

        let tool = AddMigrationTool;
        let input = AddMigrationInput {
            name: "create users table".to_string(),
            sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);".to_string(),
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["migration_status"], "pending");
        assert!(json["version"].as_str().unwrap().len() == 22);
        assert!(json["checksum"].as_str().unwrap().len() == 64);
    }

    #[tokio::test]
    async fn test_add_multiple_migrations() {
        let db = TestDatabase::new().await;
        let tool = AddMigrationTool;

        // Add first migration
        let input1 = AddMigrationInput {
            name: "create users table".to_string(),
            sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
            db_path: Some(db.key()),
        };
        let result1 = tool.execute(input1).await.unwrap();
        let json1 = unwrap_json(result1);
        let v1 = json1["version"].as_str().unwrap().to_string();

        // Small delay to ensure different timestamp
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Add second migration
        let input2 = AddMigrationInput {
            name: "create posts table".to_string(),
            sql: "CREATE TABLE posts (id INTEGER PRIMARY KEY);".to_string(),
            db_path: Some(db.key()),
        };
        let result2 = tool.execute(input2).await.unwrap();
        let json2 = unwrap_json(result2);
        let v2 = json2["version"].as_str().unwrap().to_string();

        // Versions should be different and v2 should be greater (later)
        assert_ne!(v1, v2);
        assert!(v2 > v1);
    }
}
