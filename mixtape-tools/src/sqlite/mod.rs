//! SQLite database management tools
//!
//! This module provides a comprehensive set of tools for managing SQLite databases
//! through an AI agent. Tools are separated into read-only (safe) and write/modify
//! (destructive) categories for granular permission control.
//!
//! # Quick Start
//!
//! Use the helper functions to add tool groups to your agent:
//!
//! ```rust,ignore
//! use mixtape_core::Agent;
//! use mixtape_tools::sqlite;
//!
//! // Read-only agent - can explore but not modify databases
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4)
//!     .add_tools(sqlite::read_only_tools())
//!     .build()
//!     .await?;
//!
//! // Full access agent - can read, write, and manage schemas
//! let agent = Agent::builder()
//!     .bedrock(ClaudeSonnet4)
//!     .add_tools(sqlite::all_tools())
//!     .build()
//!     .await?;
//! ```
//!
//! # Tool Groups
//!
//! | Function | Tools | Use Case |
//! |----------|-------|----------|
//! | [`read_only_tools()`] | 9 tools | Database exploration, querying, backups |
//! | [`mutative_tools()`] | 4 tools | Data modifications |
//! | [`transaction_tools()`] | 3 tools | Transaction management |
//! | [`migration_tools()`] | 7 tools | Schema evolution via stored migrations |
//! | [`all_tools()`] | 23 tools | Full database management |
//!
//! # Common Patterns
//!
//! ## Read-Only Database Explorer
//!
//! For agents that should only query and explore databases without modifying them:
//!
//! ```rust,ignore
//! use mixtape_tools::sqlite;
//!
//! let agent = Agent::builder()
//!     .add_tools(sqlite::read_only_tools())
//!     .build()
//!     .await?;
//! ```
//!
//! This includes: open/close/list databases, list/describe tables, SELECT queries,
//! schema export, and backups.
//!
//! ## Data Entry Agent
//!
//! For agents that need to insert/update data but not modify schema:
//!
//! ```rust,ignore
//! use mixtape_tools::sqlite::{self, *};
//!
//! let agent = Agent::builder()
//!     .add_tools(sqlite::read_only_tools())
//!     .add_tool(WriteQueryTool)      // INSERT/UPDATE/DELETE
//!     .add_tool(BulkInsertTool)      // Batch inserts
//!     .add_tools(sqlite::transaction_tools())
//!     .build()
//!     .await?;
//! ```
//!
//! ## Schema Migration Agent
//!
//! For agents that manage database schemas via migrations:
//!
//! ```rust,ignore
//! use mixtape_tools::sqlite;
//!
//! let agent = Agent::builder()
//!     .add_tools(sqlite::read_only_tools())
//!     .add_tools(sqlite::migration_tools())
//!     .build()
//!     .await?;
//! ```
//!
//! # Fine-Grained Permissions
//!
//! For tighter control, use configured tools that restrict access to specific
//! databases and tables. These tools validate SQL queries before execution and
//! enforce table-level read/write permissions.
//!
//! ## Lock Tools to a Specific Database
//!
//! ```rust,ignore
//! use mixtape_tools::sqlite;
//!
//! // Tools will only access this database, ignoring any db_path in input
//! let agent = Agent::builder()
//!     .add_tools(sqlite::tools_for_database("/data/app.db"))
//!     .build()
//!     .await?;
//! ```
//!
//! ## Read-Only Access to Specific Tables
//!
//! ```rust,ignore
//! use mixtape_tools::sqlite;
//!
//! // Can only SELECT from these tables, all writes blocked
//! let agent = Agent::builder()
//!     .add_tools(sqlite::read_only_tools_for_tables(
//!         "/data/app.db",
//!         ["users", "products", "orders"]
//!     ))
//!     .build()
//!     .await?;
//! ```
//!
//! ## Custom Table Permissions
//!
//! ```rust,ignore
//! use mixtape_tools::sqlite::{SqliteConfig, tools_with_config};
//!
//! let config = SqliteConfig::builder()
//!     .db_path("/data/app.db")
//!     .allow_read(["users", "products", "orders", "analytics"])
//!     .allow_write(["analytics"])  // Can only write to analytics
//!     .deny_read(["secrets"])      // Block access to secrets table
//!     .build()?;
//!
//! let agent = Agent::builder()
//!     .add_tools(tools_with_config(config))
//!     .build()
//!     .await?;
//! ```
//!
//! # Tool Categories
//!
//! ## Database Management (Safe)
//! - `sqlite_open_database` - Open or create a database
//! - `sqlite_close_database` - Close a database connection
//! - `sqlite_list_databases` - Discover database files in a directory
//! - `sqlite_database_info` - Get database metadata and statistics
//!
//! ## Table Operations
//! - `sqlite_list_tables` - List all tables and views (Safe)
//! - `sqlite_describe_table` - Get table schema details (Safe)
//!
//! ## Query Operations
//! - `sqlite_read_query` - Execute SELECT/PRAGMA/EXPLAIN queries (Safe)
//! - `sqlite_write_query` - Execute INSERT/UPDATE/DELETE queries (Destructive)
//! - `sqlite_schema_query` - Execute DDL statements (Destructive)
//! - `sqlite_bulk_insert` - Batch insert records (Destructive)
//!
//! ## Transaction Management (Configurable)
//! - `sqlite_begin_transaction` - Start a transaction
//! - `sqlite_commit_transaction` - Commit a transaction
//! - `sqlite_rollback_transaction` - Rollback a transaction
//!
//! ## Maintenance Operations
//! - `sqlite_export_schema` - Export schema as SQL or JSON (Safe)
//! - `sqlite_backup` - Create a database backup (Safe)
//! - `sqlite_vacuum` - Optimize database storage (Destructive)
//!
//! ## Migration Operations
//! - `sqlite_add_migration` - Store a new pending migration (Destructive)
//! - `sqlite_run_migrations` - Apply pending migrations in order (Destructive)
//! - `sqlite_list_migrations` - List migrations with status filter (Safe)
//! - `sqlite_get_migration` - Get migration details by version (Safe)
//! - `sqlite_remove_migration` - Remove a pending migration (Destructive)
//! - `sqlite_export_migrations` - Export migrations for transfer (Safe)
//! - `sqlite_import_migrations` - Import migrations as pending (Destructive)

