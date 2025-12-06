//! Import migrations tool

use crate::prelude::*;
use crate::sqlite::manager::with_connection;

use super::{compute_checksum, ensure_migrations_table, MIGRATIONS_TABLE};

/// A migration record to import
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MigrationRecord {
    /// Version identifier (timestamp format recommended)
    pub version: String,

    /// Human-readable name/description
    pub name: String,

    /// The SQL to execute
    pub sql: String,

    /// Optional checksum for verification (computed if not provided)
    #[serde(default)]
    pub checksum: Option<String>,
}

/// Input for importing migrations
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportMigrationsInput {
    /// Database to import migrations into (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,

    /// Migrations to import
    pub migrations: Vec<MigrationRecord>,

    /// How to handle migrations that already exist
    #[serde(default)]
    pub on_conflict: ConflictStrategy,
}

/// Strategy for handling migrations that already exist
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStrategy {
    /// Skip migrations that already exist (by version)
    #[default]
    Skip,
    /// Fail if any migration already exists
    Fail,
    /// Replace existing migrations (only if pending, fails for applied)
    Replace,
}

/// Result for a single migration import
#[derive(Debug, Serialize)]
struct ImportResult {
    version: String,
    name: String,
    status: &'static str,
    message: Option<String>,
}

/// Imports migrations from an export into the database as pending migrations
///
/// Use this to transfer migrations from one database to another. Imported
/// migrations are added as pending and must be applied using `sqlite_run_migrations`.
pub struct ImportMigrationsTool;

impl Tool for ImportMigrationsTool {
    type Input = ImportMigrationsInput;

    fn name(&self) -> &str {
        "sqlite_import_migrations"
    }

