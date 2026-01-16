//! Configured tool wrappers with permission support
//!
//! This module provides wrapper tools that enforce database path and table
//! permission restrictions configured at tool creation time.

use crate::prelude::*;
use crate::sqlite::config::SqliteConfig;
use crate::sqlite::error::SqliteToolError;
use crate::sqlite::query::{
    BulkInsertInput, BulkInsertTool, ReadQueryInput, ReadQueryTool, SchemaQueryInput,
    SchemaQueryTool, WriteQueryInput, WriteQueryTool,
};
use crate::sqlite::sql_parser::extract_table_operations;
use std::sync::Arc;

/// Validate query permissions against the configuration
fn validate_query(config: &SqliteConfig, sql: &str) -> Result<(), SqliteToolError> {
    let ops =
        extract_table_operations(sql).map_err(|e| SqliteToolError::InvalidQuery(e.to_string()))?;

    // Check read permissions
    for table in &ops.read {
        if !config.can_read(table) {
            return Err(SqliteToolError::PermissionDenied {
                operation: "read".to_string(),
                table: table.clone(),
            });
        }
    }

    // Check write permissions
    for table in &ops.write {
        if !config.can_write(table) {
            return Err(SqliteToolError::PermissionDenied {
                operation: "write".to_string(),
                table: table.clone(),
            });
        }
    }

    Ok(())
}

/// A ReadQueryTool with permission configuration
pub struct ConfiguredReadQueryTool {
    config: Arc<SqliteConfig>,
    inner: ReadQueryTool,
}

impl ConfiguredReadQueryTool {
    /// Create a new configured read query tool
    pub fn new(config: SqliteConfig) -> Self {
        Self {
            config: Arc::new(config),
            inner: ReadQueryTool,
        }
    }

    /// Create with a shared config (useful when multiple tools share permissions)
    pub fn with_shared_config(config: Arc<SqliteConfig>) -> Self {
        Self {
            config,
            inner: ReadQueryTool,
        }
    }
}

impl Tool for ConfiguredReadQueryTool {
    type Input = ReadQueryInput;

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    async fn execute(&self, mut input: Self::Input) -> Result<ToolResult, ToolError> {
        input.db_path = self.config.effective_db_path(input.db_path);
        validate_query(&self.config, &input.query)?;
        self.inner.execute(input).await
    }
}

/// A WriteQueryTool with permission configuration
pub struct ConfiguredWriteQueryTool {
    config: Arc<SqliteConfig>,
    inner: WriteQueryTool,
}

impl ConfiguredWriteQueryTool {
    /// Create a new configured write query tool
    pub fn new(config: SqliteConfig) -> Self {
        Self {
            config: Arc::new(config),
            inner: WriteQueryTool,
        }
    }

    /// Create with a shared config
    pub fn with_shared_config(config: Arc<SqliteConfig>) -> Self {
        Self {
            config,
            inner: WriteQueryTool,
        }
    }
}

impl Tool for ConfiguredWriteQueryTool {
    type Input = WriteQueryInput;

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    async fn execute(&self, mut input: Self::Input) -> Result<ToolResult, ToolError> {
        input.db_path = self.config.effective_db_path(input.db_path);
        validate_query(&self.config, &input.query)?;
        self.inner.execute(input).await
    }
}

/// A SchemaQueryTool with permission configuration
pub struct ConfiguredSchemaQueryTool {
    config: Arc<SqliteConfig>,
    inner: SchemaQueryTool,
}

impl ConfiguredSchemaQueryTool {
    /// Create a new configured schema query tool
    pub fn new(config: SqliteConfig) -> Self {
        Self {
            config: Arc::new(config),
            inner: SchemaQueryTool,
        }
    }

    /// Create with a shared config
    pub fn with_shared_config(config: Arc<SqliteConfig>) -> Self {
        Self {
            config,
            inner: SchemaQueryTool,
        }
    }
}

impl Tool for ConfiguredSchemaQueryTool {
    type Input = SchemaQueryInput;

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    async fn execute(&self, mut input: Self::Input) -> Result<ToolResult, ToolError> {
        input.db_path = self.config.effective_db_path(input.db_path);
        validate_query(&self.config, &input.query)?;
        self.inner.execute(input).await
    }
}

/// A BulkInsertTool with permission configuration
pub struct ConfiguredBulkInsertTool {
    config: Arc<SqliteConfig>,
    inner: BulkInsertTool,
}

impl ConfiguredBulkInsertTool {
    /// Create a new configured bulk insert tool
    pub fn new(config: SqliteConfig) -> Self {
        Self {
            config: Arc::new(config),
            inner: BulkInsertTool,
        }
    }

    /// Create with a shared config
    pub fn with_shared_config(config: Arc<SqliteConfig>) -> Self {
        Self {
            config,
            inner: BulkInsertTool,
        }
    }
}