pub mod config;
pub mod configured;
pub mod database;
pub mod error;
pub mod maintenance;
pub mod manager;
pub mod migration;
pub mod query;
mod sql_parser;
pub mod table;
#[cfg(test)]
pub mod test_utils;
pub mod transaction;
pub mod types;

// Re-export commonly used items
pub use config::{
    ConfigError, SqliteConfig, SqliteConfigBuilder, TablePermissionMode, TablePermissions,
};
pub use configured::{
    ConfiguredBulkInsertTool, ConfiguredReadQueryTool, ConfiguredSchemaQueryTool,
    ConfiguredWriteQueryTool,
};
pub use database::{CloseDatabaseTool, DatabaseInfoTool, ListDatabasesTool, OpenDatabaseTool};
pub use error::SqliteToolError;
pub use maintenance::{BackupDatabaseTool, ExportSchemaTool, VacuumDatabaseTool};
pub use manager::{with_connection, DATABASE_MANAGER};
pub use migration::{
    AddMigrationTool, ExportMigrationsTool, GetMigrationTool, ImportMigrationsTool,
    ListMigrationsTool, RemoveMigrationTool, RunMigrationsTool,
};
pub use query::{BulkInsertTool, ReadQueryTool, SchemaQueryTool, WriteQueryTool};
pub use table::{DescribeTableTool, ListTablesTool};
pub use transaction::{BeginTransactionTool, CommitTransactionTool, RollbackTransactionTool};
pub use types::*;

use mixtape_core::tool::{box_tool, DynTool};

/// Returns all read-only SQLite tools
///
/// These tools cannot modify data or schema - only query and export.
pub fn read_only_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        box_tool(OpenDatabaseTool),
        box_tool(CloseDatabaseTool),
        box_tool(ListDatabasesTool),
        box_tool(DatabaseInfoTool),
        box_tool(ListTablesTool),
        box_tool(DescribeTableTool),
        box_tool(ReadQueryTool),
        box_tool(ExportSchemaTool),
        box_tool(BackupDatabaseTool),
    ]
}

