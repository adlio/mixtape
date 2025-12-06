//! Run migrations tool

use crate::prelude::*;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::manager::DATABASE_MANAGER;
use chrono::Utc;

use super::{compute_checksum, ensure_migrations_table, MIGRATIONS_TABLE};

/// Input for running pending migrations
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunMigrationsInput {
    /// Database to run migrations on (uses default if not specified)
    #[serde(default)]
    pub db_path: Option<String>,
}

/// Runs all pending migrations in version order
///
/// Each migration is executed within a transaction. If a migration fails,
/// it is rolled back and subsequent migrations are not attempted.
pub struct RunMigrationsTool;

impl Tool for RunMigrationsTool {
    type Input = RunMigrationsInput;

    fn name(&self) -> &str {
        "sqlite_run_migrations"
    }

    fn description(&self) -> &str {
        "Apply all pending schema migrations in version order. Each migration runs in a \
         transaction. If a migration fails, it is rolled back and no further migrations run."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let result = tokio::task::spawn_blocking(move || -> Result<_, SqliteToolError> {
            let conn = DATABASE_MANAGER.get(input.db_path.as_deref())?;
            let mut conn = conn.lock().unwrap();

            // Ensure migrations table exists
            ensure_migrations_table(&conn)?;

            // Get pending migrations ordered by version
            let mut stmt = conn.prepare(&format!(
                "SELECT version, name, sql, checksum FROM {MIGRATIONS_TABLE} \
                 WHERE applied_at IS NULL ORDER BY version ASC"
            ))?;

            let pending: Vec<(String, String, String, String)> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            drop(stmt);

            if pending.is_empty() {
                return Ok((0, Vec::new()));
            }

            let mut applied = Vec::new();

            for (version, name, sql, stored_checksum) in pending {
                // Verify checksum
                let actual_checksum = compute_checksum(&sql);
                if actual_checksum != stored_checksum {
                    return Err(SqliteToolError::MigrationChecksumMismatch {
                        version,
                        expected: stored_checksum,
                        actual: actual_checksum,
                    });
                }

                // Execute migration in a transaction
                let tx = conn.transaction()?;

                // Execute the migration SQL
                tx.execute_batch(&sql)?;

                // Mark as applied
                let applied_at = Utc::now().to_rfc3339();
                tx.execute(
                    &format!("UPDATE {MIGRATIONS_TABLE} SET applied_at = ?1 WHERE version = ?2"),
                    rusqlite::params![applied_at, version],
                )?;

                tx.commit()?;

                applied.push(serde_json::json!({
                    "version": version,
                    "name": name,
                    "applied_at": applied_at
                }));
            }

            Ok((applied.len(), applied))
        })
        .await
        .map_err(|e| ToolError::Custom(format!("Task join error: {e}")))?;

        match result {
            Ok((count, applied)) => Ok(ToolResult::Json(serde_json::json!({
                "status": "success",
                "migrations_applied": count,
                "applied": applied,
                "message": if count == 0 {
                    "No pending migrations to apply".to_string()
                } else {
                    format!("{} migration(s) applied successfully", count)
                }
            }))),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::migration::add::AddMigrationInput;
    use crate::sqlite::migration::AddMigrationTool;
    use crate::sqlite::test_utils::{unwrap_json, TestDatabase};

    #[tokio::test]
    async fn test_run_migrations_empty() {
        let db = TestDatabase::new().await;

        let tool = RunMigrationsTool;
        let input = RunMigrationsInput {
            db_path: Some(db.key()),
        };

        let result = tool.execute(input).await.unwrap();
        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["migrations_applied"], 0);
    }

    #[tokio::test]
    async fn test_run_single_migration() {
        let db = TestDatabase::new().await;

        // Add a migration
        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "create users table".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Run migrations
        let run_tool = RunMigrationsTool;
        let result = run_tool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["migrations_applied"], 1);

        // Verify table was created
        let rows =
            db.query("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='users'");
        assert_eq!(rows[0][0], 1);
    }

    #[tokio::test]
    async fn test_run_migrations_idempotent() {
        let db = TestDatabase::new().await;

        // Add a migration
        let add_tool = AddMigrationTool;
        add_tool
            .execute(AddMigrationInput {
                name: "create users table".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Run migrations twice
        let run_tool = RunMigrationsTool;

        let result1 = run_tool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();
        let json1 = unwrap_json(result1);
        assert_eq!(json1["migrations_applied"], 1);

        // Second run should apply 0 migrations
        let result2 = run_tool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();
        let json2 = unwrap_json(result2);
        assert_eq!(json2["migrations_applied"], 0);
    }

    #[tokio::test]
    async fn test_run_multiple_migrations_in_order() {
        let db = TestDatabase::new().await;
        let add_tool = AddMigrationTool;

        // Add first migration
        add_tool
            .execute(AddMigrationInput {
                name: "create users table".to_string(),
                sql: "CREATE TABLE users (id INTEGER PRIMARY KEY);".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Add second migration (depends on first)
        add_tool
            .execute(AddMigrationInput {
                name: "add email to users".to_string(),
                sql: "ALTER TABLE users ADD COLUMN email TEXT;".to_string(),
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        // Run all migrations
        let run_tool = RunMigrationsTool;
        let result = run_tool
            .execute(RunMigrationsInput {
                db_path: Some(db.key()),
            })
            .await
            .unwrap();

        let json = unwrap_json(result);

        assert_eq!(json["status"], "success");
        assert_eq!(json["migrations_applied"], 2);

        // Verify both changes applied - check email column exists
        let rows = db.query("SELECT COUNT(*) FROM pragma_table_info('users') WHERE name='email'");
        assert_eq!(rows[0][0], 1);
    }
}
