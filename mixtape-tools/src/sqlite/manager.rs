//! Database connection manager for SQLite tools
//!
//! Provides a singleton pattern for managing multiple database connections
//! across tool invocations.
//!
//! # Test Isolation
//!
//! For test isolation, create local `DatabaseManager` instances instead of using
//! the global `DATABASE_MANAGER`. Each instance has its own connection pool and
//! default database setting. Tool tests that use the global should call
//! `close_all()` in cleanup to reset state.

use crate::sqlite::error::SqliteToolError;
use lazy_static::lazy_static;
use mixtape_core::ToolError;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

lazy_static! {
    /// Global database manager instance
    pub static ref DATABASE_MANAGER: DatabaseManager = DatabaseManager::new();
}

/// Executes a closure with a database connection in a blocking task.
///
/// This helper abstracts the common pattern of:
/// 1. Spawning a blocking task for SQLite operations
/// 2. Acquiring a connection from the manager
/// 3. Locking the connection mutex
/// 4. Mapping errors to ToolError
///
/// # Example
///
/// ```ignore
/// let tables = with_connection(input.db_path, |conn| {
///     let mut stmt = conn.prepare("SELECT name FROM sqlite_master")?;
///     // ... use the connection
///     Ok(result)
/// }).await?;
/// ```
pub async fn with_connection<T, F>(db_path: Option<String>, f: F) -> Result<T, ToolError>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, SqliteToolError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = DATABASE_MANAGER.get(db_path.as_deref())?;
        let conn = conn.lock().unwrap();
        f(&conn)
    })
    .await
    .map_err(|e| ToolError::Custom(format!("Task join error: {}", e)))?
    .map_err(|e| e.into())
}

/// Manages SQLite database connections
///
/// Supports multiple simultaneous database connections, each identified
/// by a unique name derived from the file path.
///
/// # Test Isolation
///
/// - Create new `DatabaseManager` instances for isolated tests
/// - The global `DATABASE_MANAGER` is shared; use `close_all()` for cleanup
pub struct DatabaseManager {
    /// Open database connections keyed by normalized path
    connections: RwLock<HashMap<String, Arc<Mutex<Connection>>>>,

    /// The default database to use when none is specified
    default_db: RwLock<Option<String>>,
}

