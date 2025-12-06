//! List migrations tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

use super::types::MigrationStatusFilter;
use super::{ensure_migrations_table, MIGRATIONS_TABLE};

/// Input for listing migrations
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListMigrationsInput {
    /// Filter by migration status (default: all)
    #[serde(default)]
    pub filter: MigrationStatusFilter,

    /// Database to list migrations from (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Lists all migrations with their status
///
/// Returns migrations ordered by version (oldest first).
pub struct ListMigrationsTool;

impl Tool for ListMigrationsTool {
    type Input = ListMigrationsInput;

    fn name(&self) -> &str {
        "sqlite_list_migrations"
    }

    fn description(&self) -> &str {
        "List all schema migrations with their status. Filter by 'pending', 'applied', or 'all'. \
         Returns migrations ordered by version (oldest first)."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let filter = input.filter;

        let (migrations, pending_count, applied_count) = with_connection(input.db_path, move |conn| {
            // Ensure migrations table exists
            ensure_migrations_table(conn)?;

            // Build query based on status filter
            let where_clause = match filter {
                MigrationStatusFilter::All => String::new(),
                MigrationStatusFilter::Pending => " WHERE applied_at IS NULL".to_string(),
                MigrationStatusFilter::Applied => " WHERE applied_at IS NOT NULL".to_string(),
            };

            let query = format!(
                "SELECT version, name, applied_at FROM {MIGRATIONS_TABLE}{where_clause} \
                 ORDER BY version ASC"
            );

            let mut stmt = conn.prepare(&query)?;

            let migrations: Vec<serde_json::Value> = stmt
                .query_map([], |row| {
                    let version: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let applied_at: Option<String> = row.get(2)?;

                    Ok(serde_json::json!({
                        "version": version,
                        "name": name,
                        "migration_status": if applied_at.is_some() { "applied" } else { "pending" },
                        "applied_at": applied_at
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let pending_count = migrations
                .iter()
                .filter(|m| m["migration_status"] == "pending")
                .count();
            let applied_count = migrations.len() - pending_count;

            Ok((migrations, pending_count, applied_count))
        })
        .await?;

        Ok(ToolResult::Json(serde_json::json!({
            "status": "success",
            "total": migrations.len(),
            "pending_count": pending_count,
            "applied_count": applied_count,
            "migrations": migrations
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
    async fn test_list_empty() {
        let db = TestDatabase::new().await;

        let tool = ListMigrationsTool;
        let result = tool
            .execute(ListMigrationsInput {
                filter: MigrationStatusFilter::All,
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["total"], 0);
        assert_eq!(json["pending_count"], 0);
        assert_eq!(json["applied_count"], 0);
    }

    #[tokio::test]
    async fn test_list_pending_migrations() {
        let db = TestDatabase::new().await;

        // Add migrations but don't run them
        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "first".to_string(),
                sql: "CREATE TABLE t1 (id INTEGER);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        add_tool
            .execute(AddMigrationInput {
                name: "second".to_string(),
                sql: "CREATE TABLE t2 (id INTEGER);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // List all
        let list_tool = ListMigrationsTool;
        let result = list_tool
            .execute(ListMigrationsInput {
                filter: MigrationStatusFilter::All,
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["total"], 2);
        assert_eq!(json["pending_count"], 2);
        assert_eq!(json["applied_count"], 0);

        // Filter pending only
        let result = list_tool
            .execute(ListMigrationsInput {
                filter: MigrationStatusFilter::Pending,
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["total"], 2);
    }

    #[tokio::test]
    async fn test_list_mixed_status() {
        let db = TestDatabase::new().await;

        // Add first migration
        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "first".to_string(),
                sql: "CREATE TABLE t1 (id INTEGER);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Run it
        let run_tool = RunMigrationsTool;
        run_tool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Add second migration (pending)
        add_tool
            .execute(AddMigrationInput {
                name: "second".to_string(),
                sql: "CREATE TABLE t2 (id INTEGER);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // List all
        let list_tool = ListMigrationsTool;
        let result = list_tool
            .execute(ListMigrationsInput {
                filter: MigrationStatusFilter::All,
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["total"], 2);
        assert_eq!(json["pending_count"], 1);
        assert_eq!(json["applied_count"], 1);

        // Filter applied only
        let result = list_tool
            .execute(ListMigrationsInput {
                filter: MigrationStatusFilter::Applied,
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);
        assert_eq!(json["total"], 1);
        assert_eq!(json["migrations"][0]["name"], "first");
        assert_eq!(json["migrations"][0]["migration_status"], "applied");
    }
}