/// Returns all mutative (write/modify) SQLite tools
pub fn mutative_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        box_tool(WriteQueryTool),
        box_tool(SchemaQueryTool),
        box_tool(BulkInsertTool),
        box_tool(VacuumDatabaseTool),
    ]
}

/// Returns all transaction management SQLite tools
pub fn transaction_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        box_tool(BeginTransactionTool),
        box_tool(CommitTransactionTool),
        box_tool(RollbackTransactionTool),
    ]
}

/// Returns all migration management SQLite tools
///
/// These tools allow agents to evolve database schemas over time by storing
/// and executing migrations within the database itself.
pub fn migration_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        box_tool(AddMigrationTool),
        box_tool(RunMigrationsTool),
        box_tool(ListMigrationsTool),
        box_tool(GetMigrationTool),
        box_tool(RemoveMigrationTool),
        box_tool(ExportMigrationsTool),
        box_tool(ImportMigrationsTool),
    ]
}

/// Returns all SQLite tools
pub fn all_tools() -> Vec<Box<dyn DynTool>> {
    let mut tools = read_only_tools();
    tools.extend(mutative_tools());
    tools.extend(transaction_tools());
    tools.extend(migration_tools());
    tools
}

// =============================================================================
// Configured tool factory functions
// =============================================================================

use std::sync::Arc;

/// Create a set of SQLite tools restricted to a specific database.
///
/// The returned tools will only access the specified database, ignoring
/// any `db_path` parameter in tool inputs.
///
/// # Example
///
/// ```rust,ignore
/// use mixtape_tools::sqlite;
///
/// let agent = Agent::builder()
///     .add_tools(sqlite::tools_for_database("/data/app.db"))
///     .build()
///     .await?;
/// ```
pub fn tools_for_database(db_path: impl Into<String>) -> Vec<Box<dyn DynTool>> {
    let config = Arc::new(
        SqliteConfig::builder()
            .db_path(db_path)
            .build()
            .expect("tools_for_database: config build should never fail"),
    );

    vec![
        box_tool(ConfiguredReadQueryTool::with_shared_config(config.clone())),
        box_tool(ConfiguredWriteQueryTool::with_shared_config(config.clone())),
        box_tool(ConfiguredSchemaQueryTool::with_shared_config(
            config.clone(),
        )),
        box_tool(ConfiguredBulkInsertTool::with_shared_config(config)),
    ]
}

/// Create read-only tools for specific tables in a specific database.
///
/// The returned tools can only read from the specified tables.
/// All write operations will be denied.
///
/// # Example
///
/// ```rust,ignore
/// use mixtape_tools::sqlite;
///
/// let agent = Agent::builder()
///     .add_tools(sqlite::read_only_tools_for_tables(
///         "/data/app.db",
///         ["users", "products", "orders"]
///     ))
///     .build()
///     .await?;
/// ```
pub fn read_only_tools_for_tables<I, S>(
    db_path: impl Into<String>,
    tables: I,
) -> Vec<Box<dyn DynTool>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let config = Arc::new(
        SqliteConfig::builder()
            .db_path(db_path)
            .allow_read(tables)
            .read_only()
            .build()
            .expect("read_only_tools_for_tables: config build should never fail"),
    );

    vec![box_tool(ConfiguredReadQueryTool::with_shared_config(
        config,
    ))]
}