impl Default for DatabaseManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseManager {
    /// Creates a new database manager
    pub fn new() -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            default_db: RwLock::new(None),
        }
    }

    /// Normalizes a path to a consistent string key
    fn normalize_path(path: &Path) -> String {
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string()
    }

    /// Opens or creates a database connection
    ///
    /// If `create` is false and the database doesn't exist, returns an error.
    /// If the database is already open, returns the existing connection.
    ///
    /// Returns the database identifier (normalized path) for future reference.
    pub fn open(&self, path: &Path, create: bool) -> Result<String, SqliteToolError> {
        let key = Self::normalize_path(path);

        // Check if already open
        {
            let connections = self.connections.read().unwrap();
            if connections.contains_key(&key) {
                // Set as default if it's the first/only database
                self.set_default_if_first(&key);
                return Ok(key);
            }
        }

        // Check if file exists when create=false
        if !create && !path.exists() {
            return Err(SqliteToolError::DatabaseDoesNotExist(path.to_path_buf()));
        }

        // Ensure parent directory exists for new databases
        if create {
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }
        }

        // Open the connection
        let conn = Connection::open(path).map_err(|e| SqliteToolError::ConnectionFailed {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        // Enable foreign keys by default
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        let conn = Arc::new(Mutex::new(conn));

        // Store the connection
        {
            let mut connections = self.connections.write().unwrap();
            connections.insert(key.clone(), conn);
        }

        // Set as default if first database
        self.set_default_if_first(&key);

        Ok(key)
    }

    /// Sets a database as default if no default is set
    fn set_default_if_first(&self, key: &str) {
        let mut default = self.default_db.write().unwrap();
        if default.is_none() {
            *default = Some(key.to_string());
        }
    }

    /// Closes a database connection
    pub fn close(&self, name: &str) -> Result<(), SqliteToolError> {
        let mut connections = self.connections.write().unwrap();

        // Try to find by exact key or by filename
        let key = if connections.contains_key(name) {
            name.to_string()
        } else {
            // Search for matching filename
            connections
                .keys()
                .find(|k| k.ends_with(name) || Path::new(k).file_name().is_some_and(|f| f == name))
                .cloned()
                .ok_or_else(|| SqliteToolError::DatabaseNotFound(name.to_string()))?
        };

        connections.remove(&key);

        // Clear default if it was this database
        let mut default = self.default_db.write().unwrap();
        if default.as_ref() == Some(&key) {
            // Set to another open database or None
            *default = connections.keys().next().cloned();
        }

        Ok(())
    }

    /// Gets a connection by name, or the default connection if name is None
    pub fn get(&self, name: Option<&str>) -> Result<Arc<Mutex<Connection>>, SqliteToolError> {
        let connections = self.connections.read().unwrap();

        let key = match name {
            Some(n) => {
                // Try exact match first
                if connections.contains_key(n) {
                    n.to_string()
                } else {
                    // Search for matching filename
                    connections
                        .keys()
                        .find(|k| {
                            k.ends_with(n) || Path::new(k).file_name().is_some_and(|f| f == n)
                        })
                        .cloned()
                        .ok_or_else(|| SqliteToolError::DatabaseNotFound(n.to_string()))?
                }
            }
            None => {
                let default = self.default_db.read().unwrap();
                default.clone().ok_or(SqliteToolError::NoDefaultDatabase)?
            }
        };

        connections
            .get(&key)
            .cloned()
            .ok_or_else(|| SqliteToolError::DatabaseNotFound(key))
    }

    /// Sets the default database (thread-local)
    pub fn set_default(&self, name: &str) -> Result<(), SqliteToolError> {
        let connections = self.connections.read().unwrap();

        // Verify the database exists
        let key = if connections.contains_key(name) {
            name.to_string()
        } else {
            connections
                .keys()
                .find(|k| k.ends_with(name) || Path::new(k).file_name().is_some_and(|f| f == name))
                .cloned()
                .ok_or_else(|| SqliteToolError::DatabaseNotFound(name.to_string()))?
        };

        let mut default = self.default_db.write().unwrap();
        *default = Some(key);

        Ok(())
    }

    /// Returns the current default database name
    pub fn get_default(&self) -> Option<String> {
        self.default_db.read().unwrap().clone()
    }

    /// Lists all open database connections
    pub fn list_open(&self) -> Vec<String> {
        self.connections.read().unwrap().keys().cloned().collect()
    }

    /// Checks if a database is open
    pub fn is_open(&self, name: &str) -> bool {
        let connections = self.connections.read().unwrap();
        connections.contains_key(name)
            || connections
                .keys()
                .any(|k| k.ends_with(name) || Path::new(k).file_name().is_some_and(|f| f == name))
    }

    /// Closes all database connections and clears the default
    pub fn close_all(&self) {
        let mut connections = self.connections.write().unwrap();
        connections.clear();

        let mut default = self.default_db.write().unwrap();
        *default = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> DatabaseManager {
        DatabaseManager::new()
    }

    #[test]
    fn test_open_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let manager = create_test_manager();

        // Open database
        let key = manager.open(&db_path, true).unwrap();
        assert!(!key.is_empty());

        // Get connection
        let conn = manager.get(None).unwrap();
        let guard = conn.lock().unwrap();

        // Verify it works
        guard
            .execute_batch("CREATE TABLE test (id INTEGER);")
            .unwrap();
    }

    #[test]
    fn test_open_existing_only() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("nonexistent.db");
        let manager = create_test_manager();

        // Should fail when create=false and file doesn't exist
        let result = manager.open(&db_path, false);
        assert!(result.is_err());

        // Create the file first
        std::fs::write(&db_path, "").unwrap();

        // Now it should succeed (though this isn't a valid SQLite file,
        // rusqlite will handle it)
        // For a real test, we'd create it properly first
    }

    #[test]
    fn test_close_database() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let manager = create_test_manager();

        let key = manager.open(&db_path, true).unwrap();
        assert!(manager.is_open(&key));

        manager.close(&key).unwrap();
        assert!(!manager.is_open(&key));
    }

    #[test]
    fn test_multiple_databases() {
        let temp_dir = TempDir::new().unwrap();
        let db1_path = temp_dir.path().join("db1.db");
        let db2_path = temp_dir.path().join("db2.db");
        let manager = create_test_manager();

        let key1 = manager.open(&db1_path, true).unwrap();
        let key2 = manager.open(&db2_path, true).unwrap();

        // First opened should be default
        assert_eq!(manager.get_default(), Some(key1.clone()));

        // Can get both
        assert!(manager.get(Some(&key1)).is_ok());
        assert!(manager.get(Some(&key2)).is_ok());

        // List all
        let open = manager.list_open();
        assert_eq!(open.len(), 2);
    }

    #[test]
    fn test_set_default() {
        let temp_dir = TempDir::new().unwrap();
        let db1_path = temp_dir.path().join("db1.db");
        let db2_path = temp_dir.path().join("db2.db");
        let manager = create_test_manager();

        let key1 = manager.open(&db1_path, true).unwrap();
        let key2 = manager.open(&db2_path, true).unwrap();

        assert_eq!(manager.get_default(), Some(key1.clone()));

        manager.set_default(&key2).unwrap();
        assert_eq!(manager.get_default(), Some(key2));
    }

    #[test]
    fn test_no_default_database() {
        let manager = create_test_manager();
        let result = manager.get(None);
        assert!(matches!(result, Err(SqliteToolError::NoDefaultDatabase)));
    }

    #[test]
    fn test_close_all() {
        let temp_dir = TempDir::new().unwrap();
        let db1_path = temp_dir.path().join("db1.db");
        let db2_path = temp_dir.path().join("db2.db");
        let manager = create_test_manager();

        manager.open(&db1_path, true).unwrap();
        manager.open(&db2_path, true).unwrap();

        assert_eq!(manager.list_open().len(), 2);

        manager.close_all();

        assert_eq!(manager.list_open().len(), 0);
        assert!(manager.get_default().is_none());
    }
}