    fn description(&self) -> &str {
        "Import migrations into the database as pending migrations. \
         Use this to transfer migrations exported from another database via \
         sqlite_export_migrations. Imported migrations must be applied using \
         sqlite_run_migrations."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let migrations = input.migrations;
        let on_conflict = input.on_conflict;

        let (results, imported, skipped, failed) = with_connection(input.db_path, move |conn| {
            ensure_migrations_table(conn)?;

            let mut results = Vec::new();
            let mut imported = 0;
            let mut skipped = 0;
            let mut failed = 0;

            for migration in migrations {
                // Check if migration already exists
                let existing: Option<(String, Option<String>)> = conn
                    .query_row(
                        &format!(
                            "SELECT name, applied_at FROM {MIGRATIONS_TABLE} WHERE version = ?1"
                        ),
                        [&migration.version],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .ok();

                if let Some((existing_name, applied_at)) = existing {
                    match on_conflict {
                        ConflictStrategy::Skip => {
                            results.push(ImportResult {
                                version: migration.version,
                                name: migration.name,
                                status: "skipped",
                                message: Some(format!(
                                    "Migration already exists as '{}'",
                                    existing_name
                                )),
                            });
                            skipped += 1;
                            continue;
                        }
                        ConflictStrategy::Fail => {
                            return Err(crate::sqlite::error::SqliteToolError::InvalidQuery(format!(
                                "Migration '{}' already exists. Use on_conflict: 'skip' or 'replace' to handle duplicates.",
                                migration.version
                            )));
                        }
                        ConflictStrategy::Replace => {
                            if applied_at.is_some() {
                                results.push(ImportResult {
                                    version: migration.version,
                                    name: migration.name,
                                    status: "failed",
                                    message: Some(
                                        "Cannot replace applied migration".to_string(),
                                    ),
                                });
                                failed += 1;
                                continue;
                            }
                            // Delete existing pending migration
                            conn.execute(
                                &format!("DELETE FROM {MIGRATIONS_TABLE} WHERE version = ?1"),
                                [&migration.version],
                            )?;
                        }
                    }
                }

                // Compute or verify checksum
                let computed_checksum = compute_checksum(&migration.sql);
                if let Some(provided) = &migration.checksum {
                    if provided != &computed_checksum {
                        results.push(ImportResult {
                            version: migration.version,
                            name: migration.name,
                            status: "failed",
                            message: Some(format!(
                                "Checksum mismatch: expected {}, got {}",
                                provided, computed_checksum
                            )),
                        });
                        failed += 1;
                        continue;
                    }
                }

                // Insert as pending migration
                conn.execute(
                    &format!(
                        "INSERT INTO {MIGRATIONS_TABLE} (version, name, sql, applied_at, checksum) \
                         VALUES (?1, ?2, ?3, NULL, ?4)"
                    ),
                    (
                        &migration.version,
                        &migration.name,
                        &migration.sql,
                        &computed_checksum,
                    ),
                )?;

                results.push(ImportResult {
                    version: migration.version,
                    name: migration.name,
                    status: "imported",
                    message: None,
                });
                imported += 1;
            }

            Ok((results, imported, skipped, failed))
        })
        .await?;

        Ok(ToolResult::Json(serde_json::json!({
            "status": if failed == 0 { "success" } else { "partial" },
            "imported": imported,
            "skipped": skipped,
            "failed": failed,
            "results": results
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::migration::add::AddMigrationInput;
    use crate::sqlite::migration::export::{
        ExportFormat, ExportMigrationsInput, ExportMigrationsTool,
    };
    use crate::sqlite::migration::list::{ListMigrationsInput, ListMigrationsTool};
    use crate::sqlite::migration::run::RunMigrationsInput;
    use crate::sqlite::migration::MigrationStatusFilter;
    use crate::sqlite::migration::{AddMigrationTool, RunMigrationsTool};
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_import_migrations() {
        let db = TestDatabase::new().await;

        let tool = ImportMigrationsTool;
        let result = tool
            .execute(ImportMigrationsInput {
                db_path: Some(db.key()),
                migrations: vec![
                    MigrationRecord {
                        version: "20240101_120000_000000".to_string(),
                        name: "create users".to_string(),
                        sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                        checksum: None,
                    },
                    MigrationRecord {
                        version: "20240101_120001_000000".to_string(),
                        name: "create posts".to_string(),
                        sql: "CREATE TABLE posts (id INTEGER PRIMARY KEY);".to_string(),
                        checksum: None,
                    },
                ],
                on_conflict: ConflictStrategy::Skip,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["imported"], 2);
        assert_eq!(json["skipped"], 0);
    }

    #[tokio::test]
    async fn test_import_skip_existing() {
        let db = TestDatabase::new().await;

        // Add a migration directly
        AddMigrationTool
            .execute(AddMigrationInput {
                name: "existing".to_string(),
                sql: "CREATE TABLE existing (id INTEGER);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Get the version
        let list_result = ListMigrationsTool
            .execute(ListMigrationsInput {
                db_path: Some(db.key()),
                filter: MigrationStatusFilter::All,
            })
            .await
            .unwrap();

        let list_json = unwrap_json(list_result);
        let version = list_json["migrations"][0]["version"]
            .as_str()
            .unwrap()
            .to_string();

        // Try to import with same version
        let tool = ImportMigrationsTool;
        let result = tool
            .execute(ImportMigrationsInput {
                db_path: Some(db.key()),
                migrations: vec![MigrationRecord {
                    version,
                    name: "different name".to_string(),
                    sql: "CREATE TABLE different (id INTEGER);".to_string(),
                    checksum: None,
                }],
                on_conflict: ConflictStrategy::Skip,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["imported"], 0);
        assert_eq!(json["skipped"], 1);
    }

    #[tokio::test]
    async fn test_import_fail_on_conflict() {
        let db = TestDatabase::new().await;

        // Add a migration
        AddMigrationTool
            .execute(AddMigrationInput {
                name: "existing".to_string(),
                sql: "CREATE TABLE existing (id INTEGER);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let list_result = ListMigrationsTool
            .execute(ListMigrationsInput {
                db_path: Some(db.key()),
                filter: MigrationStatusFilter::All,
            })
            .await
            .unwrap();

        let list_json = unwrap_json(list_result);
        let version = list_json["migrations"][0]["version"]
            .as_str()
            .unwrap()
            .to_string();

        // Try to import with fail strategy
        let tool = ImportMigrationsTool;
        let result = tool
            .execute(ImportMigrationsInput {
                db_path: Some(db.key()),
                migrations: vec![MigrationRecord {
                    version,
                    name: "different".to_string(),
                    sql: "CREATE TABLE different (id INTEGER);".to_string(),
                    checksum: None,
                }],
                on_conflict: ConflictStrategy::Fail,
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_import_checksum_verification() {
        let db = TestDatabase::new().await;

        let tool = ImportMigrationsTool;
        let result = tool
            .execute(ImportMigrationsInput {
                db_path: Some(db.key()),
                migrations: vec![MigrationRecord {
                    version: "20240101_120000_000000".to_string(),
                    name: "test".to_string(),
                    sql: "CREATE TABLE test (id INTEGER);".to_string(),
                    checksum: Some("invalid_checksum".to_string()),
                }],
                on_conflict: ConflictStrategy::Skip,
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "partial");
        assert_eq!(json["imported"], 0);
        assert_eq!(json["failed"], 1);
    }

    #[tokio::test]
    async fn test_roundtrip_export_import() {
        let db1 = TestDatabase::new().await;
        let db2 = TestDatabase::new().await;

        // Add migrations to db1
        AddMigrationTool
            .execute(AddMigrationInput {
                name: "create users".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db1.key()),
            })
            .await
            .unwrap();

        AddMigrationTool
            .execute(AddMigrationInput {
                name: "create posts".to_string(),
                sql: "CREATE TABLE posts (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db1.key()),
            })
            .await
            .unwrap();

        // Export from db1
        let export_result = ExportMigrationsTool
            .execute(ExportMigrationsInput {
                db_path: Some(db1.key()),
                filter: MigrationStatusFilter::All,
                format: ExportFormat::Json,
            })
            .await
            .unwrap();

        let export_json = unwrap_json(export_result);
        let exported = export_json["migrations"].as_array().unwrap();

        // Convert to import format
        let migrations: Vec<MigrationRecord> = exported
            .iter()
            .map(|m| MigrationRecord {
                version: m["version"].as_str().unwrap().to_string(),
                name: m["name"].as_str().unwrap().to_string(),
                sql: m["sql"].as_str().unwrap().to_string(),
                checksum: Some(m["checksum"].as_str().unwrap().to_string()),
            })
            .collect();

        // Import to db2
        let import_result = ImportMigrationsTool
            .execute(ImportMigrationsInput {
                db_path: Some(db2.key()),
                migrations,
                on_conflict: ConflictStrategy::Skip,
            })
            .await
            .unwrap();

        let json = unwrap_json(import_result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["imported"], 2);

        // Run migrations on db2
        RunMigrationsTool
            .execute(RunMigrationsInput {
                db_path: Some(db2.key()),
            })
            .await
            .unwrap();

        // Verify tables exist in db2
        let rows = db2.query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('users', 'posts')",
        );
        assert_eq!(rows[0][0], 2);
    }
}
