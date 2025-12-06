//! Remove migration tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;

use super::{ensure_migrations_table, MIGRATIONS_TABLE};

/// Input for removing a pending migration
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveMigrationInput {
    /// The version identifier of the migration to remove
    pub version: String,

    /// Database to remove the migration from (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Removes a pending migration from the database
///
/// Only pending (not yet applied) migrations can be removed.
/// Applied migrations cannot be removed to maintain schema integrity.
pub struct RemoveMigrationTool;

impl Tool for RemoveMigrationTool {
    type Input = RemoveMigrationInput;

    fn name(&self) -> &str {
        "sqlite_remove_migration"
    }

    fn description(&self) -> &str {
        "Remove a pending migration from the database. Only pending (not yet applied) migrations \
         can be removed. Use sqlite_list_migrations to see pending migrations."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let version = input.version;

        let name = with_connection(input.db_path, move |conn| {
            // Ensure migrations table exists
            ensure_migrations_table(conn)?;

            // Check if migration exists and get its status
            let query =
                format!("SELECT name, applied_at FROM {MIGRATIONS_TABLE} WHERE version = ?1");

            let result: Result<(String, Option<String>), _> =
                conn.query_row(&query, [&version], |row| Ok((row.get(0)?, row.get(1)?)));

            match result {
                Ok((name, applied_at)) => {
                    if applied_at.is_some() {
                        return Err(SqliteToolError::InvalidQuery(format!(
                            "Cannot remove migration '{}': it has already been applied. \
                             Applied migrations cannot be removed to maintain schema integrity.",
                            version
                        )));
                    }

                    // Delete the pending migration
                    conn.execute(
                        &format!("DELETE FROM {MIGRATIONS_TABLE} WHERE version = ?1"),
                        [&version],
                    )?;

                    Ok(name)
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Err(SqliteToolError::MigrationNotFound(version))
                }
                Err(e) => Err(e.into()),
            }
        })
        .await?;

        Ok(ToolResult::Json(serde_json::json!({
            "status": "success",
            "message": format!("Migration '{}' removed", name)
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::migration::add::AddMigrationInput;
    use crate::sqlite::migration::run::RunMigrationsInput;
    use crate::sqlite::migration::{AddMigrationTool, RunMigrationsTool};
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_remove_pending_migration() {
        let db = TestDatabase::new().await;

        // Add a migration
        let add_tool = AddMigrationTool;
        let add_result = add_tool
            .execute(AddMigrationInput {
                name: "create users table".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let add_json = unwrap_json(add_result);
        let version = add_json["version"].as_str().unwrap().to_string();

        // Remove the migration
        let remove_tool = RemoveMigrationTool;
        let result = remove_tool
            .execute(RemoveMigrationInput {
                version: version.clone(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");

        // Verify migration is gone
        let get_result = crate::sqlite::migration::GetMigrationTool
            .execute(crate::sqlite::migration::get::GetMigrationInput {
                version,
                db_path: Some(db.key()),
            })
            .await;

        assert!(get_result.is_err());
    }

    #[tokio::test]
    async fn test_cannot_remove_applied_migration() {
        let db = TestDatabase::new().await;

        // Add and run a migration
        let add_tool = AddMigrationTool;
        let add_result = add_tool
            .execute(AddMigrationInput {
                name: "create users table".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let add_json = unwrap_json(add_result);
        let version = add_json["version"].as_str().unwrap().to_string();

        // Apply the migration
        RunMigrationsTool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Try to remove it - should fail
        let remove_tool = RemoveMigrationTool;
        let result = remove_tool
            .execute(RemoveMigrationInput {
                version,
                db_path: Some(db.key()),
            })
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already been applied"));
    }

    #[tokio::test]
    async fn test_remove_nonexistent_migration() {
        let db = TestDatabase::new().await;

        let tool = RemoveMigrationTool;
        let result = tool
            .execute(RemoveMigrationInput {
                version: "nonexistent".to_string(),
                db_path: Some(db.key()),
            })
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Migration not found"));
    }
}
