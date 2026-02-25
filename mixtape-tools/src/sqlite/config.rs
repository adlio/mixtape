//! Configuration types for SQLite tool permissions
//!
//! This module provides configuration for restricting SQLite tool access
//! to specific databases and tables.

use std::collections::HashSet;
use thiserror::Error;

/// Errors that can occur when building a [`SqliteConfig`].
#[derive(Debug, Error)]
pub enum ConfigError {
    /// An empty deny list was specified, which allows all tables.
    /// Use [`TablePermissionMode::AllowAll`] instead.
    #[error(
        "empty deny list for {operation} permissions allows all tables - use AllowAll or omit"
    )]
    EmptyDenyList {
        /// Whether this was for "read" or "write" permissions
        operation: &'static str,
    },
}

/// Table permission mode - either allow specific tables or deny specific tables
#[derive(Debug, Clone, Default)]
pub enum TablePermissionMode {
    /// Allow all tables (default behavior)
    #[default]
    AllowAll,
    /// Only allow access to specified tables
    AllowList(HashSet<String>),
    /// Deny access to specified tables, allow all others
    DenyList(HashSet<String>),
}

impl TablePermissionMode {
    /// Check if access to a table is allowed
    pub fn is_allowed(&self, table: &str) -> bool {
        match self {
            TablePermissionMode::AllowAll => true,
            TablePermissionMode::AllowList(allowed) => allowed.contains(table),
            TablePermissionMode::DenyList(denied) => !denied.contains(table),
        }
    }

    /// Create an allow-list permission
    pub fn allow<I, S>(tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        TablePermissionMode::AllowList(tables.into_iter().map(Into::into).collect())
    }

    /// Create a deny-list permission.
    ///
    /// Note: Empty deny lists are rejected at [`SqliteConfigBuilder::build()`] time
    /// since they allow all tables, which is confusing. Use [`TablePermissionMode::AllowAll`] instead.
    pub fn deny<I, S>(tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        TablePermissionMode::DenyList(tables.into_iter().map(Into::into).collect())
    }

    /// Returns true if this is an empty deny list (which allows everything).
    fn is_empty_deny_list(&self) -> bool {
        matches!(self, TablePermissionMode::DenyList(tables) if tables.is_empty())
    }
}

/// Table permissions for read and write operations
#[derive(Debug, Clone, Default)]
pub struct TablePermissions {
    /// Tables allowed/denied for read operations (SELECT)
    pub read: TablePermissionMode,

    /// Tables allowed/denied for write operations (INSERT, UPDATE, DELETE)
    pub write: TablePermissionMode,
}

impl TablePermissions {
    /// Create permissions that allow all operations
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Create read-only permissions for specific tables
    pub fn read_only<I, S>(tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            read: TablePermissionMode::AllowList(tables.into_iter().map(Into::into).collect()),
            write: TablePermissionMode::AllowList(HashSet::new()), // Empty allow list denies all
        }
    }
}

/// Configuration for SQLite tools with permission constraints
#[derive(Debug, Clone, Default)]
pub struct SqliteConfig {
    /// Restrict tool to a specific database path.
    /// When set, the tool will ignore any db_path in the input and always use this path.
    pub db_path: Option<String>,

    /// Table-level permissions
    pub table_permissions: TablePermissions,
}

impl SqliteConfig {
    /// Create a new configuration with no restrictions
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder for constructing configuration
    pub fn builder() -> SqliteConfigBuilder {
        SqliteConfigBuilder::default()
    }

    /// Check if a table is allowed for read access
    pub fn can_read(&self, table: &str) -> bool {
        self.table_permissions.read.is_allowed(table)
    }

    /// Check if a table is allowed for write access
    pub fn can_write(&self, table: &str) -> bool {
        self.table_permissions.write.is_allowed(table)
    }

    /// Get the effective database path (configured path takes precedence)
    pub fn effective_db_path(&self, input_path: Option<String>) -> Option<String> {
        self.db_path.clone().or(input_path)
    }
}

/// Builder for SqliteConfig
#[derive(Debug, Clone, Default)]
pub struct SqliteConfigBuilder {
    db_path: Option<String>,
    table_permissions: TablePermissions,
}

impl SqliteConfigBuilder {
    /// Restrict to a specific database path
    pub fn db_path(mut self, path: impl Into<String>) -> Self {
        self.db_path = Some(path.into());
        self
    }

    /// Set read permissions
    pub fn read_tables(mut self, mode: TablePermissionMode) -> Self {
        self.table_permissions.read = mode;
        self
    }

    /// Set write permissions
    pub fn write_tables(mut self, mode: TablePermissionMode) -> Self {
        self.table_permissions.write = mode;
        self
    }

    /// Allow only specific tables for reading
    pub fn allow_read<I, S>(mut self, tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.table_permissions.read = TablePermissionMode::allow(tables);
        self
    }

    /// Deny specific tables for reading
    pub fn deny_read<I, S>(mut self, tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.table_permissions.read = TablePermissionMode::deny(tables);
        self
    }

    /// Allow only specific tables for writing
    pub fn allow_write<I, S>(mut self, tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.table_permissions.write = TablePermissionMode::allow(tables);
        self
    }

