//! Get migration tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::with_connection;

use super::{ensure_migrations_table, MIGRATIONS_TABLE};

/// Input for getting a specific migration
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetMigrationInput {
    /// The version identifier of the migration to retrieve
    pub version: String,

    /// Database to get the migration from (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Gets details of a specific migration including its SQL
pub struct GetMigrationTool;

impl Tool for GetMigrationTool {
    type Input = GetMigrationInput;

    fn name(&self) -> &str {
        "sqlite_get_migration"
    }

    fn description(&self) -> &str {
        "Get full details of a specific migration by version, including the SQL statement."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let version_input = input.version;

        let (version, name, sql, applied_at, checksum) =
            with_connection(input.db_path, move |conn| {
                // Ensure migrations table exists
                ensure_migrations_table(conn)?;

                let query = format!(
                    "SELECT version, name, sql, applied_at, checksum FROM {MIGRATIONS_TABLE} \
                 WHERE version = ?1"
                );

                let migration = conn.query_row(&query, [&version_input], |row| {
                    let version: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let sql: String = row.get(2)?;
                    let applied_at: Option<String> = row.get(3)?;
                    let checksum: String = row.get(4)?;

                    Ok((version, name, sql, applied_at, checksum))
                });

                match migration {
                    Ok(m) => Ok(m),
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        Err(SqliteToolError::MigrationNotFound(version_input))
                    }
                    Err(e) => Err(e.into()),
                }
            })
            .await?;

        Ok(ToolResult::Json(serde_json::json!({
            "status": "success",
            "version": version,
            "name": name,
            "sql": sql,
            "migration_status": if applied_at.is_some() { "applied" } else { "pending" },
            "applied_at": applied_at,
            "checksum": checksum
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::migration::add::AddMigrationInput;
    use crate::sqlite::migration::AddMigrationTool;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_get_migration() {
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

        // Get the migration
        let get_tool = GetMigrationTool;
        let result = get_tool
            .execute(GetMigrationInput {
                version: version.clone(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["version"], version);
        assert_eq!(json["name"], "create users table");
        assert_eq!(json["sql"], "CREATE TABLE users (id INTEGER PRIMARY KEY);");
        assert_eq!(json["migration_status"], "pending");
        assert!(json["checksum"].as_str().unwrap().len() == 64);
    }

    #[tokio::test]
    async fn test_get_migration_not_found() {
        let db = TestDatabase::new().await;

        let tool = GetMigrationTool;
        let result = tool
            .execute(GetMigrationInput {
                version: "nonexistent".to_string(),
                db_path: Some(db.key()),
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Migration not found"));
    }
}
