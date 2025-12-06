//! Schema migration tools for SQLite databases
//!
//! This module provides tools for managing database schema migrations that are
//! stored within the database itself, enabling agents to evolve schemas over time.
//!
//! # Design
//!
//! Migrations are stored in a `_schema_migrations` table within each database:
//! - `version`: Timestamp-based unique identifier (auto-generated)
//! - `name`: Human-readable description
//! - `sql`: The DDL to execute
//! - `applied_at`: When the migration was applied (NULL = pending)
//! - `checksum`: SHA256 of the SQL for integrity
//!
//! # Tools
//!
//! - [`AddMigrationTool`] - Store a new pending migration
//! - [`RunMigrationsTool`] - Apply all pending migrations in order
//! - [`ListMigrationsTool`] - List migrations with optional status filter
//! - [`GetMigrationTool`] - Get details of a specific migration
//! - [`RemoveMigrationTool`] - Remove a pending migration before it's applied

mod add;
pub mod export;
mod get;
pub mod import;
mod list;
mod remove;
mod run;
pub mod types;

pub use add::AddMigrationTool;
pub use export::ExportMigrationsTool;
pub use get::GetMigrationTool;
pub use import::ImportMigrationsTool;
pub use list::ListMigrationsTool;
pub use remove::RemoveMigrationTool;
pub use run::RunMigrationsTool;
pub use types::{Migration, MigrationStatusFilter};

use crate::sqlite::error::SqliteToolError;
use chrono::Utc;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

/// The name of the migrations table
pub const MIGRATIONS_TABLE: &str = "_schema_migrations";

/// Ensures the migrations table exists in the database
pub fn ensure_migrations_table(conn: &Connection) -> Result<(), SqliteToolError> {
    conn.execute_batch(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS {MIGRATIONS_TABLE} (
            version TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            sql TEXT NOT NULL,
            applied_at TEXT,
            checksum TEXT NOT NULL
        );
        "#
    ))?;
    Ok(())
}

/// Generates a version string based on current timestamp
///
/// Format: YYYYMMDD_HHMMSS_microseconds (e.g., "20240115_143052_123456")
pub fn generate_version() -> String {
    Utc::now().format("%Y%m%d_%H%M%S_%6f").to_string()
}

/// Computes SHA256 checksum of SQL content
pub fn compute_checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_version_format() {
        let version = generate_version();
        // Should be format: YYYYMMDD_HHMMSS_UUUUUU
        assert_eq!(version.len(), 22);
        assert_eq!(&version[8..9], "_");
        assert_eq!(&version[15..16], "_");
    }

    #[test]
    fn test_generate_version_uniqueness() {
        let v1 = generate_version();
        std::thread::sleep(std::time::Duration::from_micros(10));
        let v2 = generate_version();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_compute_checksum() {
        let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY);";
        let checksum = compute_checksum(sql);
        // SHA256 produces 64 hex characters
        assert_eq!(checksum.len(), 64);

        // Same input produces same checksum
        assert_eq!(checksum, compute_checksum(sql));

        // Different input produces different checksum
        let other = compute_checksum("CREATE TABLE posts (id INTEGER PRIMARY KEY);");
        assert_ne!(checksum, other);
    }

    #[test]
    fn test_ensure_migrations_table() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_migrations_table(&conn).unwrap();

        // Table should exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
                [MIGRATIONS_TABLE],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Calling again should be idempotent
        ensure_migrations_table(&conn).unwrap();
    }
}
