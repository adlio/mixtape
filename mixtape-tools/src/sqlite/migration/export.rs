//! Export migrations tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

use super::{ensure_migrations_table, Migration, MigrationStatusFilter, MIGRATIONS_TABLE};

/// Input for exporting migrations
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportMigrationsInput {
    /// Database to export migrations from (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,

    /// Filter by migration status (default: all)
    #[serde(default)]
    pub filter: MigrationStatusFilter,

    /// Export format (default: json)
    #[serde(default)]
    pub format: ExportFormat,
}

/// Export format options
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JSON array of migration records
    #[default]
    Json,
    /// SQL script that can be executed to recreate schema
    Sql,
}

/// Exports migrations from the database for transfer to another database
///
/// This tool exports migration records that can be imported into another
/// database using `sqlite_import_migrations`. This preserves the full
/// migration history and audit trail.
pub struct ExportMigrationsTool;

impl Tool for ExportMigrationsTool {
    type Input = ExportMigrationsInput;

    fn name(&self) -> &str {
        "sqlite_export_migrations"
    }

    fn description(&self) -> &str {
        "Export migrations from the database for transfer to another database. \
         Exports migration records (version, name, SQL, status) that can be imported \
         elsewhere using sqlite_import_migrations. Use filter to export only pending \
         or applied migrations."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let filter = input.filter;
        let format = input.format;

        let migrations = with_connection(input.db_path, move |conn| {
            ensure_migrations_table(conn)?;

            // Build query based on filter
            let query = match filter {
                MigrationStatusFilter::All => {
                    format!("SELECT version, name, sql, applied_at, checksum FROM {MIGRATIONS_TABLE} ORDER BY version")
                }
                MigrationStatusFilter::Pending => {
                    format!("SELECT version, name, sql, applied_at, checksum FROM {MIGRATIONS_TABLE} WHERE applied_at IS NULL ORDER BY version")
                }
                MigrationStatusFilter::Applied => {
                    format!("SELECT version, name, sql, applied_at, checksum FROM {MIGRATIONS_TABLE} WHERE applied_at IS NOT NULL ORDER BY version")
                }
            };

            let mut stmt = conn.prepare(&query)?;
            let migrations: Vec<Migration> = stmt
                .query_map([], |row| {
                    Ok(Migration {
                        version: row.get(0)?,
                        name: row.get(1)?,
                        sql: row.get(2)?,
                        applied_at: row.get(3)?,
                        checksum: row.get(4)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(migrations)
        })
        .await?;

        let count = migrations.len();
        let output = match format {
            ExportFormat::Json => {
                serde_json::json!({
                    "status": "success",
                    "format": "json",
                    "count": count,
                    "migrations": migrations
                })
            }
            ExportFormat::Sql => {
                // Generate SQL script with migration metadata as comments
                let mut sql = String::new();
                sql.push_str("-- Exported migrations\n");
                sql.push_str("-- Import using sqlite_import_migrations tool\n\n");

                for m in &migrations {
                    sql.push_str(&format!("-- Migration: {} ({})\n", m.name, m.version));
                    sql.push_str(&format!("-- Checksum: {}\n", m.checksum));
                    if let Some(applied) = &m.applied_at {
                        sql.push_str(&format!("-- Applied: {}\n", applied));
                    } else {
                        sql.push_str("-- Status: pending\n");
                    }
                    sql.push_str(&m.sql);
                    if !m.sql.ends_with(';') {
                        sql.push(';');
                    }
                    sql.push_str("\n\n");
                }

                serde_json::json!({
                    "status": "success",
                    "format": "sql",
                    "count": count,
                    "script": sql
                })
            }
        };
        Ok(ToolResult::Json(output))
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
    async fn test_export_all_migrations() {
        let db = TestDatabase::new().await;

        // Add two migrations
        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "create users".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        add_tool
            .execute(AddMigrationInput {
                name: "create posts".to_string(),
                sql: "CREATE TABLE posts (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Apply first one
        RunMigrationsTool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Export all
        let tool = ExportMigrationsTool;
        let result = tool
            .execute(ExportMigrationsInput {
                db_path: Some(db.key()),
                filter: MigrationStatusFilter::All,
                format: ExportFormat::Json,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["count"], 2);
        assert_eq!(json["migrations"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_export_pending_only() {
        let db = TestDatabase::new().await;

        // Add two migrations
        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "create users".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Apply it
        RunMigrationsTool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Add another (pending)
        add_tool
            .execute(AddMigrationInput {
                name: "create posts".to_string(),
                sql: "CREATE TABLE posts (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Export pending only
        let tool = ExportMigrationsTool;
        let result = tool
            .execute(ExportMigrationsInput {
                db_path: Some(db.key()),
                filter: MigrationStatusFilter::Pending,
                format: ExportFormat::Json,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["count"], 1);
        let migrations = json["migrations"].as_array().unwrap();
        assert_eq!(migrations[0]["name"], "create posts");
    }

    #[tokio::test]
    async fn test_export_sql_format() {
        let db = TestDatabase::new().await;

        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "create users".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY)".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let tool = ExportMigrationsTool;
        let result = tool
            .execute(ExportMigrationsInput {
                db_path: Some(db.key()),
                filter: MigrationStatusFilter::All,
                format: ExportFormat::Sql,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["format"], "sql");
        let script = json["script"].as_str().unwrap();
        assert!(script.contains("CREATE TABLE users"));
        assert!(script.contains("Migration: create users"));
    }

    #[tokio::test]
    async fn test_export_empty() {
        let db = TestDatabase::new().await;

        let tool = ExportMigrationsTool;
        let result = tool
            .execute(ExportMigrationsInput {
                db_path: Some(db.key()),
                filter: MigrationStatusFilter::All,
                format: ExportFormat::Json,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["count"], 0);
    }
}