impl Tool for ConfiguredBulkInsertTool {
    type Input = BulkInsertInput;

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    async fn execute(&self, mut input: Self::Input) -> Result<ToolResult, ToolError> {
        input.db_path = self.config.effective_db_path(input.db_path);
        if !self.config.can_write(&input.table) {
            return Err(SqliteToolError::PermissionDenied {
                operation: "write".to_string(),
                table: input.table.clone(),
            }
            .into());
        }
        self.inner.execute(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_utils::TestDatabase;

    #[tokio::test]
    async fn test_configured_read_tool_allows_permitted_table() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             INSERT INTO users VALUES (1, 'Alice');",
        )
        .await;

        let config = SqliteConfig::builder()
            .db_path(db.key())
            .allow_read(["users"])
            .build()
            .unwrap();

        let tool = ConfiguredReadQueryTool::new(config);
        let result = tool
            .execute(ReadQueryInput::new("SELECT * FROM users"))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_configured_read_tool_denies_unpermitted_table() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE secrets (id INTEGER, data TEXT);
             INSERT INTO secrets VALUES (1, 'secret data');",
        )
        .await;

        let config = SqliteConfig::builder()
            .db_path(db.key())
            .allow_read(["users"]) // secrets not in allow list
            .build()
            .unwrap();

        let tool = ConfiguredReadQueryTool::new(config);
        let result = tool
            .execute(ReadQueryInput::new("SELECT * FROM secrets"))
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Permission denied"));
        assert!(err_msg.contains("secrets"));
    }

    #[tokio::test]
    async fn test_configured_write_tool_allows_permitted_table() {
        let db = TestDatabase::with_schema("CREATE TABLE orders (id INTEGER, amount REAL);").await;

        let config = SqliteConfig::builder()
            .db_path(db.key())
            .allow_write(["orders"])
            .build()
            .unwrap();

        let tool = ConfiguredWriteQueryTool::new(config);
        let result = tool
            .execute(WriteQueryInput {
                query: "INSERT INTO orders VALUES (1, 99.99)".to_string(),
                params: vec![],
                db_path: None,
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_configured_write_tool_denies_unpermitted_table() {
        let db = TestDatabase::with_schema("CREATE TABLE audit_log (id INTEGER, msg TEXT);").await;

        let config = SqliteConfig::builder()
            .db_path(db.key())
            .deny_write(["audit_log"])
            .build()
            .unwrap();

        let tool = ConfiguredWriteQueryTool::new(config);
        let result = tool
            .execute(WriteQueryInput {
                query: "INSERT INTO audit_log VALUES (1, 'hacked')".to_string(),
                params: vec![],
                db_path: None,
            })
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Permission denied"));
        assert!(err_msg.contains("audit_log"));
    }

    #[tokio::test]
    async fn test_configured_tool_uses_configured_db_path() {
        let db = TestDatabase::with_schema("CREATE TABLE test (id INTEGER);").await;

        let config = SqliteConfig::builder().db_path(db.key()).build().unwrap();

        let tool = ConfiguredReadQueryTool::new(config);

        // Input provides a different path, but configured path should be used
        let result = tool
            .execute(ReadQueryInput::new("SELECT * FROM test").db_path("/nonexistent/path.db"))
            .await;

        // Should succeed because it uses the configured path, not the input path
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_configured_bulk_insert_checks_permissions() {
        let db = TestDatabase::with_schema("CREATE TABLE users (id INTEGER, name TEXT);").await;

        let config = SqliteConfig::builder()
            .db_path(db.key())
            .deny_write(["users"])
            .build()
            .unwrap();

        let tool = ConfiguredBulkInsertTool::new(config);
        let mut record = serde_json::Map::new();
        record.insert("id".to_string(), serde_json::json!(1));
        record.insert("name".to_string(), serde_json::json!("Alice"));

        let result = tool
            .execute(BulkInsertInput {
                table: "users".to_string(),
                data: vec![record],
                batch_size: 1000,
                db_path: None,
            })
            .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Permission denied"));
    }

    #[tokio::test]
    async fn test_read_only_config_blocks_all_writes() {
        let db = TestDatabase::with_schema("CREATE TABLE data (id INTEGER);").await;

        let config = SqliteConfig::builder()
            .db_path(db.key())
            .read_only()
            .build()
            .unwrap();

        let tool = ConfiguredWriteQueryTool::new(config);
        let result = tool
            .execute(WriteQueryInput {
                query: "INSERT INTO data VALUES (1)".to_string(),
                params: vec![],
                db_path: None,
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shared_config_between_tools() {
        let db = TestDatabase::with_schema(
            "CREATE TABLE users (id INTEGER, name TEXT);
             CREATE TABLE orders (id INTEGER, user_id INTEGER);",
        )
        .await;

        let config = Arc::new(
            SqliteConfig::builder()
                .db_path(db.key())
                .allow_read(["users", "orders"])
                .allow_write(["orders"])
                .build()
                .unwrap(),
        );

        let read_tool = ConfiguredReadQueryTool::with_shared_config(config.clone());
        let write_tool = ConfiguredWriteQueryTool::with_shared_config(config.clone());

        // Read from users should work
        assert!(read_tool
            .execute(ReadQueryInput::new("SELECT * FROM users"))
            .await
            .is_ok());

        // Write to users should fail
        assert!(write_tool
            .execute(WriteQueryInput {
                query: "INSERT INTO users VALUES (1, 'test')".to_string(),
                params: vec![],
                db_path: None,
            })
            .await
            .is_err());

        // Write to orders should work
        assert!(write_tool
            .execute(WriteQueryInput {
                query: "INSERT INTO orders VALUES (1, 1)".to_string(),
                params: vec![],
                db_path: None,
            })
            .await
            .is_ok());
    }
}