    /// Deny specific tables for writing
    pub fn deny_write<I, S>(mut self, tables: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.table_permissions.write = TablePermissionMode::deny(tables);
        self
    }

    /// Deny all write operations
    pub fn read_only(mut self) -> Self {
        // An empty allow list denies everything
        self.table_permissions.write = TablePermissionMode::AllowList(HashSet::new());
        self
    }

    /// Build the configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::EmptyDenyList`] if either read or write permissions
    /// use an empty deny list, since that allows all tables (use `AllowAll` instead).
    pub fn build(self) -> Result<SqliteConfig, ConfigError> {
        // Validate: empty deny lists are confusing since they allow everything
        if self.table_permissions.read.is_empty_deny_list() {
            return Err(ConfigError::EmptyDenyList { operation: "read" });
        }
        if self.table_permissions.write.is_empty_deny_list() {
            return Err(ConfigError::EmptyDenyList { operation: "write" });
        }

        Ok(SqliteConfig {
            db_path: self.db_path,
            table_permissions: self.table_permissions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_permission_mode_allow_all() {
        let mode = TablePermissionMode::AllowAll;
        assert!(mode.is_allowed("users"));
        assert!(mode.is_allowed("secrets"));
        assert!(mode.is_allowed("anything"));
    }

    #[test]
    fn test_table_permission_mode_allow_list() {
        let mode = TablePermissionMode::allow(["users", "orders"]);
        assert!(mode.is_allowed("users"));
        assert!(mode.is_allowed("orders"));
        assert!(!mode.is_allowed("secrets"));
        assert!(!mode.is_allowed("admin"));
    }

    #[test]
    fn test_table_permission_mode_deny_list() {
        let mode = TablePermissionMode::deny(["secrets", "admin_logs"]);
        assert!(mode.is_allowed("users"));
        assert!(mode.is_allowed("orders"));
        assert!(!mode.is_allowed("secrets"));
        assert!(!mode.is_allowed("admin_logs"));
    }

    #[test]
    fn test_empty_deny_list_returns_error_read() {
        let result = SqliteConfig::builder()
            .deny_read(Vec::<String>::new())
            .build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty deny list"));
        assert!(err.to_string().contains("read"));
    }

    #[test]
    fn test_empty_deny_list_returns_error_write() {
        let result = SqliteConfig::builder()
            .deny_write(Vec::<String>::new())
            .build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty deny list"));
        assert!(err.to_string().contains("write"));
    }

    #[test]
    fn test_sqlite_config_builder_db_path() {
        let config = SqliteConfig::builder()
            .db_path("/data/app.db")
            .build()
            .unwrap();

        assert_eq!(config.db_path, Some("/data/app.db".to_string()));
    }

    #[test]
    fn test_sqlite_config_builder_allow_read() {
        let config = SqliteConfig::builder()
            .allow_read(["users", "products"])
            .build()
            .unwrap();

        assert!(config.can_read("users"));
        assert!(config.can_read("products"));
        assert!(!config.can_read("secrets"));
    }

    #[test]
    fn test_sqlite_config_builder_deny_write() {
        let config = SqliteConfig::builder()
            .deny_write(["audit_log"])
            .build()
            .unwrap();

        assert!(config.can_write("users"));
        assert!(!config.can_write("audit_log"));
    }

    #[test]
    fn test_sqlite_config_builder_read_only() {
        let config = SqliteConfig::builder().read_only().build().unwrap();

        assert!(config.can_read("users"));
        assert!(!config.can_write("users"));
        assert!(!config.can_write("anything"));
    }

    #[test]
    fn test_effective_db_path_config_takes_precedence() {
        let config = SqliteConfig::builder()
            .db_path("/configured/path.db")
            .build()
            .unwrap();

        // Configured path should override input path
        assert_eq!(
            config.effective_db_path(Some("/input/path.db".to_string())),
            Some("/configured/path.db".to_string())
        );

        // Configured path should be used when input is None
        assert_eq!(
            config.effective_db_path(None),
            Some("/configured/path.db".to_string())
        );
    }

    #[test]
    fn test_effective_db_path_falls_back_to_input() {
        let config = SqliteConfig::new();

        // Without configured path, input path should be used
        assert_eq!(
            config.effective_db_path(Some("/input/path.db".to_string())),
            Some("/input/path.db".to_string())
        );

        // Without either, should be None
        assert_eq!(config.effective_db_path(None), None);
    }

    #[test]
    fn test_combined_permissions() {
        let config = SqliteConfig::builder()
            .db_path("/data/app.db")
            .allow_read(["users", "products", "orders"])
            .allow_write(["orders"])
            .build()
            .unwrap();

        // Read permissions
        assert!(config.can_read("users"));
        assert!(config.can_read("products"));
        assert!(config.can_read("orders"));
        assert!(!config.can_read("secrets"));

        // Write permissions
        assert!(!config.can_write("users"));
        assert!(!config.can_write("products"));
        assert!(config.can_write("orders"));
        assert!(!config.can_write("secrets"));
    }
}