/// Create tools with custom configuration.
///
/// This provides full control over database path and table permissions.
///
/// # Example
///
/// ```rust,ignore
/// use mixtape_tools::sqlite::{SqliteConfig, tools_with_config};
///
/// let config = SqliteConfig::builder()
///     .db_path("/data/app.db")
///     .allow_read(["users", "products", "orders", "analytics"])
///     .allow_write(["analytics"])
///     .build();
///
/// let agent = Agent::builder()
///     .add_tools(tools_with_config(config))
///     .build()
///     .await?;
/// ```
pub fn tools_with_config(config: SqliteConfig) -> Vec<Box<dyn DynTool>> {
    let config = Arc::new(config);

    vec![
        box_tool(ConfiguredReadQueryTool::with_shared_config(config.clone())),
        box_tool(ConfiguredWriteQueryTool::with_shared_config(config.clone())),
        box_tool(ConfiguredSchemaQueryTool::with_shared_config(
            config.clone(),
        )),
        box_tool(ConfiguredBulkInsertTool::with_shared_config(config)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_tools_count_and_names() {
        let tools = read_only_tools();
        assert_eq!(tools.len(), 9);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"sqlite_open_database"));
        assert!(names.contains(&"sqlite_close_database"));
        assert!(names.contains(&"sqlite_list_databases"));
        assert!(names.contains(&"sqlite_database_info"));
        assert!(names.contains(&"sqlite_list_tables"));
        assert!(names.contains(&"sqlite_describe_table"));
        assert!(names.contains(&"sqlite_read_query"));
        assert!(names.contains(&"sqlite_export_schema"));
        assert!(names.contains(&"sqlite_backup"));
    }

    #[test]
    fn test_mutative_tools_count_and_names() {
        let tools = mutative_tools();
        assert_eq!(tools.len(), 4);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"sqlite_write_query"));
        assert!(names.contains(&"sqlite_schema_query"));
        assert!(names.contains(&"sqlite_bulk_insert"));
        assert!(names.contains(&"sqlite_vacuum"));
    }

    #[test]
    fn test_transaction_tools_count_and_names() {
        let tools = transaction_tools();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"sqlite_begin_transaction"));
        assert!(names.contains(&"sqlite_commit_transaction"));
        assert!(names.contains(&"sqlite_rollback_transaction"));
    }

    #[test]
    fn test_migration_tools_count_and_names() {
        let tools = migration_tools();
        assert_eq!(tools.len(), 7);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"sqlite_add_migration"));
        assert!(names.contains(&"sqlite_run_migrations"));
        assert!(names.contains(&"sqlite_list_migrations"));
        assert!(names.contains(&"sqlite_get_migration"));
        assert!(names.contains(&"sqlite_remove_migration"));
        assert!(names.contains(&"sqlite_export_migrations"));
        assert!(names.contains(&"sqlite_import_migrations"));
    }

    #[test]
    fn test_all_tools_combines_categories() {
        let all = all_tools();
        let read_only = read_only_tools();
        let mutative = mutative_tools();
        let transaction = transaction_tools();
        let migration = migration_tools();

        assert_eq!(
            all.len(),
            read_only.len() + mutative.len() + transaction.len() + migration.len()
        );
        assert_eq!(all.len(), 23);
    }

    #[test]
    fn test_tools_have_descriptions() {
        for tool in all_tools() {
            assert!(
                !tool.description().is_empty(),
                "Tool {} has empty description",
                tool.name()
            );
        }
    }

    #[test]
    fn test_tools_have_schemas() {
        for tool in all_tools() {
            let schema = tool.input_schema();
            assert!(
                schema.is_object(),
                "Tool {} schema is not an object",
                tool.name()
            );
        }
    }

    #[test]
    fn test_no_duplicate_tool_names() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        let mut unique_names = names.clone();
        unique_names.sort();
        unique_names.dedup();
        assert_eq!(
            names.len(),
            unique_names.len(),
            "Duplicate tool names found"
        );
    }

    #[test]
    fn test_tools_for_database() {
        let tools = tools_for_database("/test/path.db");
        assert_eq!(tools.len(), 4);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"sqlite_read_query"));
        assert!(names.contains(&"sqlite_write_query"));
        assert!(names.contains(&"sqlite_schema_query"));
        assert!(names.contains(&"sqlite_bulk_insert"));
    }

    #[test]
    fn test_read_only_tools_for_tables() {
        let tools = read_only_tools_for_tables("/test/path.db", ["users", "orders"]);
        assert_eq!(tools.len(), 1);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"sqlite_read_query"));
    }

    #[test]
    fn test_tools_with_config() {
        let config = SqliteConfig::builder()
            .db_path("/test/path.db")
            .allow_read(["users"])
            .allow_write(["orders"])
            .build()
            .unwrap();

        let tools = tools_with_config(config);
        assert_eq!(tools.len(), 4);
    }
}
